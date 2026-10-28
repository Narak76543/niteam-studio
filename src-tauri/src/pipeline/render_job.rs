//! Render job (03 Step 6): one clip per scene (resumable, cached by input hash), then concat
//! into one MP4. Only one FFmpeg process at a time, below-normal priority, cancellable.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use super::ProjectError;
use super::hashing::sha;
use super::project::{AssetStatus, Project, ProjectStatus, Scene};
use super::store::ProjectStore;
use super::subs;
use super::drawn;
use super::visual_render::{self, PhotoPrep, ScenePlan};
use crate::render::RenderError;
use crate::render::clip::{ClipSpec, Frame, Input, Motion, TAIL_PAD_MS, clip_args};
use crate::stock::assets::AssetRegistry;
use crate::render::concat::{concat_args, concat_list};
use crate::render::encoders::Encoder;
use crate::render::ffmpeg::Ffmpeg;
use crate::storage::log::LogFile;
use crate::storage::{AppPaths, atomic};
use crate::tts::WordTiming;

/// Bump when the clip filter graph changes, so existing clips are re-rendered.
/// 2: subtitle track (one concat input + one overlay).
const CLIP_VERSION: &str = "2";
/// Share of the progress bar for the final concat (clips take the rest).
const CONCAT_WEIGHT: f64 = 0.1;
/// Voice kept after the last spoken word. Edge TTS MP3s end with ~0.8 s of silence; without
/// trimming, scenes are separated by ~1.3 s of dead air.
const TRAIL_KEEP_MS: u64 = 150;

/// Voice length used for the clip: cut ~0.15 s after the last word when timings are known
/// (Edge `words.json`), else the whole file (Gemini TTS).
pub fn trimmed_voice_ms(duration_ms: u64, speech_end_ms: Option<u64>) -> u64 {
    match speech_end_ms {
        Some(end) if end > 0 && end + TRAIL_KEEP_MS < duration_ms => end + TRAIL_KEEP_MS,
        _ => duration_ms,
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RenderStage {
    Clips,
    Concat,
    Done,
    Failed,
    Cancelled,
}

/// Payload of `render-progress`.
#[derive(Debug, Clone, Serialize)]
pub struct RenderProgress {
    pub project_id: String,
    pub stage: RenderStage,
    /// 1-based scene being rendered (0 during concat).
    pub scene_index: usize,
    pub total: usize,
    /// 0–1 of this run's work (cached clips excluded).
    pub percent: f64,
    pub eta_seconds: Option<u64>,
    pub output_path: Option<String>,
    pub error: Option<String>,
    pub error_kind: Option<String>,
}

pub trait RenderEvents: Send + Sync {
    fn progress(&self, progress: RenderProgress);
}

struct Emitter<'a> {
    project_id: &'a str,
    events: &'a dyn RenderEvents,
    total: usize,
}

impl Emitter<'_> {
    fn running(&self, stage: RenderStage, scene_index: usize, meter: &Meter, fraction: f64) {
        self.send(stage, scene_index, fraction, meter.eta(fraction), None, None);
    }

    fn send(
        &self,
        stage: RenderStage,
        scene_index: usize,
        percent: f64,
        eta_seconds: Option<u64>,
        output_path: Option<String>,
        error: Option<&RenderError>,
    ) {
        self.events.progress(RenderProgress {
            project_id: self.project_id.to_owned(),
            stage,
            scene_index,
            total: self.total,
            percent,
            eta_seconds,
            output_path,
            error: error.map(ToString::to_string),
            error_kind: error.map(|e| e.kind().to_owned()),
        });
    }
}

/// What blocks a render, for a clear message before anything starts.
pub fn check_ready(p: &Project, dir: &Path) -> Result<(), ProjectError> {
    if p.scenes.is_empty() {
        return Err(ProjectError::Invalid("This project has no scenes.".into()));
    }
    let missing = |f: &dyn Fn(&super::project::Scene) -> bool| p.scenes.iter().filter(|s| !f(s)).count();
    let file = |rel: &Option<String>| rel.as_ref().is_some_and(|r| dir.join(r).is_file());
    let voices = missing(&|s| s.voice.status == AssetStatus::Done && s.voice.duration_ms.is_some() && file(&s.voice.path));
    let real_world = super::presets::is_real_world(&p.style.preset);
    let registry = if real_world { registry(dir) } else { AssetRegistry::default() };
    let images = p
        .scenes
        .iter()
        .enumerate()
        .filter(|(i, s)| match real_world.then(|| visual_render::plan(p, s, *i, dir, &registry)) {
            Some(ScenePlan::Ready { .. }) => false,
            Some(ScenePlan::Missing(_)) => true,
            Some(ScenePlan::AiImage) | None => !(s.image.status == AssetStatus::Done && file(&s.image.path)),
        })
        .count();
    let subtitles = missing(&|s| subs::is_fresh(p, s, dir));
    if voices + images > 0 {
        return Err(ProjectError::Invalid(format!(
            "Not ready to render: {voices} voice(s) and {images} image(s) are missing or outdated."
        )));
    }
    if subtitles > 0 {
        return Err(ProjectError::Invalid(format!(
            "Not ready to render: subtitles for {subtitles} scene(s) are missing or outdated."
        )));
    }
    Ok(())
}

