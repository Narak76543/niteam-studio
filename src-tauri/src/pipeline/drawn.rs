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

/// Writes `scenes/<id>/visual.png` + its grid thumbnail; returns (picture, thumbnail) paths.
/// `hash` must still match the scene (`plan`).
pub fn save(p: &Project, dir: &Path, scene_id: &str, hash: &str, png: &[u8]) -> Result<(String, String), DrawError> {
    let scene = p.scenes.iter().find(|s| s.id == scene_id).ok_or(DrawError::Outdated)?;
    if draw_hash(p, scene).as_deref() != Some(hash) {
        return Err(DrawError::Outdated);
    }
    let frame = Frame::for_resolution(&p.render.resolution);
    let (w, h) = postprocess::dimensions(png).ok_or_else(|| DrawError::Image("not a PNG".into()))?;
    if (w, h) != (frame.width, frame.height) {
        return Err(DrawError::WrongSize(w, h));
    }
    let thumb = postprocess::thumb_at(png, [0.5, 0.5]).map_err(|e| DrawError::Image(e.to_string()))?;
    let pic = format!("scenes/{scene_id}/visual.png");
    let thumb_rel = format!("scenes/{scene_id}/visual_thumb.jpg");
    atomic::write_bytes(&dir.join(&pic), png)?;
    atomic::write_bytes(&dir.join(&thumb_rel), &thumb)?;
    Ok((pic, thumb_rel))
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
        let (pic, thumb) = save(&p, dir.path(), "s001", &tasks[0].hash, &png(1920, 1080)).unwrap();
        assert!(dir.path().join(&thumb).is_file());
        let v = &mut p.scenes[0].visual;
        (v.render_path, v.render_hash, v.status) = (Some(pic), Some(tasks[0].hash.clone()), AssetStatus::Done);
        assert_eq!(plan(&p, dir.path()).len(), 1, "diagram is fresh");
        p.scenes[0].visual.diagram.as_mut().unwrap().mermaid.push_str("\n B --> C");
        assert_eq!(plan(&p, dir.path()).len(), 2, "changed text is redrawn");
        assert!(matches!(save(&p, dir.path(), "s001", &tasks[0].hash, &png(1920, 1080)), Err(DrawError::Outdated)));
    }

    #[test]
    fn rejects_wrong_sizes() {
        let dir = tempfile::tempdir().unwrap();
        let p = project();
        let t = &plan(&p, dir.path())[1];
        assert!(matches!(save(&p, dir.path(), "s002", &t.hash, &png(1280, 720)), Err(DrawError::WrongSize(1280, 720))));
    }
}
