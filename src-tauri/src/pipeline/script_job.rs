//! Script generation job: outline call, then one call per chapter.
//! `project.json` is saved after the outline and after every chapter, so a crash or
//! cancel resumes at the first chapter that isn't `done`.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use super::ProjectError;
use super::hashing::{normalize, normalize_spoken};
use super::presets;
use super::project::{CodeSpec, DiagramSpec, SceneVisual, Shot, VisualKind};
use super::project::{Outline, OutlineChapter, Project, ProjectStatus, Scene, SourceStage};
use super::store::ProjectStore;
use super::visual_bible;
use crate::gemini::client::ModelChain;
use crate::gemini::retry::RetryHooks;
use crate::gemini::document;
use crate::gemini::script::{
    self, BibleKeys, ChapterContext, ChapterReply, MOODS, OutlineReply, PlanReply, SceneReply,
    ScriptBrief,
};
use crate::gemini::{GeminiClient, GeminiError};
use crate::storage::ApiKey;
use crate::storage::log::LogFile;

/// Seconds of narration per scene (docs/03-pipeline.md: ~6–8 s).
const SECONDS_PER_SCENE: u32 = 7;
const SCENES_PER_CHAPTER: u32 = 5;
const MIN_CHAPTERS: u32 = 2;
const MAX_CHAPTERS: u32 = 10;
/// Keep structured-output responses small enough for Gemini to accept and finish reliably.
/// Long projects can have 10-18 scenes in an outline chapter, so one request per chapter is
/// too large even though the outline itself is valid.
const MAX_SCENES_PER_REQUEST: usize = 5;
/// Extra requests when a batch re-writes scenes that already exist in the project.
const MAX_DUPLICATE_RETRIES: usize = 2;
/// How many earlier chapters are summarised in "already covered" context.
const MAX_COVERED_CHAPTERS: usize = 10;
/// Chapters written at the same time when the script has a plan (one `tokio::join!` slot each).
const PARALLEL_CHAPTERS: usize = 3;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptStage {
    Outline,
    /// Beat sheet + fact bank (before the first chapter).
    Plan,
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

    /// Plan step: one request for the fact bank and every chapter's beats, saved into the
    /// outline. A plan that keeps failing the checks is logged and skipped (chapters are then
    /// written from the outline alone); any other error (busy, network, quota) stops the job.
    async fn write_plan(
        &self,
        project: &Project,
        style_label: &str,
        source: Option<&str>,
    ) -> Result<Option<Project>, ProjectError> {
        let Some(outline) = project.outline.as_ref() else {
            return Ok(None);
        };
        let brief = brief(project, style_label, source);
        let chapters: Vec<(&str, &str, u32)> = outline
            .chapters
            .iter()
            .map(|c| (c.title.as_str(), c.summary.as_str(), c.scene_count))
            .collect();
        let counts: Vec<u32> = chapters.iter().map(|c| c.2).collect();
        let result: Result<(PlanReply, String), GeminiError> = script::request_json(
            self.client,
            self.key,
            self.models,
            script::plan_prompt(&brief, &outline.title, &outline.hook, &chapters),
            script::plan_schema(&counts),
            self.hooks,
        )
        .await;
        let (reply, model) = match result {
            Ok(ok) => ok,
            // Busy, offline, quota, key…: stop rather than write a script without a plan (the
            // Napoleon script, 2026-11-03, repeated its story when the plan hit a 503). The
            // outline is saved, so Continue starts again at the plan.
            Err(e) if !matches!(e, GeminiError::BadResponse(_)) => return Err(e.into()),
            Err(e) => {
                self.log.warn(&format!(
                    "script plan failed, writing chapters without a beat sheet [{}]: {}",
                    e.kind(),
                    e.detail()
                ));
                return Ok(None);
            }
        };
        // Facts sorted into chapter order, citations renumbered (the model often lists a few
        // facts out of order; that is fixed here instead of rejecting the plan).
        let (ordered_facts, ordered_beats) = reply.in_chapter_order();
        let facts: Vec<String> = ordered_facts
            .iter()
            .map(|f| normalize(script::strip_fact_label(f)).trim().to_owned())
            .collect();
        self.log.info(&format!(
            "script plan written by {model}: {} fact(s), {} beat(s)",
            facts.len(),
            counts.iter().sum::<u32>()
        ));
        let updated = self
            .store
            .update(&project.id, |p| {
                if let Some(o) = p.outline.as_mut() {
                    for (index, chapter) in o.chapters.iter_mut().enumerate() {
                        let mut beats = clean_lines(ordered_beats.get(index).map_or(&[], Vec::as_slice));
                        beats.truncate(chapter.scene_count as usize);
                        chapter.beats = beats;
                    }
                    o.facts = facts;
                }
                Ok(())
            })
            .await?
            .0;
        Ok(Some(updated))
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

        // Beat sheet + fact bank before the first chapter, so every scene has its own job.
        // Scripts already part-written without a plan keep going without one.
        if needs_plan(&project) {
            emit(progress(id, ScriptStage::Plan, 0, 0));
            if let Some(project_with_plan) = self.write_plan(&project, style_label, source.as_deref()).await? {
                project = project_with_plan;
            }
        }

        let total = project.outline.as_ref().map_or(0, |o| o.chapters.len());
        // With a plan every chapter owns its facts and beats, so up to PARALLEL_CHAPTERS
        // chapters are written side by side (the client's text semaphore still caps requests).
        // Older scripts without a plan lean on the previous chapter's text: one at a time.
        let wave = if project.outline.as_ref().is_some_and(|o| !o.facts.is_empty()) {
            PARALLEL_CHAPTERS
        } else {
            1
        };
        let mut attempted: HashSet<usize> = HashSet::new();
        while let Some(outline) = project.outline.as_ref() {
            let pending: Vec<usize> = outline
                .chapters
                .iter()
                .enumerate()
                .filter(|(i, c)| !c.done && !attempted.contains(i))
                .map(|(i, _)| i)
                .take(wave)
                .collect();
            if pending.is_empty() {
                break;
            }
            attempted.extend(pending.iter().copied());
            emit(progress(id, ScriptStage::Chapter, chapter_counts(&project).0, total));
            let (snapshot, source) = (&project, source.as_deref());
            let mut tasks = pending.iter().map(|&index| async move {
                let (replies, models) = self
                    .write_chapter(snapshot, index, style_label, source)
                    .await?;
                self.save_chapter(id, index, total, replies, &models, emit).await
            });
            let (a, b, c) = tokio::join!(
                run_some(tasks.next()),
                run_some(tasks.next()),
                run_some(tasks.next())
            );
            // Finished chapters are already saved; the first error stops the job (Continue
            // picks up the chapters that are not done).
            for result in [a, b, c].into_iter().flatten() {
                result?;
            }
            project = self.store.get(id).await?;
        }
        Ok(())
    }

    /// Writes one chapter in batches of `MAX_SCENES_PER_REQUEST`, using `project` (the
    /// chapters saved before this wave) as context. Returns the scenes and the models used.
    async fn write_chapter(
        &self,
        project: &Project,
        index: usize,
        style_label: &str,
        source: Option<&str>,
    ) -> Result<(Vec<SceneReply>, Vec<String>), ProjectError> {
        let Some(outline) = project.outline.as_ref() else {
            return Ok((Vec::new(), Vec::new()));
        };
        let Some(chapter) = outline.chapters.get(index) else {
            return Ok((Vec::new(), Vec::new()));
        };
        let previous = project
            .scenes
            .iter()
            .rev()
            .find(|s| s.chapter < index as u32)
            .map(|s| s.narration.clone());
        let brief = brief(project, style_label, source);
        let bible = project.visual_bible.as_ref().map_or_else(BibleKeys::default, |b| BibleKeys {
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
        });
        let chapters: Vec<_> = outline
            .chapters
            .iter()
            .map(|c| (c.title.as_str(), c.summary.as_str()))
            .collect();
        let chapter_scene_count = chapter.scene_count as usize;
        let mut replies: Vec<SceneReply> = Vec::with_capacity(chapter_scene_count);
        // Every narration already written, so later chapters cannot repeat a scene
        // ("the same line in another section").
        let mut seen: HashSet<String> = project
            .scenes
            .iter()
            .map(|s| scene_key(&s.narration))
            .collect();
        let mut models = Vec::new();
        // Names as the outline and earlier scenes spell them ("napoleon" → "Napoleon").
        let mut names = NameCase::default();
        names.learn(&outline.title);
        names.learn(&outline.hook);
        for c in &outline.chapters {
            names.learn(&c.title);
            names.learn(&c.summary);
        }
        for s in &project.scenes {
            names.learn(&s.narration);
        }
        // The facts the plan gives this chapter; the writer must leave the others alone.
        let chapter_facts = script::cited_facts(&chapter.beats);
        for start in (0..chapter_scene_count).step_by(MAX_SCENES_PER_REQUEST) {
            let count = (chapter_scene_count - start).min(MAX_SCENES_PER_REQUEST);
            let mut shot_history: Vec<String> = project
                .scenes
                .iter()
                .filter(|s| s.chapter < index as u32)
                .rev()
                .take(2)
                .map(|s| s.shot.as_str().to_owned())
                .collect();
            shot_history.reverse();
            shot_history.extend(replies.iter().map(|s| s.shot.clone()));
            let shot_start = shot_history.len().saturating_sub(2);
            let previous_shots: Vec<String> = shot_history.into_iter().skip(shot_start).collect();
            let previous_shot_refs = previous_shots.iter().map(String::as_str).collect();
            let previous_narration = replies
                .last()
                .map(|s: &SceneReply| s.narration.as_str())
                .or(previous.as_deref());
            let ctx = ChapterContext {
                title: &outline.title,
                hook: &outline.hook,
                chapters: chapters.clone(),
                index,
                scene_count: count,
                segment_start: start,
                chapter_total_scenes: chapter_scene_count,
                previous_narration,
                previous_shots: previous_shot_refs,
                bible: BibleKeys {
                    characters: bible.characters.clone(),
                    places: bible.places.clone(),
                },
                beats: chapter
                    .beats
                    .get(start..(start + count).min(chapter.beats.len()))
                    .unwrap_or(&[]),
                facts: &outline.facts,
                chapter_facts: &chapter_facts,
            };
            let mut prompt = script::chapter_prompt(&brief, &ctx);
            let mut covered = String::from(
                "\nALREADY COVERED IN THIS VIDEO — continuation context, not new material:\n",
            );
            let mut prior_chapters: Vec<u32> = project
                .scenes
                .iter()
                .filter(|s| s.chapter < index as u32)
                .map(|s| s.chapter)
                .collect();
            prior_chapters.sort_unstable();
            prior_chapters.dedup();
            // Only the latest chapters: the `seen` set already blocks exact repeats, and
            // the prompt would otherwise grow with every chapter of a 20-minute video.
            let skip = prior_chapters.len().saturating_sub(MAX_COVERED_CHAPTERS);
            for prior_chapter in prior_chapters.into_iter().skip(skip) {
                let chapter_text = project
                    .scenes
                    .iter()
                    .filter(|s| s.chapter == prior_chapter)
                    .map(|s| s.narration.trim())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                if !chapter_text.is_empty() {
                    covered.push_str(&format!(
                        "Earlier chapter {}: {}\n",
                        prior_chapter + 1,
                        chapter_text.chars().take(700).collect::<String>()
                    ));
                }
            }
            for (offset, scene) in replies.iter().enumerate() {
                covered.push_str(&format!(
                    "Already written in this chapter, scene {}: {}\n",
                    offset + 1,
                    scene.narration.trim()
                ));
            }
            let mut recent_visuals: Vec<String> = project
                .scenes
                .iter()
                .filter(|s| s.chapter < index as u32)
                .map(|s| s.image_prompt.trim().to_owned())
                .filter(|s| !s.is_empty())
                .collect();
            recent_visuals.extend(
                replies
                    .iter()
                    .map(|s| s.image_prompt.trim().to_owned())
                    .filter(|s| !s.is_empty()),
            );
            let visual_start = recent_visuals.len().saturating_sub(8);
            if visual_start < recent_visuals.len() {
                covered.push_str("RECENT VISUALS ALREADY USED (avoid repeating these setups):\n");
                for (offset, visual) in recent_visuals.iter().skip(visual_start).enumerate() {
                    covered.push_str(&format!(
                        "{}. {}\n",
                        visual_start + offset + 1,
                        visual.chars().take(180).collect::<String>()
                    ));
                }
            }
            covered.push_str(
                "Give each new scene a distinct purpose and add a new fact, example, consequence, or step. Do not repeat a hook, definition, claim, example, transition, sentence, or sentence opening already covered. Continue the explanation from the next logical point. For every image_prompt, name a concrete topic-specific object and action; for technology scenes show real computing tools or a developer at work, not a generic classroom, group or landscape. Choose a different setting, composition and visible action from recent visuals unless the narration requires continuity. Use natural varied Khmer phrasing; do not force awkward synonyms.\n",
            );
            prompt.push_str(&covered);
            if chapter_scene_count > MAX_SCENES_PER_REQUEST {
                prompt.push_str(&format!(
                    "\nThis is segment {} of {} for this chapter, covering chapter scenes {}–{}. \
                     This segment continues the same chapter. Continue chronologically from the previous scene; \
                     do not restart the hook or repeat earlier scenes. Advance the story to the next event.\n",
                    start / MAX_SCENES_PER_REQUEST + 1,
                    chapter_scene_count.div_ceil(MAX_SCENES_PER_REQUEST),
                    start + 1,
                    start + count,
                ));
            }
            let mut fresh: Vec<SceneReply> = Vec::new();
            let mut attempts = 0;
            loop {
                let (reply, model): (ChapterReply, String) = script::request_json(
                    self.client,
                    self.key,
                    self.models,
                    prompt.clone(),
                    script::chapter_schema(
                        count.saturating_sub(fresh.len()).max(1),
                        &ctx.bible,
                        source.is_some(),
                        brief.real_world,
                    ),
                    self.hooks,
                )
                .await?;
                models.push(model);
                let mut scenes = reply.scenes;
                for s in &scenes {
                    names.learn(&s.narration);
                }
                for s in &mut scenes {
                    s.narration = names.fix(&s.narration);
                    s.spoken_text = names.fix(&s.spoken_text);
                }
                let (mut unique, dropped) = take_unique_scenes(scenes, &mut seen);
                fresh.append(&mut unique);
                if dropped == 0 {
                    break;
                }
                self.log.warn(&format!(
                    "chapter {} segment {}-{} repeated {dropped} scene(s) already written elsewhere",
                    index + 1,
                    start + 1,
                    start + count
                ));
                if fresh.len() >= count || attempts >= MAX_DUPLICATE_RETRIES {
                    if fresh.len() < count {
                        self.log.warn(&format!(
                            "chapter {} segment {}-{} kept {} of {count} scene(s); skipped the repeated ones",
                            index + 1,
                            start + 1,
                            start + count,
                            fresh.len()
                        ));
                    }
                    break;
                }
                attempts += 1;
                prompt.push_str(&format!(
                    "\nRETRY WARNING: your previous answer repeated {dropped} scene(s) that already exist in this video. \
                     Do NOT restate them. Write completely NEW scenes describing the NEXT steps of the story. \
                     Return exactly {} new scene(s).\n",
                    count - fresh.len()
                ));
                for scene in &fresh {
                    prompt.push_str(&format!(
                        "Just written in this batch (do not repeat): {}\n",
                        scene.narration.trim()
                    ));
                }
            }
            replies.extend(fresh);
        }
        Ok((replies, models))
    }

    /// Saves a written chapter (dropping scenes another chapter of the same wave already
    /// said) and reports progress.
    async fn save_chapter(
        &self,
        id: &str,
        index: usize,
        total: usize,
        replies: Vec<SceneReply>,
        models: &[String],
        emit: &impl Fn(ScriptProgress),
    ) -> Result<(), ProjectError> {
        let (project, (kept, dropped)) = self
            .store
            .update(id, |p| {
                let mut seen: HashSet<String> = p
                    .scenes
                    .iter()
                    .filter(|s| s.chapter != index as u32)
                    .map(|s| scene_key(&s.narration))
                    .collect();
                let (unique, dropped) = take_unique_scenes(replies, &mut seen);
                let kept = unique.len();
                append_chapter(p, index, unique);
                Ok((kept, dropped))
            })
            .await?;
        if dropped > 0 {
            self.log.warn(&format!(
                "chapter {} repeated {dropped} scene(s) of a chapter written at the same time; skipped them",
                index + 1
            ));
        }
        self.log.info(&format!(
            "chapter {}/{total} ({kept} scenes) written in {} request(s) by {}",
            index + 1,
            models.len(),
            models.join(", ")
        ));
        emit(progress(id, ScriptStage::Chapter, chapter_counts(&project).0, total));
        Ok(())
    }
}

