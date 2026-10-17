//! "Improve image prompts" (Scenes page): rewrites every scene's `image_prompt` with the
//! current rules in batches, without touching the narration. Each batch is saved as it
//! arrives (resumable: a re-run simply rewrites again); images whose prompt changed become
//! outdated and are made again by the next image job.

use std::sync::Arc;

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use super::ProjectError;
use super::hashing::{self, normalize};
use super::presets;
use super::project::{Project, ProjectStatus, Scene};
use super::store::ProjectStore;
use crate::gemini::client::ModelChain;
use crate::gemini::image_prompts::{self, PromptContext, PromptInput, PromptOut, PromptReply};
use crate::gemini::retry::RetryHooks;
use crate::gemini::script::{BibleKeys, request_json};
use crate::gemini::{GeminiClient, GeminiError};
use crate::storage::ApiKey;
use crate::storage::log::LogFile;

/// Scenes per Gemini call.
pub const BATCH: usize = 12;

/// Stores one batch. Scenes whose narration changed since sending are left alone.
/// Returns how many prompts changed.
pub fn apply_batch(p: &mut Project, sent: &[(String, String)], out: &[PromptOut]) -> Result<usize, ProjectError> {
    if p.status == ProjectStatus::Rendering {
        return Err(ProjectError::WrongState("rendering".into()));
    }
    let mut changed = 0;
    for (id, narration_at_send) in sent {
        let Some(new) = out.iter().find(|o| &o.scene_id == id).map(|o| normalize(&o.image_prompt)) else {
            continue;
        };
        let Some(scene) = p.scenes.iter_mut().find(|s| &s.id == id) else {
            continue;
        };
        if &scene.narration != narration_at_send || new.is_empty() || new == scene.image_prompt {
            continue;
        }
        scene.image_prompt = new;
        changed += 1;
    }
    if changed > 0 {
        hashing::mark_all_images_stale(p);
    }
    Ok(changed)
}

fn bible_keys(p: &Project) -> BibleKeys<'_> {
    p.visual_bible.as_ref().map_or_else(BibleKeys::default, |b| BibleKeys {
        characters: b.characters.iter().map(|(k, d)| (k.as_str(), d.as_str())).collect(),
        places: b.places.iter().map(|(k, d)| (k.as_str(), d.as_str())).collect(),
    })
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RewriteStage {
    Running,
    Done,
    Failed,
    Cancelled,
}

/// Payload of `prompt-rewrite-progress`.
#[derive(Debug, Clone, Serialize)]
pub struct RewriteProgress {
    pub project_id: String,
    pub stage: RewriteStage,
    pub done: usize,
    pub total: usize,
    /// Prompts that changed so far.
    pub changed: usize,
    pub error: Option<String>,
    pub error_kind: Option<String>,
}

pub trait RewriteEvents: Send + Sync {
    fn progress(&self, p: RewriteProgress);
}

pub struct RewriteJob {
    pub store: Arc<ProjectStore>,
    pub client: GeminiClient,
    pub key: ApiKey,
    pub chain: ModelChain,
    pub hooks: RetryHooks,
    pub log: LogFile,
}

