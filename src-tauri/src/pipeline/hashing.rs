//! Text normalization and input hashes. A done asset whose input hash no longer
//! matches becomes `stale` — that is what makes resume and partial regeneration work.

use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

use super::project::{AssetStatus, Project, Scene};

const ZERO_WIDTH_SPACE: char = '\u{200B}';
const SEP: &str = "\u{1F}";

/// NFC-normalizes text and trims surrounding whitespace.
pub fn normalize(text: &str) -> String {
    text.nfc().collect::<String>().trim().to_owned()
}

/// Like [`normalize`] and also removes zero-width spaces (TTS input).
pub fn normalize_spoken(text: &str) -> String {
    normalize(text).replace(ZERO_WIDTH_SPACE, "")
}

fn sha(parts: &[&str]) -> String {
    let mut h = Sha256::new();
    h.update(parts.join(SEP).as_bytes());
    hex::encode(&h.finalize()[..16])
}

/// Hash of everything that changes the voice: spoken text + voice settings.
pub fn text_hash(project: &Project, scene: &Scene) -> String {
    let v = &project.voice;
    sha(&[
        &normalize_spoken(&scene.spoken_text),
        &v.model,
        &v.voice_name,
        &v.direction,
    ])
}

/// Hash of everything that changes the image: style prompt + image prompt.
pub fn prompt_hash(project: &Project, scene: &Scene) -> String {
    sha(&[
        &normalize(&project.style.style_prompt),
        &normalize(&scene.image_prompt),
    ])
}

/// Marks done/failed assets of `scene_index` stale if their input changed.
/// Subtitles and clip depend on narration/voice/image, so they follow.
pub fn mark_stale(project: &mut Project, scene_index: usize) {
    let Some(scene) = project.scenes.get(scene_index) else {
        return;
    };
    let th = text_hash(project, scene);
    let ph = prompt_hash(project, scene);
    let Some(scene) = project.scenes.get_mut(scene_index) else {
        return;
    };
    let voice_changed = is_outdated(scene.voice.status, scene.voice.text_hash.as_deref(), &th);
    let image_changed = is_outdated(scene.image.status, scene.image.prompt_hash.as_deref(), &ph);
    if voice_changed {
        scene.voice.status = AssetStatus::Stale;
    }
    if image_changed {
        scene.image.status = AssetStatus::Stale;
    }
    if voice_changed || image_changed {
        for st in [&mut scene.subs.status, &mut scene.clip.status] {
            if *st == AssetStatus::Done {
                *st = AssetStatus::Stale;
            }
        }
    }
}

fn is_outdated(status: AssetStatus, stored: Option<&str>, current: &str) -> bool {
    status == AssetStatus::Done && stored != Some(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::project::tests::sample;

    #[test]
    fn nfc_makes_equivalent_text_hash_equal() {
        // "é" composed vs decomposed
        assert_eq!(normalize("caf\u{00E9}"), normalize("cafe\u{0301}"));
    }

    #[test]
    fn spoken_text_drops_zero_width_spaces() {
        assert_eq!(normalize_spoken("ស្រូវ\u{200B}កើត"), "ស្រូវកើត");
    }

    #[test]
    fn edit_marks_done_voice_stale() {
        let mut p = sample();
        let mut s = Scene::new("s001".into(), 0);
        s.spoken_text = "មួយ".into();
        p.scenes.push(s);
        let h = text_hash(&p, &p.scenes[0]);
        p.scenes[0].voice.status = AssetStatus::Done;
        p.scenes[0].voice.text_hash = Some(h);
        p.scenes[0].clip.status = AssetStatus::Done;

        mark_stale(&mut p, 0);
        assert_eq!(p.scenes[0].voice.status, AssetStatus::Done);

        p.scenes[0].spoken_text = "ពីរ".into();
        mark_stale(&mut p, 0);
        assert_eq!(p.scenes[0].voice.status, AssetStatus::Stale);
        assert_eq!(p.scenes[0].clip.status, AssetStatus::Stale);
        assert_eq!(p.scenes[0].image.status, AssetStatus::Pending);
    }
}
