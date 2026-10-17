//! Script generation job: outline call, then one call per chapter.
//! `project.json` is saved after the outline and after every chapter, so a crash or
//! cancel resumes at the first chapter that isn't `done`.

use serde::Serialize;

use super::ProjectError;
use super::hashing::{normalize, normalize_spoken};
use super::presets;
use super::project::Shot;
use super::project::{Outline, OutlineChapter, Project, ProjectStatus, Scene, SourceStage};
use super::store::ProjectStore;
use super::visual_bible;
use crate::gemini::client::ModelChain;
use crate::gemini::retry::RetryHooks;
use crate::gemini::document;
use crate::gemini::script::{
    self, BibleKeys, ChapterContext, ChapterReply, MOODS, OutlineReply, SceneReply, ScriptBrief,
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
        if project.source.as_ref().is_some_and(|s| s.stage != SourceStage::Confirmed) {
            return Err(ProjectError::WrongState("source_review".into()));
        }
        let style_label = presets::find(&project.style.preset).map_or("", |p| p.label);
        let source = source_text(&project);

        if project.outline.is_none() {
            emit(progress(id, ScriptStage::Outline, 0, 0));
            let plan = plan_scenes(project.target_minutes);
            let brief = brief(&project, style_label, source.as_deref());
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

        // Visual Bible right after the outline, so chapter scenes can refer to its keys.
        if project.visual_bible.is_none() {
            emit(progress(id, ScriptStage::Outline, 0, 0));
            let (bible, model) =
                visual_bible::generate(self.client, self.key, self.models, &project, self.hooks)
                    .await?;
            self.log.info(&format!(
                "visual bible written by {model}: {} character(s), {} place(s)",
                bible.characters.len(),
                bible.places.len()
            ));
            project = self
                .store
                .update(id, |p| {
                    p.visual_bible = Some(bible);
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
                previous_shots: project
                    .scenes
                    .iter()
                    .filter(|s| s.chapter < index as u32)
                    .rev()
                    .take(2)
                    .map(|s| s.shot.as_str())
                    .collect(),
                bible: project
                    .visual_bible
                    .as_ref()
                    .map_or_else(BibleKeys::default, |b| BibleKeys {
                        characters: b
                            .characters
                            .iter()
                            .map(|(k, d)| (k.as_str(), d.as_str()))
                            .collect(),
                        places: b
                            .places
                            .iter()
                            .map(|(k, d)| (k.as_str(), d.as_str()))
                            .collect(),
                    }),
            };
            let brief = brief(&project, style_label, source.as_deref());
            let (reply, model): (ChapterReply, String) = script::request_json(
                self.client,
                self.key,
                self.models,
                script::chapter_prompt(&brief, &ctx),
                script::chapter_schema(ctx.scene_count, &ctx.bible, source.is_some()),
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

/// `document::source_block` for "From document" projects.
fn source_text(p: &Project) -> Option<String> {
    let s = p.source.as_ref()?;
    let summary = s.summary.as_ref()?;
    let sections: Vec<_> = s.chunks.iter().flat_map(|c| c.sections.iter().cloned()).collect();
    Some(document::source_block(summary, &sections, s.audience, &s.focus))
}

fn brief<'a>(p: &'a Project, style_label: &'a str, source: Option<&'a str>) -> ScriptBrief<'a> {
    ScriptBrief {
        topic: &p.topic,
        language: p.language,
        target_minutes: p.target_minutes,
        style_label,
        figures: super::presets::figures(&p.style.style_prompt),
        keep_terms: &p.keep_terms,
        source,
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
        let bible = p.visual_bible.as_ref();
        scene.characters = r
            .characters
            .iter()
            .map(|k| visual_bible::normalize_key(k))
            .filter(|k| bible.is_some_and(|b| b.characters.contains_key(k)))
            .collect();
        scene.characters.dedup();
        let place = visual_bible::normalize_key(&r.place);
        scene.place = bible
            .filter(|b| b.places.contains_key(&place))
            .map(|_| place);
        scene.shot = Shot::parse(&r.shot).unwrap_or_default();
        scene.key_scene = r.key_scene;
        if let Some(s) = &p.source {
            let mut pages: Vec<u32> =
                r.source_pages.iter().copied().filter(|n| (s.page_from..=s.page_to).contains(n)).collect();
            pages.sort_unstable();
            pages.dedup();
            // General knowledge is not a claim from the document: no page refs.
            scene.source_pages = if r.general_knowledge { vec![] } else { pages };
            scene.general_knowledge = r.general_knowledge;
        }
        new_scenes.push(scene);
    }
    p.scenes.splice(insert_at..insert_at, new_scenes);
    p.renumber();
    // Enforce the 07 §2 rules regardless of what the model marked.
    visual_bible::apply_key_scenes(p);
    visual_bible::vary_shots(p);
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
                characters: vec![],
                place: String::new(),
                shot: "wide".into(),
                key_scene: false,
                source_pages: vec![],
                general_knowledge: false,
            })
            .collect()
    }

    #[test]
    fn source_projects_keep_page_refs_inside_the_read_range() {
        use crate::pipeline::project::{Audience, SourceDoc};
        let mut p = sample();
        p.source = Some(SourceDoc {
            file_name: "a.pdf".into(),
            size_bytes: 1,
            page_count: 40,
            sha256: String::new(),
            page_from: 11,
            page_to: 20,
            audience: Audience::General,
            focus: String::new(),
            stage: SourceStage::Confirmed,
            chunks: vec![],
            summary: None,
            pause: None,
            credit: String::new(),
        });
        let mut replies = reply(2);
        replies[0].source_pages = vec![14, 3, 14, 12];
        replies[1].general_knowledge = true;
        replies[1].source_pages = vec![13];
        append_chapter(&mut p, 0, replies);
        assert_eq!(p.scenes[0].source_pages, vec![12, 14]);
        assert!(!p.scenes[0].general_knowledge);
        assert!(p.scenes[1].general_knowledge);
        assert!(p.scenes[1].source_pages.is_empty());
        // Topic projects ignore the fields.
        let mut q = sample();
        let mut r = reply(1);
        r[0].source_pages = vec![5];
        append_chapter(&mut q, 0, r);
        assert!(q.scenes[0].source_pages.is_empty());
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