/// One scene's clip: what to render and whether the existing file can be reused.
#[derive(Debug, Clone)]
pub struct ClipTask {
    pub scene_id: String,
    pub spec: ClipSpec,
    pub hash: String,
    pub fresh: bool,
    /// Real-world photo to prepare before the clip (`visual_render::ensure`).
    pub prep: Option<PhotoPrep>,
    /// Typed-out code: (track path, `ffconcat` text) written before the clip.
    pub track: Option<(String, String)>,
}

fn registry(dir: &Path) -> AssetRegistry {
    AssetRegistry::load(&dir.join("assets")).unwrap_or_default()
}

/// Plans every clip. Call after `check_ready`.
pub fn plan_clips(p: &Project, dir: &Path, encoder: Encoder) -> Vec<ClipTask> {
    let frame = Frame::for_project_settings(p);
    let real_world = super::presets::is_real_world(&p.style.preset);
    let registry = if real_world { registry(dir) } else { AssetRegistry::default() };
    p.scenes
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let voice_ms = trimmed_voice_ms(s.voice.duration_ms.unwrap_or(0), speech_end(s, dir));
            let (input, prep, visual_hash) = match real_world.then(|| visual_render::plan(p, s, i, dir, &registry)) {
                Some(ScenePlan::Ready { input, prep, hash }) => (input, prep, hash),
                _ => {
                    let path = s.image.path.clone().unwrap_or_default();
                    (Input::Image { path, motion: Motion::Pan(visual_render::ai_pan(i)) }, None, String::new())
                }
            };
            let (image, motion) = match &input {
                Input::Image { path, motion } => (path.clone(), motion.as_str()),
                Input::Video { path, .. } => (path.clone(), "video"),
                Input::Frames { track } => (track.clone(), "frames"),
            };
            let mut parts = vec![
                "clip",
                CLIP_VERSION,
                s.image.prompt_hash.as_deref().unwrap_or(""),
                &image,
                s.voice.text_hash.as_deref().unwrap_or(""),
                s.voice.path.as_deref().unwrap_or(""),
            ];
            let voice_len = voice_ms.to_string();
            parts.extend([&voice_len[..], s.subs.hash.as_deref().unwrap_or(""), encoder.name(), &p.render.resolution, p.aspect.as_str(), motion]);
            // AI-image scenes keep their old hash (no needless re-render of existing projects).
            if !visual_hash.is_empty() {
                parts.push(&visual_hash);
            }
            let hash = sha(&parts);
            let final_rel = clip_path(&s.id);
            let fresh = s.clip.status == AssetStatus::Done
                && s.clip.hash.as_deref() == Some(hash.as_str())
                && dir.join(&final_rel).is_file();
            let track = match &input {
                Input::Frames { track } => Some((track.clone(), drawn::code_track(&s.visual.frames, voice_ms, voice_ms + TAIL_PAD_MS))),
                _ => None,
            };
            ClipTask {
                scene_id: s.id.clone(),
                hash,
                fresh,
                prep,
                track,
                spec: ClipSpec {
                    input,
                    audio: s.voice.path.clone().unwrap_or_default(),
                    voice_ms,
                    subs_track: (!s.subs.lines.is_empty()).then(|| subs::track_path(&s.id)),
                    frame,
                    encoder_args: encoder.args(),
                    output: format!("scenes/{}/clip.tmp.mkv", s.id),
                },
            }
        })
        .collect()
}

fn words(s: &Scene, dir: &Path) -> Option<Vec<WordTiming>> {
    s.voice
        .words_path
        .as_ref()
        .and_then(|w| atomic::read_json::<Vec<WordTiming>>(&dir.join(w)).ok().flatten())
}

