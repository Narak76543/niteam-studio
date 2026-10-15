//! Subtitle PNG bookkeeping (03 Step 5). Lines are segmented, timed and drawn in the WebView
//! (only the browser shapes Khmer reliably, 04 §3); Rust decides which scenes need new PNGs
//! and stores what the UI hands back.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::ProjectError;
use super::hashing::subs_hash;
use super::project::{AssetStatus, Project, Scene, SubLine, SubsAsset};
use crate::render::subs_track::{TRACK_FILE, TrackImage, ffconcat};
use crate::storage::atomic;
use crate::tts::WordTiming;

/// Sanity limits for what the UI may send for one scene (one image per spoken word).
const MAX_STATES: usize = 600;
const MAX_PNG_BYTES: usize = 3 * 1024 * 1024;
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// One scene whose subtitles must be (re)drawn.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SubsTask {
    pub scene_id: String,
    pub narration: String,
    pub duration_ms: u64,
    /// Edge TTS word/phrase timings; `None` for Gemini TTS.
    pub words: Option<Vec<WordTiming>>,
    /// Must be sent back with the PNGs; a mismatch means the scene changed meanwhile.
    pub hash: String,
}

/// One rendered subtitle state: a cue (up to 2 lines, `\n`-joined), possibly with the
/// spoken word highlighted. States are consecutive in time.
#[derive(Debug, Clone, Deserialize)]
pub struct CuePng {
    pub text: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub png: Vec<u8>,
}

/// Subtitles are current: done, same inputs, every PNG on disk.
pub fn is_fresh(project: &Project, scene: &Scene, dir: &Path) -> bool {
    let Some(hash) = subs_hash(project, scene) else {
        return false;
    };
    scene.subs.status == AssetStatus::Done
        && scene.subs.hash.as_deref() == Some(hash.as_str())
        && !scene.subs.lines.is_empty()
        && scene.subs.lines.iter().all(|l| dir.join(&l.path).is_file())
        && dir.join(track_path(&scene.id)).is_file()
}

/// `scenes/<id>/subs/track.ffconcat`, relative to the project folder.
pub fn track_path(scene_id: &str) -> String {
    format!("scenes/{scene_id}/subs/{TRACK_FILE}")
}

/// Scenes with a finished voice whose subtitles are missing or outdated.
pub fn plan(project: &Project, dir: &Path) -> Vec<SubsTask> {
    project
        .scenes
        .iter()
        .filter(|s| s.voice.status == AssetStatus::Done && !is_fresh(project, s, dir))
        .filter_map(|s| {
            let hash = subs_hash(project, s)?;
            let words = s
                .voice
                .words_path
                .as_ref()
                .and_then(|p| atomic::read_json::<Vec<WordTiming>>(&dir.join(p)).ok().flatten());
            Some(SubsTask {
                scene_id: s.id.clone(),
                narration: s.narration.clone(),
                duration_ms: s.voice.duration_ms.unwrap_or(0),
                words,
                hash,
            })
        })
        .collect()
}

fn invalid(msg: &str) -> ProjectError {
    ProjectError::Invalid(msg.to_owned())
}

/// Checks the cues against the scene and writes `scenes/<id>/subs/NNN.png`.
/// Returns the new asset; the caller stores it with `apply`.
pub fn write_pngs(
    project: &Project,
    scene_id: &str,
    hash: &str,
    cues: &[CuePng],
    dir: &Path,
) -> Result<SubsAsset, ProjectError> {
    let scene = project
        .scenes
        .iter()
        .find(|s| s.id == scene_id)
        .ok_or(ProjectError::SceneNotFound)?;
    if subs_hash(project, scene).as_deref() != Some(hash) {
        return Err(invalid("The scene changed while its subtitles were drawn. Try again."));
    }
    if cues.is_empty() || cues.len() > MAX_STATES {
        return Err(invalid("Unexpected number of subtitle images."));
    }
    if cues.windows(2).any(|w| w[1].start_ms < w[0].start_ms) {
        return Err(invalid("Subtitle images are not in time order."));
    }
    let duration = scene.voice.duration_ms.unwrap_or(0);
    for c in cues {
        if c.start_ms > c.end_ms || c.end_ms > duration + 1000 {
            return Err(invalid("Subtitle timing is outside the scene."));
        }
        if c.png.len() > MAX_PNG_BYTES || !c.png.starts_with(PNG_SIGNATURE) {
            return Err(invalid("A subtitle image is not a valid PNG."));
        }
    }

    let rel_dir = format!("scenes/{scene_id}/subs");
    let abs_dir = dir.join(&rel_dir);
    if abs_dir.exists() {
        std::fs::remove_dir_all(&abs_dir).map_err(|e| ProjectError::Invalid(e.to_string()))?;
    }
    let mut lines = Vec::with_capacity(cues.len());
    let mut names = Vec::with_capacity(cues.len());
    for (i, c) in cues.iter().enumerate() {
        let name = format!("{i:03}.png");
        let rel = format!("{rel_dir}/{name}");
        atomic::write_bytes(&dir.join(&rel), &c.png)?;
        names.push(name);
        lines.push(SubLine {
            text: c.text.clone(),
            start_ms: c.start_ms,
            end_ms: c.end_ms,
            path: rel,
        });
    }
    let track: Vec<TrackImage<'_>> = cues
        .iter()
        .zip(&names)
        .map(|(c, name)| TrackImage { file: name, start_ms: c.start_ms, end_ms: c.end_ms })
        .collect();
    atomic::write_bytes(&dir.join(track_path(scene_id)), ffconcat(&track).as_bytes())?;
    Ok(SubsAsset {
        status: AssetStatus::Done,
        lines,
        hash: Some(hash.to_owned()),
    })
}

