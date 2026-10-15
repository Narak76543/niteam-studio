//! Proofreading job (Script page → "Check grammar"): sends the narration to Gemini in
//! batches, stores each correction on its scene as a suggestion the user accepts or
//! dismisses. Resumable: `scene.proofread_hash` records what was checked, so a re-run only
//! sends scenes whose text changed. Nothing is changed without the user's Accept.

use std::sync::Arc;

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use super::ProjectError;
use super::hashing::{self, normalize, normalize_spoken, sha};
use super::project::{Language, Project, ProjectStatus, ProofSuggestion, Scene};
use super::store::ProjectStore;
use crate::gemini::client::ModelChain;
use crate::gemini::proofread::{self, ProofFix, ProofInput, ProofReply};
use crate::gemini::retry::RetryHooks;
use crate::gemini::script::request_json;
use crate::gemini::{GeminiClient, GeminiError};
use crate::storage::ApiKey;
use crate::storage::log::LogFile;

/// Scenes per Gemini call (keeps replies small and retries cheap).
pub const BATCH: usize = 15;
const MAX_TERMS: usize = 50;
const MAX_TERM_CHARS: usize = 60;

/// What the proofreader was given for a scene; a suggestion is only valid for this text.
pub fn proof_hash(p: &Project, s: &Scene) -> String {
    let language = match p.language {
        Language::Km => "km",
        Language::En => "en",
    };
    sha(&[
        "proof",
        &normalize(&s.narration),
        &normalize_spoken(&s.spoken_text),
        language,
        &p.keep_terms.join("|"),
    ])
}

/// Scene ids not yet checked in their current form, in story order.
pub fn pending(p: &Project) -> Vec<String> {
    p.scenes
        .iter()
        .filter(|s| !s.narration.trim().is_empty() && s.proofread_hash.as_deref() != Some(proof_hash(p, s).as_str()))
        .map(|s| s.id.clone())
        .collect()
}

/// Every user term found in `before` must still be in `after` (case-sensitive).
fn keeps_terms(terms: &[String], before: &str, after: &str) -> bool {
    terms.iter().filter(|t| !t.trim().is_empty() && before.contains(t.trim())).all(|t| after.contains(t.trim()))
}

/// Stores one batch's result. Scenes edited meanwhile (hash changed) are left alone.
/// Returns the number of new suggestions.
pub fn apply_batch(p: &mut Project, sent: &[(String, String)], fixes: &[ProofFix]) -> usize {
    let mut added = 0;
    for (id, hash_at_send) in sent {
        let Some(idx) = p.scenes.iter().position(|s| &s.id == id) else {
            continue;
        };
        let current = p.scenes.get(idx).map(|s| proof_hash(p, s));
        if current.as_deref() != Some(hash_at_send.as_str()) {
            continue;
        }
        let terms = p.keep_terms.clone();
        let Some(scene) = p.scenes.get_mut(idx) else {
            continue;
        };
        let fix = fixes.iter().find(|f| &f.scene_id == id).filter(|f| {
            let narration = normalize(&f.narration);
            let spoken = normalize_spoken(&f.spoken_text);
            (narration != scene.narration || spoken != scene.spoken_text)
                && keeps_terms(&terms, &scene.narration, &narration)
        });
        scene.proofread = fix.map(|f| ProofSuggestion {
            narration: normalize(&f.narration),
            spoken_text: normalize_spoken(&f.spoken_text),
            note: normalize(&f.note),
        });
        if scene.proofread.is_some() {
            added += 1;
        }
        scene.proofread_hash = Some(hash_at_send.clone());
    }
    added
}

fn require_draft(p: &Project) -> Result<(), ProjectError> {
    if p.status == ProjectStatus::ScriptDraft {
        Ok(())
    } else {
        Err(ProjectError::WrongState(super::editing::status_name(p.status).into()))
    }
}

/// Accept: the suggestion becomes the scene text (voice etc. become outdated as for any edit).
pub fn accept(p: &mut Project, ids: &[String]) -> Result<usize, ProjectError> {
    require_draft(p)?;
    let mut n = 0;
    for i in 0..p.scenes.len() {
        let take = p.scenes.get(i).is_some_and(|s| ids.contains(&s.id) && s.proofread.is_some());
        if !take {
            continue;
        }
        if let Some(scene) = p.scenes.get_mut(i)
            && let Some(sug) = scene.proofread.take()
        {
            scene.narration = sug.narration;
            scene.spoken_text = sug.spoken_text;
            n += 1;
        }
        hashing::mark_stale(p, i);
        // The accepted text counts as checked.
        let h = p.scenes.get(i).map(|s| proof_hash(p, s));
        if let Some(scene) = p.scenes.get_mut(i) {
            scene.proofread_hash = h;
        }
    }
    Ok(n)
}