fn speech_end(s: &Scene, dir: &Path) -> Option<u64> {
    words(s, dir).and_then(|w| w.last().map(|w| w.end_ms))
}

/// Placeholder length in the editor timeline for a scene without a voice yet.
const PLACEHOLDER_MS: u64 = 4000;

/// One scene in the editor timeline: exactly the times and pan the render will use.
#[derive(Debug, Clone, Serialize)]
pub struct TimelineScene {
    pub scene_id: String,
    pub order: u32,
    pub start_ms: u64,
    /// Clip length (used voice + 0.25 s pause).
    pub clip_ms: u64,
    /// Voice length used (trailing silence trimmed).
    pub voice_ms: u64,
    /// Full voice file length (subtitle timings run over this).
    pub duration_ms: u64,
    /// "lr" | "rl" | "tb" | "bt" (AI images), "zoom_in" | "zoom_out" | "still" (real-world
    /// pictures; clips preview as a still of their poster).
    pub pan: &'static str,
    /// Largest zoom of "zoom_in" / "zoom_out" (1 otherwise).
    pub zoom: f32,
    pub voice_ready: bool,
    pub image_ready: bool,
    pub narration: String,
    pub words: Option<Vec<WordTiming>>,
}

/// The editor preview's timeline (same durations and pans as `plan_clips`).
pub fn timeline(p: &Project, dir: &Path) -> Vec<TimelineScene> {
    let file = |rel: &Option<String>| rel.as_ref().is_some_and(|r| dir.join(r).is_file());
    let real_world = super::presets::is_real_world(&p.style.preset);
    let registry = if real_world { registry(dir) } else { AssetRegistry::default() };
    let mut start = 0;
    p.scenes
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let visual = real_world.then(|| visual_render::preview(p, s, i, dir, &registry)).flatten();
            let ai_ready = s.image.status == AssetStatus::Done && file(&s.image.path);
            let (pan, zoom, image_ready) = match &visual {
                Some((_, _, motion)) => (motion.as_str(), motion.zoom(), true),
                None if real_world && !s.needs_ai_image() => ("still", 1.0, false),
                None => (visual_render::ai_pan(i).as_str(), 1.0, ai_ready),
            };
            let voice_ready = s.voice.status == AssetStatus::Done && file(&s.voice.path);
            let duration_ms = if voice_ready { s.voice.duration_ms.unwrap_or(0) } else { 0 };
            let voice_ms = if voice_ready { trimmed_voice_ms(duration_ms, speech_end(s, dir)) } else { 0 };
            let clip_ms = if voice_ready { voice_ms + TAIL_PAD_MS } else { PLACEHOLDER_MS };
            let item = TimelineScene {
                scene_id: s.id.clone(),
                order: s.order,
                start_ms: start,
                clip_ms,
                voice_ms,
                duration_ms,
                pan,
                zoom,
                voice_ready,
                image_ready,
                narration: s.narration.clone(),
                words: if voice_ready { words(s, dir) } else { None },
            };
            start += clip_ms;
            item
        })
        .collect()
}

fn clip_path(scene_id: &str) -> String {
    format!("scenes/{scene_id}/clip.mkv")
}

/// `<output_dir>/<title>.mp4` when the user chose a folder, else `<project>/output/final.mp4`.
pub fn output_path(p: &Project, project_dir: &Path, output_dir: Option<&str>) -> PathBuf {
    match output_dir.filter(|d| !d.trim().is_empty()) {
        Some(d) => Path::new(d).join(format!("{}.mp4", safe_file_name(&p.title))),
        None => project_dir.join("output").join("final.mp4"),
    }
}

fn safe_file_name(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| if c.is_control() || "\\/:*?\"<>|".contains(c) { '_' } else { c })
        .collect::<String>()
        .trim()
        .trim_end_matches('.')
        .chars()
        .take(80)
        .collect();
    if cleaned.trim().is_empty() { "video".into() } else { cleaned }
}

/// Progress over this run's work: durations of clips to render + the concat.
struct Meter {
    started: Instant,
    total_ms: f64,
    done_ms: f64,
}

impl Meter {
    fn fraction(&self, current_ms: f64) -> f64 {
        if self.total_ms <= 0.0 {
            return 1.0;
        }
        ((self.done_ms + current_ms) / self.total_ms).clamp(0.0, 1.0)
    }

