//! Diagram and code scenes are drawn in the WebView (Mermaid, code card; Khmer shaping like the
//! subtitles, `docs/11-real-world-visuals.md` §10): Rust says what to draw (`plan`), the UI
//! draws a frame-size PNG and hands it back (`save`). A drawing is redone only when its text,
//! the resolution or the drawing code (`DRAW_VERSION`) changed.

use std::path::Path;

use serde::Serialize;

use super::hashing::sha;
use super::project::{AssetStatus, CodeSpec, DiagramSpec, Project, Scene, VisualKind};
use crate::images::postprocess;
use crate::render::clip::Frame;
use crate::render::subs_track::{TrackImage, ffconcat};
use crate::storage::{StorageError, atomic};

/// Bump when the drawing code in the UI changes the look.
pub const DRAW_VERSION: &str = "1";

/// One picture for the UI to draw.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DrawTask {
    pub scene_id: String,
    /// "diagram" | "code"
    pub kind: &'static str,
    pub hash: String,
    pub diagram: Option<DiagramSpec>,
    pub code: Option<CodeSpec>,
    pub width: u32,
    pub height: u32,
    /// Top of the subtitle band: keep the picture's content above it.
    pub band_top: u32,
}

/// What the drawing of `s` depends on, or `None` for scenes that aren't drawn.
pub fn draw_hash(p: &Project, s: &Scene) -> Option<String> {
    let res = p.render.resolution.as_str();
    match (s.visual.kind, &s.visual.diagram, &s.visual.code) {
        (VisualKind::Diagram, Some(d), _) => Some(sha(&["diagram", DRAW_VERSION, &d.mermaid, res])),
        (VisualKind::Code, _, Some(c)) => {
            Some(sha(&["code", DRAW_VERSION, &c.language, &c.text, &c.terminal.to_string(), &c.type_out.to_string(), res]))
        }
        _ => None,
    }
}

/// Scenes whose picture is missing or outdated (failed ones too, so a fixed text is redrawn).
pub fn plan(p: &Project, dir: &Path) -> Vec<DrawTask> {
    let frame = Frame::for_resolution(&p.render.resolution);
    p.scenes
        .iter()
        .filter_map(|s| {
            let hash = draw_hash(p, s)?;
            let v = &s.visual;
            let fresh = v.status == AssetStatus::Done
                && v.render_hash.as_deref() == Some(hash.as_str())
                && v.render_path.as_ref().is_some_and(|r| dir.join(r).is_file());
            if fresh || (v.status == AssetStatus::Failed && v.render_hash.as_deref() == Some(hash.as_str())) {
                return None;
            }
            Some(DrawTask {
                scene_id: s.id.clone(),
                kind: if v.kind == VisualKind::Diagram { "diagram" } else { "code" },
                hash,
                diagram: v.diagram.clone().filter(|_| v.kind == VisualKind::Diagram),
                code: v.code.clone().filter(|_| v.kind == VisualKind::Code),
                width: frame.width,
                height: frame.height,
                band_top: frame.subs_y(),
            })
        })
        .collect()
}

