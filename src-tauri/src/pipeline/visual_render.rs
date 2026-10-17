//! What a real-world scene's clip shows (`docs/11-real-world-visuals.md` §9): the photo
//! prepared at frame × zoom (cropped around the subject point, never enlarged past its own
//! pixels: small photos get no zoom), a stock clip, or the drawn diagram / code picture.
//! AI-image scenes keep the pan over `image_1080.png` (`render_job`).

use std::path::Path;

use image::imageops::FilterType;

use super::hashing::sha;
use super::project::{AssetStatus, Project, Scene, VisualKind};
use crate::images::postprocess::crop_16x9_at;
use crate::render::clip::{Frame, Input, Motion, Pan};
use crate::stock::Media;
use crate::stock::assets::AssetRegistry;
use crate::stock::rank::max_zoom;
use crate::storage::atomic;

const PHOTO_JPEG_QUALITY: u8 = 92;

/// A photo (or a clip's poster) cut and sized for the frame, written once per input.
#[derive(Debug, Clone, PartialEq)]
pub struct PhotoPrep {
    /// Source file, relative to the project.
    pub src: String,
    /// 16:9 crop (x, y, w, h) in source pixels.
    pub crop: (u32, u32, u32, u32),
    /// Output size: frame × zoom (even numbers).
    pub size: (u32, u32),
    /// `scenes/<id>/photo_<res>_<hash>.jpg`.
    pub out: String,
}