    fn eta(&self, fraction: f64) -> Option<u64> {
        let elapsed = self.started.elapsed().as_secs_f64();
        (fraction > 0.02 && elapsed > 2.0).then(|| (elapsed * (1.0 - fraction) / fraction).round() as u64)
    }
}

pub struct RenderJob {
    pub store: Arc<ProjectStore>,
    pub paths: AppPaths,
    pub ffmpeg: Ffmpeg,
    pub encoder: Encoder,
    pub output_dir: Option<String>,
    pub log: LogFile,
    /// One FFmpeg process at a time across the app (low-spec laptops).
    pub ffmpeg_lock: Arc<Mutex<()>>,
}

impl RenderJob {
    pub async fn run(&self, project_id: &str, cancel: CancellationToken, events: Arc<dyn RenderEvents>) {
        let started = Instant::now();
        let total = self.store.get(project_id).await.map(|p| p.scenes.len()).unwrap_or(0);
        let emit = Emitter { project_id, events: events.as_ref(), total };
        match self.render(project_id, &cancel, &emit).await {
            Ok(output) => {
                self.log.info(&format!(
                    "render done in {:.0} s -> {}",
                    started.elapsed().as_secs_f64(),
                    output.display()
                ));
                emit.send(RenderStage::Done, 0, 1.0, None, Some(output.display().to_string()), None);
            }
            Err(e) => {
                let cancelled = matches!(e, RenderError::Cancelled);
                if cancelled {
                    self.log.info("render cancelled");
                } else {
                    self.log.error(&format!("render failed: {e}"));
                }
                let status = if cancelled { "cancelled" } else { "failed" };
                let _ = self
                    .store
                    .update(project_id, |p| {
                        for s in &mut p.scenes {
                            if s.clip.status == AssetStatus::Running {
                                s.clip.status = AssetStatus::Pending;
                            }
                        }
                        if p.status == ProjectStatus::Rendering {
                            p.status = ProjectStatus::Review;
                        }
                        p.render.status = status.into();
                        Ok(())
                    })
                    .await;
                let stage = if cancelled { RenderStage::Cancelled } else { RenderStage::Failed };
                emit.send(stage, 0, 0.0, None, None, Some(&e));
            }
        }
    }

    async fn render(
        &self,
        project_id: &str,
        cancel: &CancellationToken,
        emit: &Emitter<'_>,
    ) -> Result<PathBuf, RenderError> {
        let dir = self.paths.project_dir(project_id);
        let io = |e: ProjectError| RenderError::Io(e.to_string());
        let project = self.store.get(project_id).await.map_err(io)?;
        check_ready(&project, &dir).map_err(io)?;
        let tasks = plan_clips(&project, &dir, self.encoder);
        let output = output_path(&project, &dir, self.output_dir.as_deref());
        let total = tasks.len();

        let _guard = tokio::select! {
            () = cancel.cancelled() => return Err(RenderError::Cancelled),
            g = self.ffmpeg_lock.lock() => g,
        };
        let encoder = self.encoder.name();
        self.store
            .update(project_id, |p| {
                p.status = ProjectStatus::Rendering;
                p.render.status = "running".into();
                p.render.encoder = Some(encoder.into());
                Ok(())
            })
            .await
            .map_err(io)?;

        let todo_ms: u64 = tasks.iter().filter(|t| !t.fresh).map(|t| t.spec.duration_ms()).sum();
        let all_ms: u64 = tasks.iter().map(|t| t.spec.duration_ms()).sum();
        let concat_ms = all_ms as f64 * CONCAT_WEIGHT;
        let mut meter = Meter {
            started: Instant::now(),
            total_ms: todo_ms as f64 + concat_ms,
            done_ms: 0.0,
        };
        self.log.info(&format!(
            "render started: {total} scene(s), {} to render, encoder {encoder}, {}",
            tasks.iter().filter(|t| !t.fresh).count(),
            project.render.resolution
        ));

        for (i, task) in tasks.iter().enumerate() {
            if task.fresh {
                continue;
            }
            emit.running(RenderStage::Clips, i + 1, &meter, meter.fraction(0.0));
            self.set_clip(project_id, &task.scene_id, AssetStatus::Running, None).await;
            if let Some((track, text)) = &task.track {
                std::fs::write(dir.join(track), text)?;
            }
            if let Some(prep) = task.prep.clone() {
                let d = dir.clone();
                tokio::task::spawn_blocking(move || visual_render::ensure(&d, &prep))
                    .await
                    .map_err(|e| RenderError::Io(e.to_string()))?
                    .map_err(RenderError::Io)?;
            }
            let clip_started = Instant::now();
            let tmp = dir.join(&task.spec.output);
            let result = self
                .ffmpeg
                .run(&clip_args(&task.spec), &dir, cancel, |ms| {
                    emit.running(RenderStage::Clips, i + 1, &meter, meter.fraction(ms as f64));
                })
                .await;
            if let Err(e) = result {
                let _ = std::fs::remove_file(&tmp);
                return Err(e);
            }
            let final_rel = clip_path(&task.scene_id);
            std::fs::rename(&tmp, dir.join(&final_rel))?;
            self.set_clip(project_id, &task.scene_id, AssetStatus::Done, Some((&final_rel, &task.hash)))
                .await;
            meter.done_ms += task.spec.duration_ms() as f64;
            self.log.info(&format!(
                "{} clip ok ({:.1} s for {:.1} s of video)",
                task.scene_id,
                clip_started.elapsed().as_secs_f64(),
                task.spec.duration_ms() as f64 / 1000.0
            ));
        }

        // Concat into a temp file inside the project, then move it to its final place.
        let clips: Vec<String> = tasks.iter().map(|t| clip_path(&t.scene_id)).collect();
        std::fs::write(dir.join("concat.txt"), concat_list(&clips))?;
        std::fs::create_dir_all(dir.join("output"))?;
        let tmp_rel = "output/final.tmp.mp4";
        emit.running(RenderStage::Concat, 0, &meter, meter.fraction(0.0));
        let result = self
            .ffmpeg
            .run(&concat_args("concat.txt", tmp_rel), &dir, cancel, |ms| {
                emit.running(RenderStage::Concat, 0, &meter, meter.fraction(ms as f64 * CONCAT_WEIGHT));
            })
            .await;
        if let Err(e) = result {
            let _ = std::fs::remove_file(dir.join(tmp_rel));
            return Err(e);
        }
        move_file(&dir.join(tmp_rel), &output)?;

        let out = output.display().to_string();
        self.store
            .update(project_id, |p| {
                p.status = ProjectStatus::Done;
                p.render.status = "done".into();
                p.render.output_path = Some(out.clone());
                Ok(())
            })
            .await
            .map_err(io)?;
        Ok(output)
    }