#[derive(Debug, thiserror::Error)]
pub enum DrawError {
    #[error("this drawing is out of date (the text changed meanwhile)")]
    Outdated,
    #[error("the picture has the wrong size ({0}×{1})")]
    WrongSize(u32, u32),
    #[error("could not read the picture: {0}")]
    Image(String),
    #[error(transparent)]
    Storage(#[from] StorageError),
}

/// Paths written by [`save`].
#[derive(Debug, Clone, PartialEq)]
pub struct Saved {
    pub picture: String,
    pub thumb: String,
    /// Typed-out code frames (`scenes/<id>/code/000.png`…), empty otherwise.
    pub frames: Vec<String>,
}

/// Writes `scenes/<id>/visual.png` + its grid thumbnail, and for typed-out code one picture
/// per revealed line (old ones removed). `hash` must still match the scene (`plan`).
pub fn save(p: &Project, dir: &Path, scene_id: &str, hash: &str, png: &[u8], frames: &[Vec<u8>]) -> Result<Saved, DrawError> {
    let scene = p.scenes.iter().find(|s| s.id == scene_id).ok_or(DrawError::Outdated)?;
    if draw_hash(p, scene).as_deref() != Some(hash) {
        return Err(DrawError::Outdated);
    }
    let frame = Frame::for_resolution(&p.render.resolution);
    let (w, h) = postprocess::dimensions(png).ok_or_else(|| DrawError::Image("not a PNG".into()))?;
    if (w, h) != (frame.width, frame.height) {
        return Err(DrawError::WrongSize(w, h));
    }
    for f in frames {
        let size = postprocess::dimensions(f).ok_or_else(|| DrawError::Image("not a PNG".into()))?;
        if size != (frame.width, frame.height) {
            return Err(DrawError::WrongSize(size.0, size.1));
        }
    }
    let thumb = postprocess::thumb_at(png, [0.5, 0.5]).map_err(|e| DrawError::Image(e.to_string()))?;
    let pic = format!("scenes/{scene_id}/visual.png");
    let thumb_rel = format!("scenes/{scene_id}/visual_thumb.jpg");
    atomic::write_bytes(&dir.join(&pic), png)?;
    atomic::write_bytes(&dir.join(&thumb_rel), &thumb)?;
    let code_dir = dir.join(format!("scenes/{scene_id}/code"));
    let _ = std::fs::remove_dir_all(&code_dir);
    let mut written = Vec::with_capacity(frames.len());
    for (i, f) in frames.iter().enumerate() {
        let rel = format!("scenes/{scene_id}/code/{i:03}.png");
        atomic::write_bytes(&dir.join(&rel), f)?;
        written.push(rel);
    }
    Ok(Saved { picture: pic, thumb: thumb_rel, frames: written })
}

/// Typed-out code track: frame i appears at i × step, where the reveal takes 60 % of the voice
/// (at most 0.9 s per line); the last frame stays until the clip ends.
pub fn code_track(frames: &[String], voice_ms: u64, clip_ms: u64) -> String {
    let n = frames.len().max(1) as u64;
    let step = (voice_ms * 6 / 10).min(n * 900) / n;
    let names: Vec<&str> = frames.iter().map(|f| f.rsplit('/').next().unwrap_or(f)).collect();
    let images: Vec<TrackImage> = names
        .iter()
        .enumerate()
        .map(|(i, file)| TrackImage { file, start_ms: i as u64 * step, end_ms: clip_ms })
        .collect();
    ffconcat(&images)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::project::tests::sample;

    fn project() -> Project {
        let mut p = sample();
        let mut d = Scene::new("s001".into(), 0);
        d.visual.kind = VisualKind::Diagram;
        d.visual.diagram = Some(DiagramSpec { template: "flow".into(), mermaid: "flowchart LR\n A --> B".into() });
        let mut c = Scene::new("s002".into(), 0);
        c.visual.kind = VisualKind::Code;
        c.visual.code = Some(CodeSpec { language: "bash".into(), text: "$ ping example.com".into(), terminal: true, type_out: false });
        let photo = Scene::new("s003".into(), 0);
        p.scenes = vec![d, c, photo];
        p
    }

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        image::RgbImage::from_pixel(w, h, image::Rgb([250, 250, 250])).write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn draws_diagrams_and_code_once_until_their_text_changes() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = project();
        let tasks = plan(&p, dir.path());
        assert_eq!(tasks.iter().map(|t| t.kind).collect::<Vec<_>>(), vec!["diagram", "code"]);
        assert_eq!((tasks[0].width, tasks[0].band_top), (1920, 816));
        let saved = save(&p, dir.path(), "s001", &tasks[0].hash, &png(1920, 1080), &[]).unwrap();
        assert!(dir.path().join(&saved.thumb).is_file());
        let v = &mut p.scenes[0].visual;
        (v.render_path, v.render_hash, v.status) = (Some(saved.picture), Some(tasks[0].hash.clone()), AssetStatus::Done);
        assert_eq!(plan(&p, dir.path()).len(), 1, "diagram is fresh");
        p.scenes[0].visual.diagram.as_mut().unwrap().mermaid.push_str("\n B --> C");
        assert_eq!(plan(&p, dir.path()).len(), 2, "changed text is redrawn");
        assert!(matches!(save(&p, dir.path(), "s001", &tasks[0].hash, &png(1920, 1080), &[]), Err(DrawError::Outdated)));
    }

    #[test]
    fn typed_code_keeps_one_frame_per_line_and_reveals_them_over_the_voice() {
        let dir = tempfile::tempdir().unwrap();
        let p = project();
        let t = &plan(&p, dir.path())[1];
        let frames = vec![png(1920, 1080), png(1920, 1080), png(1920, 1080)];
        let saved = save(&p, dir.path(), "s002", &t.hash, &png(1920, 1080), &frames).unwrap();
        assert_eq!(saved.frames, vec!["scenes/s002/code/000.png", "scenes/s002/code/001.png", "scenes/s002/code/002.png"]);
        assert!(dir.path().join("scenes/s002/code/002.png").is_file());
        // 3 lines, 6 s voice: 0.9 s per line (cap), the last frame until the clip ends.
        assert_eq!(
            code_track(&saved.frames, 6_000, 6_250),
            "ffconcat version 1.0\nfile '000.png'\nduration 0.900\nfile '001.png'\nduration 0.900\nfile '002.png'\nduration 4.450\nfile '002.png'\n"
        );
        // Short voice: the reveal takes 60 % of it.
        assert!(code_track(&saved.frames, 1_500, 1_750).contains("duration 0.300"));
    }

    #[test]
    fn rejects_wrong_sizes() {
        let dir = tempfile::tempdir().unwrap();
        let p = project();
        let t = &plan(&p, dir.path())[1];
        assert!(matches!(save(&p, dir.path(), "s002", &t.hash, &png(1280, 720), &[]), Err(DrawError::WrongSize(1280, 720))));
        assert!(matches!(save(&p, dir.path(), "s002", &t.hash, &png(1920, 1080), &[png(10, 10)]), Err(DrawError::WrongSize(10, 10))));
    }
}