impl RewriteJob {
    pub async fn run(&self, project_id: &str, cancel: CancellationToken, events: Arc<dyn RewriteEvents>) {
        let (mut done, mut total, mut changed) = (0, 0, 0);
        let send = |stage, done, total, changed, err: Option<&ProjectError>| {
            events.progress(RewriteProgress {
                project_id: project_id.to_owned(),
                stage,
                done,
                total,
                changed,
                error: err.map(ProjectError::detail),
                error_kind: err.map(|e| e.kind().to_owned()),
            });
        };
        let result: Result<(), ProjectError> = async {
            let project = self.store.get(project_id).await?;
            let ids: Vec<String> = project.scenes.iter().filter(|s| !s.narration.trim().is_empty()).map(|s| s.id.clone()).collect();
            total = ids.len();
            self.log.info(&format!("image prompt rewrite started: {total} scene(s)"));
            send(RewriteStage::Running, 0, total, 0, None);
            let mut previous: Option<String> = None;
            for chunk in ids.chunks(BATCH) {
                if cancel.is_cancelled() {
                    return Err(ProjectError::Gemini(GeminiError::Cancelled));
                }
                let project = self.store.get(project_id).await?;
                let scenes: Vec<&Scene> = chunk.iter().filter_map(|id| project.scenes.iter().find(|s| &s.id == id)).collect();
                let inputs: Vec<PromptInput<'_>> = scenes
                    .iter()
                    .map(|s| PromptInput {
                        scene_id: &s.id,
                        narration: &s.narration,
                        current_image_prompt: &s.image_prompt,
                        characters: &s.characters,
                        place: s.place.as_deref().unwrap_or(""),
                    })
                    .collect();
                let sent: Vec<(String, String)> = scenes.iter().map(|s| (s.id.clone(), s.narration.clone())).collect();
                let ids: Vec<&str> = scenes.iter().map(|s| s.id.as_str()).collect();
                let ctx = PromptContext {
                    topic: &project.topic,
                    title: &project.title,
                    stick_figure: presets::is_stick_figure(&project.style.style_prompt),
                    bible: bible_keys(&project),
                    previous: previous.as_deref(),
                };
                let request = request_json::<PromptReply>(
                    &self.client,
                    &self.key,
                    &self.chain,
                    image_prompts::prompt(&ctx, &inputs),
                    image_prompts::schema(&ids),
                    &self.hooks,
                );
                let (reply, model) = tokio::select! {
                    () = cancel.cancelled() => return Err(ProjectError::Gemini(GeminiError::Cancelled)),
                    r = request => r?,
                };
                let last_id = sent.last().map(|s| s.0.clone());
                let (saved, n) = self.store.update(project_id, |p| apply_batch(p, &sent, &reply.scenes)).await?;
                previous = last_id.and_then(|id| saved.scenes.iter().find(|s| s.id == id).map(|s| s.image_prompt.clone()));
                done += sent.len();
                changed += n;
                self.log.info(&format!("image prompts {done}/{total} by {model}: {n} changed"));
                send(RewriteStage::Running, done, total, changed, None);
            }
            Ok(())
        }
        .await;
        match result {
            Ok(()) => send(RewriteStage::Done, done, total, changed, None),
            Err(ProjectError::Gemini(GeminiError::Cancelled)) => send(RewriteStage::Cancelled, done, total, changed, None),
            Err(e) => {
                self.log.error(&format!("image prompt rewrite failed: {}", e.detail()));
                send(RewriteStage::Failed, done, total, changed, Some(&e));
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
        for id in ["s001", "s002"] {
            let mut s = Scene::new(id.into(), 0);
            s.narration = format!("narration {id}");
            s.image_prompt = "old".into();
            p.scenes.push(s);
        }
        p
    }

    fn out(id: &str, t: &str) -> PromptOut {
        PromptOut { scene_id: id.into(), image_prompt: t.into() }
    }

    #[test]
    fn new_prompts_are_stored_unless_the_narration_changed() {
        let mut p = project();
        let sent: Vec<(String, String)> = p.scenes.iter().map(|s| (s.id.clone(), s.narration.clone())).collect();
        p.scenes[1].narration = "edited meanwhile".into();
        let n = apply_batch(&mut p, &sent, &[out("s001", "a farmer plants rice"), out("s002", "x")]).unwrap();
        assert_eq!(n, 1);
        assert_eq!(p.scenes[0].image_prompt, "a farmer plants rice");
        assert_eq!(p.scenes[1].image_prompt, "old");
    }

    #[test]
    fn not_while_rendering() {
        let mut p = project();
        p.status = ProjectStatus::Rendering;
        assert!(apply_batch(&mut p, &[], &[]).is_err());
    }
}