/// The clip input of a scene, or why it can't be rendered yet.
#[derive(Debug, Clone, PartialEq)]
pub enum ScenePlan {
    /// Use the AI image with this pan.
    AiImage,
    Ready { input: Input, prep: Option<PhotoPrep>, hash: String },
    Missing(&'static str),
}

fn even(v: f32) -> u32 {
    ((v.round() as u32) / 2 * 2).max(2)
}

fn photo_prep(scene_id: &str, src: &str, (w, h): (u32, u32), focus: [f32; 2], frame: Frame, zoom: f32, resolution: &str) -> PhotoPrep {
    let crop = crop_16x9_at(w, h, focus);
    let size = (even(frame.width as f32 * zoom), even(frame.height as f32 * zoom));
    let key = sha(&["photo", src, &format!("{crop:?}"), &format!("{size:?}")]);
    PhotoPrep { src: src.to_owned(), crop, size, out: format!("scenes/{scene_id}/photo_{resolution}_{}.jpg", &key[..12]) }
}

/// Clip input for scene `index` of a real-world project.
pub fn plan(p: &Project, s: &Scene, index: usize, dir: &Path, registry: &AssetRegistry) -> ScenePlan {
    if s.needs_ai_image() {
        return ScenePlan::AiImage;
    }
    let frame = Frame::for_resolution(&p.render.resolution);
    let res = p.render.resolution.as_str();
    let v = &s.visual;
    let focus = v.focus.unwrap_or([0.5, 0.5]);
    match v.kind {
        VisualKind::Diagram | VisualKind::Code => {
            let Some(path) = v.render_path.clone().filter(|r| v.status == AssetStatus::Done && dir.join(r).is_file()) else {
                return ScenePlan::Missing("diagram or code picture");
            };
            let hash = sha(&["drawn", &path, v.render_hash.as_deref().unwrap_or("")]);
            let typed = v.kind == VisualKind::Code
                && v.code.as_ref().is_some_and(|c| c.type_out)
                && v.frames.len() > 1
                && v.frames.iter().all(|f| dir.join(f).is_file());
            if typed {
                return ScenePlan::Ready { input: Input::Frames { track: code_track_path(&s.id) }, prep: None, hash };
            }
            ScenePlan::Ready { input: Input::Image { path, motion: Motion::Still }, prep: None, hash }
        }
        VisualKind::RealPhoto | VisualKind::StockVideo | VisualKind::Screenshot => {
            let Some(r) = v.asset.as_deref().and_then(|id| registry.get(id)).filter(|r| dir.join(&r.file).is_file()) else {
                return ScenePlan::Missing("photo or clip");
            };
            if r.media == Media::Video {
                let hash = sha(&["video", &r.file, &r.sha256, &format!("{focus:?}")]);
                return ScenePlan::Ready { input: Input::Video { path: r.file.clone(), focus }, prep: None, hash };
            }
            let zoom = max_zoom(r.width, r.height, frame.width);
            let motion = if zoom > 1.005 { Motion::Zoom { to: zoom, zoom_in: index.is_multiple_of(2) } } else { Motion::Still };
            let prep = photo_prep(&s.id, &r.file, (r.width, r.height), focus, frame, motion.zoom(), res);
            let hash = sha(&["photo", &r.sha256, &prep.out, motion.as_str(), &format!("{:.4}", motion.zoom())]);
            ScenePlan::Ready { input: Input::Image { path: prep.out.clone(), motion }, prep: Some(prep), hash }
        }
        VisualKind::AiImage => ScenePlan::AiImage,
    }
}

/// The picture the editor preview shows: the prepared photo, a clip's poster (frame size,
/// cropped around the subject) or the drawn picture; with the motion the clip will use.
pub fn preview(p: &Project, s: &Scene, index: usize, dir: &Path, registry: &AssetRegistry) -> Option<(String, Option<PhotoPrep>, Motion)> {
    match plan(p, s, index, dir, registry) {
        ScenePlan::Ready { input: Input::Image { path, motion }, prep, .. } => Some((path, prep, motion)),
        // Typed-out code previews as the finished card.
        ScenePlan::Ready { input: Input::Frames { .. }, .. } => s.visual.render_path.clone().map(|p| (p, None, Motion::Still)),
        ScenePlan::Ready { input: Input::Video { focus, .. }, .. } => {
            let r = s.visual.asset.as_deref().and_then(|id| registry.get(id))?;
            let poster = r.poster.clone()?;
            let (w, h) = image::image_dimensions(dir.join(&poster)).ok()?;
            let frame = Frame::for_resolution(&p.render.resolution);
            let prep = photo_prep(&s.id, &poster, (w, h), focus, frame, 1.0, &p.render.resolution);
            Some((prep.out.clone(), Some(prep), Motion::Still))
        }
        _ => None,
    }
}

/// Writes `prep.out` if it doesn't exist yet (and removes older photos of the scene).
/// CPU work: call from `spawn_blocking`.
pub fn ensure(dir: &Path, prep: &PhotoPrep) -> Result<(), String> {
    let out = dir.join(&prep.out);
    if out.is_file() {
        return Ok(());
    }
    let img = image::open(dir.join(&prep.src)).map_err(|e| format!("could not read {}: {e}", prep.src))?;
    let (x, y, w, h) = prep.crop;
    let sized = img.crop_imm(x, y, w, h).resize_exact(prep.size.0, prep.size.1, FilterType::Lanczos3).to_rgb8();
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, PHOTO_JPEG_QUALITY)
        .encode_image(&sized)
        .map_err(|e| e.to_string())?;
    if let Some(scene_dir) = out.parent()
        && let Ok(entries) = std::fs::read_dir(scene_dir)
    {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with("photo_") && name.ends_with(".jpg") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    atomic::write_bytes(&out, bytes.get_ref()).map_err(|e| e.to_string())
}

/// `scenes/<id>/code/track.ffconcat` (written by the render job with the voice timing).
pub fn code_track_path(scene_id: &str) -> String {
    format!("scenes/{scene_id}/code/track.ffconcat")
}

/// Pan of an AI-image scene (same cycle as before real-world scenes existed).
pub fn ai_pan(index: usize) -> Pan {
    Pan::for_index(index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::project::tests::sample;
    use crate::stock::Source;
    use crate::stock::assets::AssetRecord;
    use crate::stock::license::{License, LicenseCode};

    fn record(id: &str, media: Media, w: u32, h: u32) -> AssetRecord {
        AssetRecord {
            id: id.into(),
            source: Source::Wikimedia,
            media,
            file: format!("assets/{id}.{}", if media == Media::Video { "mp4" } else { "jpg" }),
            title: String::new(),
            author: String::new(),
            author_url: None,
            license: License::new(LicenseCode::Cc0),
            original_url: String::new(),
            file_url: String::new(),
            downloaded_at: String::new(),
            width: w,
            height: h,
            duration_ms: None,
            attribution: String::new(),
            restrictions: vec![],
            sha256: "abc".into(),
            bytes: 1,
            poster: None,
        }
    }

    fn setup(w: u32, h: u32) -> (tempfile::TempDir, Project, AssetRegistry) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("assets")).unwrap();
        image::RgbImage::from_pixel(w, h, image::Rgb([200, 30, 30])).save(dir.path().join("assets/w_1.jpg")).unwrap();
        let mut p = sample();
        p.style.preset = "tech_explainer".into();
        let mut s = Scene::new("s001".into(), 0);
        s.visual.kind = VisualKind::RealPhoto;
        s.visual.asset = Some("w_1".into());
        s.visual.status = AssetStatus::Done;
        p.scenes.push(s);
        (dir, p, AssetRegistry { assets: vec![record("w_1", Media::Photo, w, h)] })
    }