/// Dismiss: keep the current text; the scene is not checked again until it changes.
pub fn dismiss(p: &mut Project, ids: &[String]) {
    for s in p.scenes.iter_mut().filter(|s| ids.contains(&s.id)) {
        s.proofread = None;
    }
}

/// Names that stay as written (trimmed, NFC, deduplicated, limited).
pub fn set_keep_terms(p: &mut Project, terms: &[String]) -> Result<(), ProjectError> {
    let mut out: Vec<String> = Vec::new();
    for t in terms {
        let t = normalize(t);
        if t.is_empty() || out.contains(&t) {
            continue;
        }
        if t.chars().count() > MAX_TERM_CHARS {
            return Err(ProjectError::Invalid(format!("A name is too long (max {MAX_TERM_CHARS} characters).")));
        }
        out.push(t);
    }
    if out.len() > MAX_TERMS {
        return Err(ProjectError::Invalid(format!("Too many names (max {MAX_TERMS}).")));
    }
    p.keep_terms = out;
    Ok(())
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProofStage {
    Running,
    Done,
    Failed,
    Cancelled,
}

/// Payload of `proofread-progress`.
#[derive(Debug, Clone, Serialize)]
pub struct ProofProgress {
    pub project_id: String,
    pub stage: ProofStage,
    pub checked: usize,
    pub total: usize,
    /// Suggestions found in this run so far.
    pub suggestions: usize,
    pub error: Option<String>,
    pub error_kind: Option<String>,
}

pub trait ProofEvents: Send + Sync {
    fn progress(&self, p: ProofProgress);
}

pub struct ProofJob {
    pub store: Arc<ProjectStore>,
    pub client: GeminiClient,
    pub key: ApiKey,
    pub chain: ModelChain,
    pub hooks: RetryHooks,
    pub log: LogFile,
}

impl ProofJob {
    pub async fn run(&self, project_id: &str, cancel: CancellationToken, events: Arc<dyn ProofEvents>) {
        let mut checked = 0;
        let mut found = 0;
        let mut total = 0;
        let send = |stage, checked, total, found, err: Option<&ProjectError>| {
            events.progress(ProofProgress {
                project_id: project_id.to_owned(),
                stage,
                checked,
                total,
                suggestions: found,
                error: err.map(ProjectError::detail),
                error_kind: err.map(|e| e.kind().to_owned()),
            });
        };
        let result: Result<(), ProjectError> = async {
            let project = self.store.get(project_id).await?;
            let ids = pending(&project);
            total = ids.len();
            self.log.info(&format!("proofread started: {total} scene(s) to check"));
            send(ProofStage::Running, 0, total, 0, None);
            for chunk in ids.chunks(BATCH) {
                if cancel.is_cancelled() {
                    return Err(ProjectError::Gemini(GeminiError::Cancelled));
                }
                let project = self.store.get(project_id).await?;
                let scenes: Vec<&Scene> = chunk.iter().filter_map(|id| project.scenes.iter().find(|s| &s.id == id)).collect();
                let inputs: Vec<ProofInput<'_>> = scenes
                    .iter()
                    .map(|s| ProofInput { scene_id: &s.id, narration: &s.narration, spoken_text: &s.spoken_text })
                    .collect();
                let sent: Vec<(String, String)> = scenes.iter().map(|s| (s.id.clone(), proof_hash(&project, s))).collect();
                let ids: Vec<&str> = scenes.iter().map(|s| s.id.as_str()).collect();
                let prompt = proofread::prompt(project.language, &project.topic, &project.keep_terms, &inputs);
                let request = request_json::<ProofReply>(
                    &self.client,
                    &self.key,
                    &self.chain,
                    prompt,
                    proofread::schema(&ids),
                    &self.hooks,
                );
                let (reply, model) = tokio::select! {
                    () = cancel.cancelled() => return Err(ProjectError::Gemini(GeminiError::Cancelled)),
                    r = request => r?,
                };
                let (_, added) = self
                    .store
                    .update(project_id, |p| Ok(apply_batch(p, &sent, &reply.fixes)))
                    .await?;
                checked += sent.len();
                found += added;
                self.log.info(&format!("proofread {checked}/{total} by {model}: {added} suggestion(s)"));
                send(ProofStage::Running, checked, total, found, None);
            }
            Ok(())
        }
        .await;
        match result {
            Ok(()) => send(ProofStage::Done, checked, total, found, None),
            Err(ProjectError::Gemini(GeminiError::Cancelled)) => send(ProofStage::Cancelled, checked, total, found, None),
            Err(e) => {
                self.log.error(&format!("proofread failed: {}", e.detail()));
                send(ProofStage::Failed, checked, total, found, Some(&e));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::project::tests::sample;

    fn project() -> Project {
        let mut p = sample();
        for (id, n) in [("s001", "React Native ជាអ្វី?"), ("s002", "វាល្អណាស")] {
            let mut s = Scene::new(id.into(), 0);
            s.narration = n.into();
            s.spoken_text = n.into();
            p.scenes.push(s);
        }
        p.keep_terms = vec!["React Native".into()];
        p
    }

    fn fix(id: &str, n: &str) -> ProofFix {
        ProofFix { scene_id: id.into(), narration: n.into(), spoken_text: n.into(), note: "ok".into() }
    }

    fn sent(p: &Project) -> Vec<(String, String)> {
        p.scenes.iter().map(|s| (s.id.clone(), proof_hash(p, s))).collect()
    }

    #[test]
    fn suggestions_are_stored_and_checked_scenes_skipped_next_time() {
        let mut p = project();
        assert_eq!(pending(&p).len(), 2);
        let s = sent(&p);
        let added = apply_batch(&mut p, &s, &[fix("s002", "វាល្អណាស់")]);
        assert_eq!(added, 1);
        assert_eq!(p.scenes[1].proofread.as_ref().map(|x| x.narration.as_str()), Some("វាល្អណាស់"));
        assert!(p.scenes[0].proofread.is_none());
        assert!(pending(&p).is_empty());
        p.scenes[0].narration = "React Native គឺជាអ្វី?".into();
        assert_eq!(pending(&p), vec!["s001".to_owned()]);
    }

    #[test]
    fn a_fix_that_translates_a_kept_name_is_dropped() {
        let mut p = project();
        let s = sent(&p);
        let added = apply_batch(&mut p, &s, &[fix("s001", "រីអេកណេធីវ ជាអ្វី?")]);
        assert_eq!(added, 0);
        assert!(p.scenes[0].proofread.is_none());
    }

    #[test]
    fn scenes_edited_while_checking_are_left_alone() {
        let mut p = project();
        let s = sent(&p);
        p.scenes[1].narration = "edited".into();
        assert_eq!(apply_batch(&mut p, &s, &[fix("s002", "វាល្អណាស់")]), 0);
        assert!(p.scenes[1].proofread_hash.is_none());
    }

    #[test]
    fn accept_applies_text_and_dismiss_keeps_it() {
        let mut p = project();
        let s = sent(&p);
        apply_batch(&mut p, &s, &[fix("s002", "វាល្អណាស់")]);
        let mut q = p.clone();
        assert_eq!(accept(&mut p, &["s002".into()]).unwrap(), 1);
        assert_eq!(p.scenes[1].narration, "វាល្អណាស់");
        assert!(p.scenes[1].proofread.is_none());
        assert!(pending(&p).is_empty(), "accepted text counts as checked");

        dismiss(&mut q, &["s002".into()]);
        assert_eq!(q.scenes[1].narration, "វាល្អណាស");
        assert!(q.scenes[1].proofread.is_none());
        assert!(pending(&q).is_empty());
    }

    #[test]
    fn accept_needs_the_script_draft() {
        let mut p = project();
        p.status = ProjectStatus::Review;
        assert!(accept(&mut p, &["s001".into()]).is_err());
    }

    #[test]
    fn keep_terms_are_cleaned() {
        let mut p = sample();
        set_keep_terms(&mut p, &[" Android ".into(), "Android".into(), "".into(), "React Native".into()]).unwrap();
        assert_eq!(p.keep_terms, vec!["Android".to_owned(), "React Native".to_owned()]);
        assert!(set_keep_terms(&mut p, &["x".repeat(61)]).is_err());
    }
}
