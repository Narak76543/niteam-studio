//! Script generation job: outline call, then one call per chapter.
//! `project.json` is saved after the outline and after every chapter, so a crash or
//! cancel resumes at the first chapter that isn't `done`.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use super::ProjectError;
use super::hashing::{normalize, normalize_spoken};
use super::presets;
use super::project::{Outline, OutlineChapter, Project, ProjectStatus, Scene};
use super::store::ProjectStore;
use crate::gemini::client::ModelChain;
use crate::gemini::retry::RetryHooks;
use crate::gemini::script::{
    self, ChapterContext, ChapterReply, MOODS, OutlineReply, SceneReply, ScriptBrief,
};
use crate::gemini::{GeminiClient, GeminiError};
use crate::storage::ApiKey;
use crate::storage::log::LogFile;

/// Seconds of narration per scene (docs/03-pipeline.md: ~6–8 s).
const SECONDS_PER_SCENE: u32 = 7;
const SCENES_PER_CHAPTER: u32 = 5;
const MIN_CHAPTERS: u32 = 2;
const MAX_CHAPTERS: u32 = 10;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptStage {
    Outline,
    Chapter,
    Done,
    Failed,
    Cancelled,
}

/// Payload of the `script-progress` event.
#[derive(Debug, Clone, Serialize)]
pub struct ScriptProgress {
    pub project_id: String,
    pub stage: ScriptStage,
    pub chapters_done: usize,
    pub chapters_total: usize,
    /// Raw error text (for the "Details" section and the log).
    pub error: Option<String>,
    /// Stable code for a friendly, translated message (see `GeminiError::kind`).
    pub error_kind: Option<String>,
}

/// Scene count per chapter for a target length.
/// 3 min → 26 scenes in 5 chapters; 20 min → 171 scenes in 10 chapters.
pub fn plan_scenes(target_minutes: u32) -> Vec<u32> {
    let total = ((target_minutes * 60 + SECONDS_PER_SCENE / 2) / SECONDS_PER_SCENE).clamp(3, 200);
    let chapters = (total / SCENES_PER_CHAPTER).clamp(MIN_CHAPTERS, MAX_CHAPTERS);
    let base = total / chapters;
    let extra = total % chapters;
    (0..chapters).map(|i| base + u32::from(i < extra)).collect()
}

/// Tracks running jobs so a project can't run two at once and can be cancelled.
#[derive(Default)]
pub struct ScriptJobs {
    running: Mutex<HashMap<String, CancellationToken>>,
}

impl ScriptJobs {
    pub fn start(&self, project_id: &str) -> Result<CancellationToken, ProjectError> {
        let mut map = self.running.lock().unwrap_or_else(|p| p.into_inner());
        if map.contains_key(project_id) {
            return Err(ProjectError::Busy);
        }
        let token = CancellationToken::new();
        map.insert(project_id.to_owned(), token.clone());
        Ok(token)
    }

    pub fn finish(&self, project_id: &str) {
        let mut map = self.running.lock().unwrap_or_else(|p| p.into_inner());
        map.remove(project_id);
    }

    pub fn cancel(&self, project_id: &str) {
        let map = self.running.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(t) = map.get(project_id) {
            t.cancel();
        }
    }

    pub fn is_running(&self, project_id: &str) -> bool {
        let map = self.running.lock().unwrap_or_else(|p| p.into_inner());
        map.contains_key(project_id)
    }
}

pub struct ScriptJob<'a> {
    pub store: &'a ProjectStore,
    pub client: &'a GeminiClient,
    pub key: &'a ApiKey,
    /// Text model chain (`settings.models.text_chain`).
    pub models: &'a ModelChain,
    /// Cancellation + retry notices (the caller turns them into `gemini-retry` events).
    pub hooks: &'a RetryHooks,
    pub log: &'a LogFile,
}