    #[test]
    fn big_photos_zoom_and_are_prepared_at_frame_times_zoom() {
        let (dir, p, reg) = setup(2400, 1600);
        let ScenePlan::Ready { input: Input::Image { motion, path }, prep: Some(prep), .. } = plan(&p, &p.scenes[0], 0, dir.path(), &reg) else {
            panic!("ready photo expected")
        };
        assert_eq!(motion, Motion::Zoom { to: 1.08, zoom_in: true });
        assert_eq!(prep.size, (2074, 1166));
        assert_eq!(prep.crop, (0, 125, 2400, 1350));
        assert_eq!(path, prep.out);
        ensure(dir.path(), &prep).unwrap();
        assert_eq!(image::image_dimensions(dir.path().join(&prep.out)).unwrap(), (2074, 1166));
    }

    #[test]
    fn small_photos_never_zoom() {
        let (dir, p, reg) = setup(1280, 853);
        let ScenePlan::Ready { input: Input::Image { motion, .. }, prep: Some(prep), .. } = plan(&p, &p.scenes[0], 1, dir.path(), &reg) else {
            panic!("ready photo expected")
        };
        assert_eq!(motion, Motion::Still);
        assert_eq!(prep.size, (1920, 1080));
        // Slightly bigger than the frame: zoom only up to its own pixels.
        let (dir, p, reg) = setup(2000, 1125);
        let ScenePlan::Ready { input: Input::Image { motion, .. }, .. } = plan(&p, &p.scenes[0], 1, dir.path(), &reg) else { panic!() };
        assert!(matches!(motion, Motion::Zoom { to, zoom_in: false } if (to - 2000.0 / 1920.0).abs() < 1e-4), "{motion:?}");
    }

    #[test]
    fn missing_files_block_and_failed_searches_use_the_ai_image() {
        let (dir, mut p, reg) = setup(2400, 1600);
        p.scenes[0].visual.asset = None;
        assert_eq!(plan(&p, &p.scenes[0], 0, dir.path(), &reg), ScenePlan::Missing("photo or clip"));
        p.scenes[0].visual.status = AssetStatus::Failed;
        assert_eq!(plan(&p, &p.scenes[0], 0, dir.path(), &reg), ScenePlan::AiImage);
        p.scenes[0].visual.kind = VisualKind::Diagram;
        assert_eq!(plan(&p, &p.scenes[0], 0, dir.path(), &reg), ScenePlan::Missing("diagram or code picture"));
    }

    #[test]
    fn clips_play_from_their_file() {
        let (dir, mut p, mut reg) = setup(2400, 1600);
        reg.assets.push(record("v_2", Media::Video, 1920, 1080));
        std::fs::write(dir.path().join("assets/v_2.mp4"), b"x").unwrap();
        p.scenes[0].visual.kind = VisualKind::StockVideo;
        p.scenes[0].visual.asset = Some("v_2".into());
        p.scenes[0].visual.focus = Some([0.5, 0.3]);
        let ScenePlan::Ready { input, prep: None, .. } = plan(&p, &p.scenes[0], 0, dir.path(), &reg) else { panic!() };
        assert_eq!(input, Input::Video { path: "assets/v_2.mp4".into(), focus: [0.5, 0.3] });
    }
}