/// Awaits `task` if there is one (fills the fixed `tokio::join!` slots of a wave).
async fn run_some<F: std::future::Future>(task: Option<F>) -> Option<F::Output> {
    match task {
        Some(task) => Some(task.await),
        None => None,
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
        real_world: super::presets::is_real_world(&p.style.preset),
        tone: p.tone,
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

/// Narration normalized for duplicate detection (NFC, collapsed spaces, case-folded).
fn scene_key(text: &str) -> String {
    normalize(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Keeps only scenes whose narration has not been written yet; returns them plus how many
/// repeats were dropped. A batch can repeat an earlier chapter, so `seen` spans the project.
fn take_unique_scenes(
    scenes: Vec<SceneReply>,
    seen: &mut HashSet<String>,
) -> (Vec<SceneReply>, usize) {
    let mut dropped = 0;
    let mut out = Vec::with_capacity(scenes.len());
    for mut scene in scenes {
        scene.narration = strip_ellipsis(&scene.narration);
        let key = scene_key(&scene.narration);
        if key.is_empty() || seen.iter().any(|old| near_duplicate(old, &key)) || !seen.insert(key) {
            dropped += 1;
        } else {
            out.push(scene);
        }
    }
    (out, dropped)
}

/// True before the first chapter of a script that has no beat sheet yet.
fn needs_plan(p: &Project) -> bool {
    p.outline.as_ref().is_some_and(|o| {
        p.scenes.is_empty()
            && o.facts.is_empty()
            && o.chapters.iter().all(|c| !c.done && c.beats.is_empty())
    })
}

fn clean_lines(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .map(|l| normalize(l).trim().to_owned())
        .filter(|l| !l.is_empty())
        .collect()
}

/// Capitalised Latin names seen in the outline and scenes, to fix the same name written in
/// lower case ("napoleon") in Khmer narration. Only words that never appear capitalised
/// elsewhere stay as they are (e.g. "km").
#[derive(Default)]
struct NameCase {
    names: HashMap<String, String>,
}

impl NameCase {
    fn learn(&mut self, text: &str) {
        for word in latin_words(text) {
            if word.len() >= 3 && word.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                self.names.entry(word.to_ascii_lowercase()).or_insert_with(|| word.to_owned());
            }
        }
    }

    fn fix(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut word = String::new();
        let flush = |word: &mut String, out: &mut String| {
            let all_lower = !word.is_empty() && word.chars().all(|c| c.is_ascii_lowercase());
            match self.names.get(word.as_str()).filter(|_| all_lower) {
                Some(name) => out.push_str(name),
                None => out.push_str(word),
            }
            word.clear();
        };
        for c in text.chars() {
            if c.is_ascii_alphabetic() {
                word.push(c);
            } else {
                flush(&mut word, &mut out);
                out.push(c);
            }
        }
        flush(&mut word, &mut out);
        out
    }
}

fn latin_words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !c.is_ascii_alphabetic()).filter(|w| !w.is_empty())
}

/// Subtitles never carry the TTS pause marks the model likes to copy into `narration`.
fn strip_ellipsis(text: &str) -> String {
    let cleaned = text.replace("...", " ").replace('\u{2026}', " ");
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The same story retold in other words: most character trigrams are shared. Short lines
/// are only compared exactly (by `seen`).
fn near_duplicate(a: &str, b: &str) -> bool {
    fn trigrams(s: &str) -> HashSet<(char, char, char)> {
        let c: Vec<char> = s.chars().filter(|c| !c.is_whitespace()).collect();
        c.windows(3).map(|w| (w[0], w[1], w[2])).collect()
    }
    let (ta, tb) = (trigrams(a), trigrams(b));
    if ta.len() < 20 || tb.len() < 20 {
        return false;
    }
    let shared = ta.intersection(&tb).count() as f32;
    shared / ta.union(&tb).count() as f32 >= 0.6
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
                beats: vec![],
            })
            .collect(),
        facts: vec![],
    }
}

