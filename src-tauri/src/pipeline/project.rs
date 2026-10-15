//! `project.json` data model (see `docs/02-architecture.md`).

use serde::{Deserialize, Serialize};

use std::collections::BTreeMap;

use crate::images::provider::ProviderId;
use crate::tts::EngineKind;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Km,
    En,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    ScriptDraft,
    Generating,
    Review,
    Rendering,
    Done,
    /// Image job stopped: Cloudflare daily allowance used up (`image_pause` says until when).
    PausedQuota,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum AssetStatus {
    #[default]
    Pending,
    Running,
    Done,
    Failed,
    Stale,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Project {
    pub schema_version: u32,
    pub id: String,
    pub title: String,
    pub topic: String,
    pub language: Language,
    pub target_minutes: u32,
    pub style: Style,
    pub voice: VoiceSettings,
    pub status: ProjectStatus,
    pub created_at: String,
    pub updated_at: String,
    /// Script plan; chapters flip to `done` as their scenes arrive (resume point).
    #[serde(default)]
    pub outline: Option<Outline>,
    /// Look & feel shared by every image (`docs/07-image-generation.md` §2).
    #[serde(default)]
    pub visual_bible: Option<VisualBible>,
    /// Image settings snapshot used for the cache hash (07 §8) + project image options.
    #[serde(default)]
    pub image_settings: ImageSnapshot,
    /// Set while `status == PausedQuota`.
    #[serde(default)]
    pub image_pause: Option<ImagePause>,
    #[serde(default)]
    pub scenes: Vec<Scene>,
    #[serde(default)]
    pub render: RenderSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Style {
    pub preset: String,
    pub style_prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
/// Voice used for this project's scenes. Copied from Settings (resolved for the project
/// language) when voice generation starts; part of every scene's `text_hash`.
pub struct VoiceSettings {
    #[serde(default)]
    pub engine: EngineKind,
    /// Gemini TTS model (ignored by Edge).
    pub model: String,
    /// Edge voice ID (`km-KH-SreymomNeural`) or Gemini voice name (`Kore`).
    pub voice_name: String,
    /// Gemini delivery style (ignored by Edge).
    pub direction: String,
    /// Edge rate in % (−50…+50).
    #[serde(default)]
    pub rate: i32,
    /// Edge pitch in Hz (−50…+50).
    #[serde(default)]
    pub pitch: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Outline {
    pub title: String,
    pub hook: String,
    pub chapters: Vec<OutlineChapter>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OutlineChapter {
    pub title: String,
    pub summary: String,
    pub scene_count: u32,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Scene {
    pub id: String,
    pub order: u32,
    /// 0-based index into `outline.chapters`.
    #[serde(default)]
    pub chapter: u32,
    pub narration: String,
    pub spoken_text: String,
    pub image_prompt: String,
    pub mood: String,
    /// Claim the model was unsure about; shown to the user for checking.
    #[serde(default)]
    pub fact_note: Option<String>,
    /// Visual Bible character keys shown in this scene.
    #[serde(default)]
    pub characters: Vec<String>,
    /// Visual Bible place key.
    #[serde(default)]
    pub place: Option<String>,
    #[serde(default)]
    pub shot: Shot,
    /// Scene 1, each chapter's first scene and the last scene (generated first, may use the
    /// key-scene provider).
    #[serde(default)]
    pub key_scene: bool,
    #[serde(default)]
    pub voice: VoiceAsset,
    #[serde(default)]
    pub image: ImageAsset,
    #[serde(default)]
    pub subs: SubsAsset,
    #[serde(default)]
    pub clip: ClipAsset,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct VoiceAsset {
    pub status: AssetStatus,
    /// Relative to the project folder: `scenes/<id>/voice.mp3` (Edge) or `.wav` (Gemini).
    pub path: Option<String>,
    pub duration_ms: Option<u64>,
    pub error: Option<String>,
    /// Stable error code for the UI (`edge_unavailable`, `busy`, `suspicious_length`, …).
    #[serde(default)]
    pub error_kind: Option<String>,
    pub text_hash: Option<String>,
    /// `scenes/<id>/words.json` when the engine reported word/phrase timings (Edge).
    #[serde(default)]
    pub words_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ImageAsset {
    pub status: AssetStatus,
    /// Pan-size 16:9 image: `scenes/<id>/image_1080.png` (or `image_720.png`).
    pub path: Option<String>,
    /// Provider original: `scenes/<id>/image_raw.jpg|png`.
    #[serde(default)]
    pub raw_path: Option<String>,
    /// 384×216 `scenes/<id>/thumb.jpg` for the grid.
    #[serde(default)]
    pub thumb_path: Option<String>,
    /// Provider that made it (`cloudflare_flux` / `gemini_nano_banana`).
    #[serde(default)]
    pub provider: Option<String>,
    /// Seed override from "Regenerate"; `None` = bible seed + scene number.
    #[serde(default)]
    pub seed: Option<u32>,
    /// With "2 scenes per image": the scene whose image this one reuses.
    #[serde(default)]
    pub shared_from: Option<String>,
    pub error: Option<String>,
    #[serde(default)]
    pub error_kind: Option<String>,
    /// sha256(provider + model + final prompt + seed + steps).
    pub prompt_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct VisualBible {
    pub style: String,
    #[serde(default)]
    pub characters: BTreeMap<String, String>,
    #[serde(default)]
    pub places: BTreeMap<String, String>,
    #[serde(default)]
    pub culture_notes: String,
    pub seed: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Shot {
    Wide,
    #[default]
    Medium,
    Close,
    Detail,
    Aerial,
}

impl Shot {
    pub const ALL: [Shot; 5] = [
        Shot::Wide,
        Shot::Medium,
        Shot::Close,
        Shot::Detail,
        Shot::Aerial,
    ];

    pub fn parse(s: &str) -> Option<Shot> {
        match s.trim().to_lowercase().as_str() {
            "wide" => Some(Shot::Wide),
            "medium" => Some(Shot::Medium),
            "close" => Some(Shot::Close),
            "detail" => Some(Shot::Detail),
            "aerial" => Some(Shot::Aerial),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Shot::Wide => "wide",
            Shot::Medium => "medium",
            Shot::Close => "close",
            Shot::Detail => "detail",
            Shot::Aerial => "aerial",
        }
    }
}

/// Image settings the project's images were made with + project image options.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ImageSnapshot {
    pub provider: ProviderId,
    pub gemini_model: String,
    pub steps: u8,
    pub key_scene_provider: Option<ProviderId>,
    /// 1 (default) or 2: consecutive scene pairs share one image (07 §5).
    pub scenes_per_image: u8,
}

impl Default for ImageSnapshot {
    fn default() -> Self {
        Self {
            provider: ProviderId::CloudflareFlux,
            gemini_model: String::new(),
            steps: 4,
            key_scene_provider: None,
            scenes_per_image: 1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImagePause {
    /// RFC 3339, next 00:00 UTC.
    pub resets_at_utc: String,
    pub done: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SubsAsset {
    pub status: AssetStatus,
    #[serde(default)]
    pub lines: Vec<SubLine>,
    /// `hashing::subs_hash` the PNGs were rendered for.
    #[serde(default)]
    pub hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SubLine {
    /// One subtitle cue: up to 2 display lines joined with a line break.
    pub text: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ClipAsset {
    pub status: AssetStatus,
    /// `scenes/<id>/clip.mkv` (H.264 + PCM so clips join without audio gaps).
    pub path: Option<String>,
    /// `hashing::clip_hash` of the inputs the clip was rendered from.
    #[serde(default)]
    pub hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RenderSettings {
    pub resolution: String,
    pub music: Option<String>,
    pub music_volume: f32,
    pub encoder: Option<String>,
    pub status: String,
    pub output_path: Option<String>,
    /// Background colour (`#RRGGBB`) behind the word being spoken; `None` = no word tracker.
    #[serde(default = "default_highlight")]
    pub subtitle_highlight: Option<String>,
}

pub const DEFAULT_HIGHLIGHT: &str = "#FFD60A";

#[allow(clippy::unnecessary_wraps)]
fn default_highlight() -> Option<String> {
    Some(DEFAULT_HIGHLIGHT.into())
}

impl Default for RenderSettings {
    fn default() -> Self {
        Self {
            resolution: "1080p".into(),
            music: Some("default_calm.mp3".into()),
            music_volume: 0.12,
            encoder: None,
            status: "idle".into(),
            output_path: None,
            subtitle_highlight: default_highlight(),
        }
    }
}

/// Lightweight row for the projects list.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectSummary {
    pub id: String,
    pub title: String,
    pub topic: String,
    pub language: Language,
    pub target_minutes: u32,
    pub style_preset: String,
    pub status: ProjectStatus,
    pub scene_count: usize,
    pub updated_at: String,
}

impl From<&Project> for ProjectSummary {
    fn from(p: &Project) -> Self {
        Self {
            id: p.id.clone(),
            title: p.title.clone(),
            topic: p.topic.clone(),
            language: p.language,
            target_minutes: p.target_minutes,
            style_preset: p.style.preset.clone(),
            status: p.status,
            scene_count: p.scenes.len(),
            updated_at: p.updated_at.clone(),
        }
    }
}

impl Project {
    /// True when an outline exists and every chapter has its scenes.
    pub fn script_complete(&self) -> bool {
        self.outline
            .as_ref()
            .is_some_and(|o| o.chapters.iter().all(|c| c.done))
    }

    /// Next free scene id: `s001`, `s002`, … (never reuses a deleted id).
    pub fn next_scene_id(&self) -> String {
        self.next_scene_id_after(&[])
    }

    /// Next id also considering scenes not yet inserted into the project.
    pub fn next_scene_id_after(&self, pending: &[Scene]) -> String {
        let max = self
            .scenes
            .iter()
            .chain(pending)
            .filter_map(|s| s.id.strip_prefix('s').and_then(|n| n.parse::<u32>().ok()))
            .max()
            .unwrap_or(0);
        format!("s{:03}", max + 1)
    }

    /// Re-numbers `order` to 1..n after any list change.
    pub fn renumber(&mut self) {
        for (i, s) in self.scenes.iter_mut().enumerate() {
            s.order = u32::try_from(i + 1).unwrap_or(u32::MAX);
        }
    }

    /// generating → review once every scene has its voice and image.
    pub fn promote_to_review(&mut self) {
        let all_done = !self.scenes.is_empty()
            && self.scenes.iter().all(|s| {
                s.voice.status == AssetStatus::Done && s.image.status == AssetStatus::Done
            });
        if self.status == ProjectStatus::Generating && all_done {
            self.status = ProjectStatus::Review;
        }
    }

    /// On app start: anything left `running` by a crash goes back to `pending`.
    pub fn reset_running_assets(&mut self) -> bool {
        let mut changed = false;
        // A render interrupted by a crash: finished clips are kept, the project goes back to review.
        if self.status == ProjectStatus::Rendering {
            self.status = ProjectStatus::Review;
            changed = true;
        }
        for s in &mut self.scenes {
            for st in [
                &mut s.voice.status,
                &mut s.image.status,
                &mut s.subs.status,
                &mut s.clip.status,
            ] {
                if *st == AssetStatus::Running {
                    *st = AssetStatus::Pending;
                    changed = true;
                }
            }
        }
        changed
    }
}

impl Scene {
    pub fn new(id: String, chapter: u32) -> Self {
        Self {
            id,
            order: 0,
            chapter,
            narration: String::new(),
            spoken_text: String::new(),
            image_prompt: String::new(),
            mood: "calm".into(),
            fact_note: None,
            characters: vec![],
            place: None,
            shot: Shot::default(),
            key_scene: false,
            voice: VoiceAsset::default(),
            image: ImageAsset::default(),
            subs: SubsAsset::default(),
            clip: ClipAsset::default(),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn next_scene_id_skips_used_numbers() {
        let mut p = sample();
        assert_eq!(p.next_scene_id(), "s001");
        p.scenes.push(Scene::new("s001".into(), 0));
        p.scenes.push(Scene::new("s007".into(), 0));
        assert_eq!(p.next_scene_id(), "s008");
    }

    #[test]
    fn status_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&ProjectStatus::ScriptDraft).unwrap(),
            "\"script_draft\""
        );
        assert_eq!(serde_json::to_string(&Language::Km).unwrap(), "\"km\"");
    }

    pub(crate) fn sample() -> Project {
        Project {
            schema_version: SCHEMA_VERSION,
            id: "01J9ZQ3V4R8S6T2W5X7Y9A1B3C".into(),
            title: "t".into(),
            topic: "t".into(),
            language: Language::Km,
            target_minutes: 3,
            style: Style {
                preset: "storybook_watercolor".into(),
                style_prompt: "watercolor".into(),
            },
            voice: VoiceSettings {
                engine: EngineKind::Edge,
                model: "m".into(),
                voice_name: "v".into(),
                direction: "d".into(),
                rate: 0,
                pitch: 0,
            },
            status: ProjectStatus::ScriptDraft,
            created_at: String::new(),
            updated_at: String::new(),
            outline: None,
            visual_bible: None,
            image_settings: ImageSnapshot::default(),
            image_pause: None,
            scenes: vec![],
            render: RenderSettings::default(),
        }
    }
}