impl ScriptJob<'_> {
    /// Runs to completion, reporting each step through `emit`.
    pub async fn run(&self, project_id: &str, emit: impl Fn(ScriptProgress)) {
        self.log.info(&format!(
            "script generation started (model chain: {})",
            self.models.models().join(" → ")
        ));
        let result = self.run_inner(project_id, &emit).await;
        let (stage, error, error_kind) = match result {
            Ok(()) => {
                self.log.info("script generation finished");
                (ScriptStage::Done, None, None)
            }
            Err(ProjectError::Gemini(GeminiError::Cancelled)) => {
                self.log.info("script generation cancelled");
                (ScriptStage::Cancelled, None, None)
            }
            Err(e) => {
                self.log.error(&format!(
                    "script generation failed [{}]: {}",
                    e.kind(),
                    e.detail()
                ));
                (
                    ScriptStage::Failed,
                    Some(e.detail()),
                    Some(e.kind().to_owned()),
                )
            }
        };
        let (done, total) = match self.store.get(project_id).await {
            Ok(p) => chapter_counts(&p),
            Err(_) => (0, 0),
        };
        emit(ScriptProgress {
            project_id: project_id.to_owned(),
            stage,
            chapters_done: done,
            chapters_total: total,
            error,
            error_kind,
        });
    }

    async fn run_inner(
        &self,
        id: &str,
        emit: &impl Fn(ScriptProgress),
    ) -> Result<(), ProjectError> {
        let mut project = self.store.get(id).await?;
        if project.status != ProjectStatus::ScriptDraft {
            return Err(ProjectError::WrongState("generating".into()));
        }
        let style_label = presets::find(&project.style.preset).map_or("", |p| p.label);

        if project.outline.is_none() {
            emit(progress(id, ScriptStage::Outline, 0, 0));
            let plan = plan_scenes(project.target_minutes);
            let brief = brief(&project, style_label);
            let total_scenes: u32 = plan.iter().sum();
            let (reply, model): (OutlineReply, String) = script::request_json(
                self.client,
                self.key,
                self.models,
                script::outline_prompt(&brief, plan.len(), total_scenes as usize),
                script::outline_schema(plan.len()),
                self.hooks,
            )
            .await?;
            self.log.info(&format!("outline written by {model}"));
            let outline = build_outline(reply, &plan);
            project = self
                .store
                .update(id, |p| {
                    p.title = outline.title.clone();
                    p.outline = Some(outline);
                    p.scenes.clear();
                    Ok(())
                })
                .await?
                .0;
        }

        let total = project.outline.as_ref().map_or(0, |o| o.chapters.len());
        for index in 0..total {
            let Some(outline) = project.outline.as_ref() else {
                break;
            };
            let Some(chapter) = outline.chapters.get(index) else {
                break;
            };
            if chapter.done {
                continue;
            }
            emit(progress(id, ScriptStage::Chapter, index, total));
            let previous = project
                .scenes
                .iter()
                .rev()
                .find(|s| s.chapter < index as u32)
                .map(|s| s.narration.clone());
            let ctx = ChapterContext {
                title: &outline.title,
                hook: &outline.hook,
                chapters: outline
                    .chapters
                    .iter()
                    .map(|c| (c.title.as_str(), c.summary.as_str()))
                    .collect(),
                index,
                scene_count: chapter.scene_count as usize,
                previous_narration: previous.as_deref(),
            };
            let brief = brief(&project, style_label);
            let (reply, model): (ChapterReply, String) = script::request_json(
                self.client,
                self.key,
                self.models,
                script::chapter_prompt(&brief, &ctx),
                script::chapter_schema(ctx.scene_count),
                self.hooks,
            )
            .await?;
            self.log.info(&format!(
                "chapter {}/{total} ({} scenes) written by {model}",
                index + 1,
                reply.scenes.len()
            ));
            project = self
                .store
                .update(id, |p| {
                    append_chapter(p, index, reply.scenes);
                    Ok(())
                })
                .await?
                .0;
        }
        Ok(())
    }
}

fn progress(id: &str, stage: ScriptStage, done: usize, total: usize) -> ScriptProgress {
    ScriptProgress {
        project_id: id.to_owned(),
        stage,
        chapters_done: done,
        chapters_total: total,
        error: None,
        error_kind: None,
    }
}

fn brief<'a>(p: &'a Project, style_label: &'a str) -> ScriptBrief<'a> {
    ScriptBrief {
        topic: &p.topic,
        language: p.language,
        target_minutes: p.target_minutes,
        style_label,
    }
}