/// A real-world scene's visual from the model's reply. A diagram whose Mermaid can't be used
/// falls back to a photo (or an AI image without keywords); unknown code languages become text.
fn visual_from_reply(r: &SceneReply) -> SceneVisual {
    let mut keywords: Vec<String> = Vec::new();
    for k in &r.keywords {
        let k = normalize(k).chars().take(60).collect::<String>();
        if !k.is_empty() && !keywords.iter().any(|x| x.eq_ignore_ascii_case(&k)) && keywords.len() < 4 {
            keywords.push(k);
        }
    }
    let photo_or_ai = if keywords.is_empty() { VisualKind::AiImage } else { VisualKind::RealPhoto };
    let mut v = SceneVisual { keywords, ..SceneVisual::default() };
    v.kind = match r.visual_type.as_str() {
        "real_photo" => photo_or_ai,
        "stock_video" if !v.keywords.is_empty() => VisualKind::StockVideo,
        "diagram" => match script::mermaid_template(&r.mermaid) {
            Some(template) => {
                v.diagram = Some(DiagramSpec { template: template.into(), mermaid: r.mermaid.trim().to_owned() });
                VisualKind::Diagram
            }
            None => photo_or_ai,
        },
        "code" if !r.code.trim().is_empty() => {
            let language = if script::CODE_LANGUAGES.contains(&r.code_language.as_str()) { r.code_language.clone() } else { "text".into() };
            v.code = Some(CodeSpec { language, text: r.code.trim_end().to_owned(), terminal: r.terminal, type_out: true });
            VisualKind::Code
        }
        "ai_image" => VisualKind::AiImage,
        _ => photo_or_ai,
    };
    v
}