/// Stores new subtitles if the scene is still unchanged; its clip becomes outdated.
pub fn apply(p: &mut Project, scene_id: &str, subs: SubsAsset) -> Result<(), ProjectError> {
    let current = p
        .scenes
        .iter()
        .find(|s| s.id == scene_id)
        .and_then(|s| subs_hash(p, s));
    if current.is_none() || current != subs.hash {
        return Err(invalid("The scene changed while its subtitles were drawn. Try again."));
    }
    let scene = p
        .scenes
        .iter_mut()
        .find(|s| s.id == scene_id)
        .ok_or(ProjectError::SceneNotFound)?;
    scene.subs = subs;
    if scene.clip.status == AssetStatus::Done {
        scene.clip.status = AssetStatus::Stale;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::project::tests::sample;

    fn png() -> Vec<u8> {
        let mut v = PNG_SIGNATURE.to_vec();
        v.extend_from_slice(b"rest");
        v
    }

    fn project_with_voice() -> Project {
        let mut p = sample();
        let mut s = Scene::new("s001".into(), 0);
        s.narration = "តើស្រូវកើតមកពីណា?".into();
        s.voice.status = AssetStatus::Done;
        s.voice.text_hash = Some("abc".into());
        s.voice.duration_ms = Some(3000);
        p.scenes.push(s);
        p
    }

    fn cue(start: u64, end: u64) -> CuePng {
        CuePng { text: "line".into(), start_ms: start, end_ms: end, png: png() }
    }

    #[test]
    fn plans_scenes_with_voice_and_missing_subs() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = project_with_voice();
        p.scenes.push(Scene::new("s002".into(), 0)); // no voice yet
        let tasks = plan(&p, dir.path());
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].scene_id, "s001");
        assert_eq!(tasks[0].duration_ms, 3000);
        assert!(tasks[0].words.is_none());
    }

    #[test]
    fn saved_subs_are_fresh_until_the_narration_changes() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = project_with_voice();
        let hash = plan(&p, dir.path())[0].hash.clone();
        let subs = write_pngs(&p, "s001", &hash, &[cue(0, 1500), cue(1500, 3000)], dir.path()).unwrap();
        assert!(dir.path().join("scenes/s001/subs/001.png").is_file());
        let track = std::fs::read_to_string(dir.path().join(track_path("s001"))).unwrap();
        assert!(track.contains("file '000.png'\nduration 1.500"), "{track}");
        apply(&mut p, "s001", subs).unwrap();
        assert!(plan(&p, dir.path()).is_empty());

        p.scenes[0].narration = "ថ្មី".into();
        assert_eq!(plan(&p, dir.path()).len(), 1);
    }

    #[test]
    fn rejects_outdated_hash_bad_png_and_bad_times() {
        let dir = tempfile::tempdir().unwrap();
        let p = project_with_voice();
        let hash = plan(&p, dir.path())[0].hash.clone();
        assert!(write_pngs(&p, "s001", "old", &[cue(0, 100)], dir.path()).is_err());
        let mut bad = cue(0, 100);
        bad.png = b"GIF89a".to_vec();
        assert!(write_pngs(&p, "s001", &hash, &[bad], dir.path()).is_err());
        assert!(write_pngs(&p, "s001", &hash, &[cue(200, 100)], dir.path()).is_err());
        assert!(write_pngs(&p, "s001", &hash, &[cue(0, 60_000)], dir.path()).is_err());
        assert!(write_pngs(&p, "s001", &hash, &[], dir.path()).is_err());
        assert!(write_pngs(&p, "s001", &hash, &[cue(500, 900), cue(0, 400)], dir.path()).is_err());
    }

    #[test]
    fn new_subs_make_a_done_clip_stale() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = project_with_voice();
        p.scenes[0].clip.status = AssetStatus::Done;
        let hash = plan(&p, dir.path())[0].hash.clone();
        let subs = write_pngs(&p, "s001", &hash, &[cue(0, 3000)], dir.path()).unwrap();
        apply(&mut p, "s001", subs).unwrap();
        assert_eq!(p.scenes[0].clip.status, AssetStatus::Stale);
    }
}