    async fn set_clip(&self, project_id: &str, scene_id: &str, status: AssetStatus, done: Option<(&str, &str)>) {
        let _ = self
            .store
            .update(project_id, |p| {
                if let Some(s) = p.scenes.iter_mut().find(|s| s.id == scene_id) {
                    s.clip.status = status;
                    if let Some((path, hash)) = done {
                        s.clip.path = Some(path.to_owned());
                        s.clip.hash = Some(hash.to_owned());
                    }
                }
                Ok(())
            })
            .await;
    }
}

/// Rename, or copy + delete when the output folder is on another drive.
fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if to.exists() {
        std::fs::remove_file(to)?;
    }
    if std::fs::rename(from, to).is_err() {
        std::fs::copy(from, to)?;
        std::fs::remove_file(from)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::hashing::subs_hash;
    use crate::pipeline::project::tests::sample;
    use crate::pipeline::project::SubLine;

    /// Two scenes with voice, image and subtitle files on disk.
    fn ready_project(dir: &Path) -> Project {
        let mut p = sample();
        for (i, id) in ["s001", "s002"].iter().enumerate() {
            let mut s = Scene::new((*id).into(), 0);
            s.narration = format!("line {i}");
            s.voice.status = AssetStatus::Done;
            s.voice.path = Some(format!("scenes/{id}/voice.mp3"));
            s.voice.text_hash = Some("t".into());
            s.voice.duration_ms = Some(4000);
            s.image.status = AssetStatus::Done;
            s.image.path = Some(format!("scenes/{id}/image_1080.png"));
            s.image.prompt_hash = Some("p".into());
            std::fs::create_dir_all(dir.join(format!("scenes/{id}/subs"))).unwrap();
            for f in ["voice.mp3", "image_1080.png", "subs/000.png", "subs/track.ffconcat"] {
                std::fs::write(dir.join(format!("scenes/{id}/{f}")), b"x").unwrap();
            }
            p.scenes.push(s);
        }
        for i in 0..p.scenes.len() {
            let h = subs_hash(&p, &p.scenes[i]);
            let s = &mut p.scenes[i];
            s.subs.status = AssetStatus::Done;
            s.subs.hash = h;
            s.subs.lines = vec![SubLine {
                text: "a".into(),
                start_ms: 0,
                end_ms: 4000,
                path: format!("scenes/{}/subs/000.png", s.id),
            }];
        }
        p
    }

    #[test]
    fn ready_when_every_asset_is_current() {
        let dir = tempfile::tempdir().unwrap();
        let p = ready_project(dir.path());
        check_ready(&p, dir.path()).unwrap();
    }

    #[test]
    fn missing_image_or_outdated_subs_block_the_render() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = ready_project(dir.path());
        p.scenes[1].image.status = AssetStatus::Stale;
        let e = check_ready(&p, dir.path()).unwrap_err().to_string();
        assert!(e.contains("1 image(s)"), "{e}");

        let mut p = ready_project(dir.path());
        p.scenes[0].narration = "changed".into();
        let e = check_ready(&p, dir.path()).unwrap_err().to_string();
        assert!(e.contains("subtitles for 1 scene(s)"), "{e}");
    }

    #[test]
    fn clips_are_reused_only_with_the_same_inputs() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = ready_project(dir.path());
        let tasks = plan_clips(&p, dir.path(), Encoder::X264);
        assert!(tasks.iter().all(|t| !t.fresh));
        assert!(matches!(tasks[0].spec.input, Input::Image { motion: Motion::Pan(crate::render::clip::Pan::LeftToRight), .. }));
        assert!(matches!(tasks[1].spec.input, Input::Image { motion: Motion::Pan(crate::render::clip::Pan::RightToLeft), .. }));

        std::fs::write(dir.path().join("scenes/s001/clip.mkv"), b"x").unwrap();
        p.scenes[0].clip.status = AssetStatus::Done;
        p.scenes[0].clip.hash = Some(tasks[0].hash.clone());
        assert!(plan_clips(&p, dir.path(), Encoder::X264)[0].fresh);
        assert!(!plan_clips(&p, dir.path(), Encoder::Nvenc)[0].fresh, "encoder is part of the hash");
        p.scenes[0].voice.duration_ms = Some(4100);
        assert!(!plan_clips(&p, dir.path(), Encoder::X264)[0].fresh);
    }

    #[test]
    fn output_goes_to_the_chosen_folder_with_a_safe_name() {
        let mut p = sample();
        p.title = "How was Angkor Wat built? <1/2>".into();
        let dir = Path::new("C:/data/projects/x");
        assert_eq!(output_path(&p, dir, None), dir.join("output").join("final.mp4"));
        let out = output_path(&p, dir, Some("D:/Videos"));
        assert_eq!(out, Path::new("D:/Videos").join("How was Angkor Wat built_ _1_2_.mp4"));
        p.title = "  ...  ".into();
        assert!(output_path(&p, dir, Some("D:/Videos")).ends_with("video.mp4"));
    }

    #[test]
    fn timeline_matches_the_render_plan() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = ready_project(dir.path());
        p.scenes.push(Scene::new("s003".into(), 0)); // no voice yet
        let t = timeline(&p, dir.path());
        let plan = plan_clips(&p, dir.path(), Encoder::X264);
        assert_eq!(t[0].start_ms, 0);
        assert_eq!(t[0].clip_ms, plan[0].spec.duration_ms());
        assert_eq!(t[1].start_ms, t[0].clip_ms);
        assert_eq!((t[0].pan, t[1].pan), ("lr", "rl"));
        assert!(t[0].voice_ready && t[0].image_ready);
        assert!(!t[2].voice_ready);
        assert_eq!(t[2].clip_ms, PLACEHOLDER_MS);
    }

    #[test]
    fn trailing_silence_is_trimmed_only_with_word_timings() {
        assert_eq!(trimmed_voice_ms(8208, Some(7362)), 7512);
        assert_eq!(trimmed_voice_ms(8208, None), 8208);
        assert_eq!(trimmed_voice_ms(8208, Some(8150)), 8208, "never longer than the file");
        assert_eq!(trimmed_voice_ms(8208, Some(0)), 8208);
    }

    #[test]
    fn eta_needs_some_progress_first() {
        let m = Meter { started: Instant::now(), total_ms: 1000.0, done_ms: 0.0 };
        assert_eq!(m.eta(0.0), None);
        assert!((m.fraction(500.0) - 0.5).abs() < 1e-9);
        let empty = Meter { started: Instant::now(), total_ms: 0.0, done_ms: 0.0 };
        assert!((empty.fraction(0.0) - 1.0).abs() < 1e-9);
    }
}