fn append_chapter(p: &mut Project, index: usize, replies: Vec<SceneReply>) {
    let chapter = index as u32;
    let real_world = super::presets::is_real_world(&p.style.preset);
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
        if real_world {
            scene.visual = visual_from_reply(&r);
        }
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
                visual_type: String::new(),
                keywords: vec![],
                mermaid: String::new(),
                code_language: String::new(),
                code: String::new(),
                terminal: false,
            })
            .collect()
    }

    #[test]
    fn real_world_replies_become_scene_visuals() {
        let mut r = reply(1).remove(0);
        r.visual_type = "diagram".into();
        r.keywords = vec![" undersea cable ".into(), "undersea cable".into(), "".into()];
        r.mermaid = "flowchart LR\n A[\"Phnom Penh\"] --> B[\"Singapore\"]".into();
        let v = visual_from_reply(&r);
        assert_eq!(v.kind, VisualKind::Diagram);
        assert_eq!(v.keywords, vec!["undersea cable"]);
        assert_eq!(v.diagram.as_ref().map(|d| d.template.as_str()), Some("flow"));

        r.mermaid = "not mermaid".into();
        assert_eq!(visual_from_reply(&r).kind, VisualKind::RealPhoto, "broken diagram falls back to a photo");
        r.keywords.clear();
        assert_eq!(visual_from_reply(&r).kind, VisualKind::AiImage);

        r.visual_type = "code".into();
        r.code = "$ traceroute example.com".into();
        r.code_language = "klingon".into();
        let c = visual_from_reply(&r);
        let code = c.code.unwrap();
        assert_eq!((c.kind, code.language.as_str(), code.terminal, code.type_out), (VisualKind::Code, "text", false, true));

        r.visual_type = "stock_video".into();
        r.keywords = vec!["cable ship".into()];
        assert_eq!(visual_from_reply(&r).kind, VisualKind::StockVideo);
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
    fn ellipses_leave_narration_and_reworded_lines_count_as_repeats() {
        assert_eq!(strip_ellipsis("កាលពីពេលនោះ... គ្មាននរណាដឹង…។"), "កាលពីពេលនោះ គ្មាននរណាដឹង ។");
        let a = "រោងចក្រ TSMC បានប្រើប្រាស់ទឹកបរិសុទ្ធជាង 150,000 តោនក្នុងមួយថ្ងៃ";
        let b = "រោងចក្រ TSMC តែមួយ បានប្រើប្រាស់ទឹកបរិសុទ្ធជាង 150,000 តោនក្នុងមួយថ្ងៃ";
        assert!(near_duplicate(&scene_key(a), &scene_key(b)));
        let other = "ក្នុងឆ្នាំ 1971 Intel បានបញ្ចេញ microprocessor ដំបូងគេឈ្មោះ Intel 4004";
        assert!(!near_duplicate(&scene_key(a), &scene_key(other)));
    }

    #[test]
    fn names_get_their_capital_letter_back() {
        let mut names = NameCase::default();
        names.learn("មេទ័ព Napoleon និង Tsar Alexander I នៅ Moscow");
        assert_eq!(
            names.fix("napoleon រង់ចាំ tsar នៅ moscow 10 km, stubborn"),
            "Napoleon រង់ចាំ Tsar នៅ Moscow 10 km, stubborn"
        );
        // Mixed case is left alone (iPhone), and so are words never seen capitalised.
        assert_eq!(names.fix("iPhone napoleons"), "iPhone napoleons");
    }

    #[test]
    fn take_unique_scenes_drops_repeats_across_the_whole_project() {
        let mut seen: HashSet<String> = HashSet::new();
        let (kept, dropped) = take_unique_scenes(reply(2), &mut seen);
        assert_eq!((kept.len(), dropped), (2, 0));

        let mut repeat = reply(2);
        repeat[0].narration = "  n0 ".into();
        repeat[1].narration = "n1".into();
        let (kept, dropped) = take_unique_scenes(repeat, &mut seen);
        assert_eq!(
            (kept.len(), dropped),
            (0, 2),
            "whitespace variants still count as repeats"
        );
    }

    #[test]
    fn take_unique_scenes_drops_repeats_inside_one_batch() {
        let mut seen: HashSet<String> = HashSet::new();
        let mut scenes = reply(1);
        scenes.push(scenes[0].clone());
        let (kept, dropped) = take_unique_scenes(scenes, &mut seen);
        assert_eq!((kept.len(), dropped), (1, 1));
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
                aspect: crate::pipeline::project::Aspect::Wide,
                style_preset: "storybook_watercolor".into(),
                tone: Default::default(),
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