pub fn chapter_counts(p: &Project) -> (usize, usize) {
    p.outline.as_ref().map_or((0, 0), |o| {
        (
            o.chapters.iter().filter(|c| c.done).count(),
            o.chapters.len(),
        )
    })
}

fn build_outline(reply: OutlineReply, plan: &[u32]) -> Outline {
    // The schema asks for exactly plan.len() chapters; tolerate a model that returns
    // fewer by keeping its chapters and folding the extra scenes into the last one.
    let n = reply.chapters.len().min(plan.len()).max(1);
    let mut counts: Vec<u32> = plan.iter().take(n).copied().collect();
    let leftover: u32 = plan.iter().skip(n).sum();
    if let Some(last) = counts.last_mut() {
        *last += leftover;
    }
    Outline {
        title: normalize(&reply.title),
        hook: normalize(&reply.hook),
        chapters: reply
            .chapters
            .into_iter()
            .take(n)
            .zip(counts)
            .map(|(c, scene_count)| OutlineChapter {
                title: normalize(&c.title),
                summary: normalize(&c.summary),
                scene_count,
                done: false,
            })
            .collect(),
    }
}

fn append_chapter(p: &mut Project, index: usize, replies: Vec<SceneReply>) {
    let chapter = index as u32;
    // A retried chapter must not duplicate scenes.
    p.scenes.retain(|s| s.chapter != chapter);
    // Keep chapters in story order even if the user added scenes meanwhile.
    let insert_at = p
        .scenes
        .iter()
        .position(|s| s.chapter > chapter)
        .unwrap_or(p.scenes.len());
    let mut new_scenes = Vec::with_capacity(replies.len());
    for r in replies {
        let mut scene = Scene::new(p.next_scene_id_after(&new_scenes), chapter);
        scene.narration = normalize(&r.narration);
        scene.spoken_text = normalize_spoken(&r.spoken_text);
        scene.image_prompt = normalize(&r.image_prompt);
        scene.mood = if MOODS.contains(&r.mood.as_str()) {
            r.mood
        } else {
            "calm".into()
        };
        let note = normalize(&r.fact_note);
        scene.fact_note = (!note.is_empty()).then_some(note);
        new_scenes.push(scene);
    }
    p.scenes.splice(insert_at..insert_at, new_scenes);
    p.renumber();
    if let Some(c) = p.outline.as_mut().and_then(|o| o.chapters.get_mut(index)) {
        c.done = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gemini::script::OutlineChapterReply;
    use crate::pipeline::project::tests::sample;

    #[test]
    fn three_minutes_is_about_25_scenes() {
        let plan = plan_scenes(3);
        let total: u32 = plan.iter().sum();
        assert_eq!(total, 26);
        assert_eq!(plan.len(), 5);
    }

    #[test]
    fn plan_bounds() {
        assert_eq!(plan_scenes(1).len(), 2);
        let long = plan_scenes(20);
        assert_eq!(long.len(), 10);
        assert_eq!(long.iter().sum::<u32>(), 171);
    }

    fn reply(n: usize) -> Vec<SceneReply> {
        (0..n)
            .map(|i| SceneReply {
                narration: format!("n{i}"),
                spoken_text: format!("s{i}"),
                image_prompt: "p".into(),
                mood: "curious".into(),
                fact_note: String::new(),
            })
            .collect()
    }

    #[test]
    fn chapters_append_in_order_and_retry_replaces() {
        let mut p = sample();
        p.outline = Some(build_outline(
            OutlineReply {
                title: "T".into(),
                hook: "H".into(),
                chapters: vec![
                    OutlineChapterReply {
                        title: "a".into(),
                        summary: "a".into(),
                    },
                    OutlineChapterReply {
                        title: "b".into(),
                        summary: "b".into(),
                    },
                ],
            },
            &[2, 3],
        ));
        append_chapter(&mut p, 1, reply(3));
        append_chapter(&mut p, 0, reply(2));
        let chapters: Vec<u32> = p.scenes.iter().map(|s| s.chapter).collect();
        assert_eq!(chapters, vec![0, 0, 1, 1, 1]);
        assert!(p.script_complete());
        append_chapter(&mut p, 0, reply(2));
        assert_eq!(p.scenes.len(), 5);
        let ids: std::collections::HashSet<_> = p.scenes.iter().map(|s| &s.id).collect();
        assert_eq!(ids.len(), 5);
    }

    #[test]
    fn short_outline_folds_extra_scenes() {
        let o = build_outline(
            OutlineReply {
                title: "T".into(),
                hook: "H".into(),
                chapters: vec![OutlineChapterReply {
                    title: "a".into(),
                    summary: "a".into(),
                }],
            },
            &[5, 5, 4],
        );
        assert_eq!(o.chapters.len(), 1);
        assert_eq!(o.chapters[0].scene_count, 14);
    }

    #[test]
    fn jobs_are_exclusive() {
        let jobs = ScriptJobs::default();
        let t = jobs.start("a").unwrap();
        assert!(jobs.start("a").is_err());
        jobs.cancel("a");
        assert!(t.is_cancelled());
        jobs.finish("a");
        assert!(!jobs.is_running("a"));
    }

    /// Live end-to-end check against the real Gemini API with the key from Windows
    /// Credential Manager. Not run by default (costs a few requests):
    /// `cargo test --manifest-path src-tauri/Cargo.toml live_khmer -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "calls the real Gemini API"]
    async fn live_khmer_one_minute_script() {
        use std::sync::Arc;

        use crate::gemini::retry::RetryNotice;
        use crate::pipeline::editing::{NewProjectInput, new_project};
        use crate::pipeline::project::Language;
        use crate::storage::settings::Settings;
        use crate::storage::{AppPaths, secrets};

        let key = secrets::get_api_key()
            .expect("keyring")
            .expect("no API key saved in Settings");
        let settings = Settings::default();
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(dir.path().to_path_buf());
        let store = ProjectStore::new(paths.clone());
        let client = GeminiClient::new(&settings.concurrency).unwrap();
        let chain = ModelChain::new(&settings.models.text_chain);

        let project = new_project(
            &NewProjectInput {
                topic: "How was Angkor Wat built?".into(),
                language: Language::Km,
                target_minutes: 1,
                style_preset: "storybook_watercolor".into(),
            },
            &settings.models.tts,
        )
        .unwrap();
        store.insert(&project).await.unwrap();

        let log = LogFile::new(paths.project_log(&project.id)).redacting(key.expose());
        let hooks = RetryHooks::new(
            Default::default(),
            Arc::new(|n: &RetryNotice| {
                eprintln!(
                    "[retry] {:?} model={} attempt={}/{} delay={}ms :: {}",
                    n.reason, n.model, n.attempt, n.max_attempts, n.delay_ms, n.error
                )
            }),
        );
        let job = ScriptJob {
            store: &store,
            client: &client,
            key: &key,
            models: &chain,
            hooks: &hooks,
            log: &log,
        };
        let last = std::sync::Mutex::new(None);
        job.run(&project.id, |p| {
            eprintln!(
                "[progress] {:?} {}/{}",
                p.stage, p.chapters_done, p.chapters_total
            );
            *last.lock().unwrap() = Some(p);
        })
        .await;

        let log_text = std::fs::read_to_string(paths.project_log(&project.id)).unwrap_or_default();
        eprintln!(
            "---- pipeline.log ----
{log_text}"
        );
        assert!(!log_text.contains(key.expose()), "key leaked into the log");

        let p = store.get(&project.id).await.unwrap();
        eprintln!("---- title: {} ----", p.title);
        for s in &p.scenes {
            eprintln!(
                "{} [ch{}] {}
      spoken: {}
      image: {}",
                s.id,
                s.chapter + 1,
                s.narration,
                s.spoken_text,
                s.image_prompt
            );
        }
        let last = last.lock().unwrap().take().expect("no progress event");
        assert!(
            matches!(last.stage, ScriptStage::Done),
            "failed: {:?}",
            last.error
        );
        assert!(p.script_complete());
        assert!(
            p.scenes.len() >= 6,
            "expected ~9 scenes, got {}",
            p.scenes.len()
        );
    }
}
