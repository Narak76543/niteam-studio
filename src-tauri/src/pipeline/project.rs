//! `project.json` data model (see `docs/02-architecture.md`).

use serde::{Deserialize, Serialize};

use std::collections::BTreeMap;

use crate::images::provider::ProviderId;
use crate::stock::Candidate;
use crate::tts::EngineKind;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Km,
    En,
}

/// Output aspect ratio: `project.aspect`. Set at project creation; vertical projects
/// export at 1080×1920 (TikTok / Reels / Shorts), horizontal ones at 1920×1080.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum Aspect {
    #[default]
    #[serde(rename = "16:9")]
    Wide,
    #[serde(rename = "9:16")]
    Vertical,
}

impl Aspect {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Wide => "16:9",
            Self::Vertical => "9:16",
        }
    }
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
    /// Frame aspect: `"16:9"` (default) or `"9:16"`. Changing it marks images,
    /// subtitles and clips outdated (`editing::set_aspect`).
    #[serde(default)]
    pub aspect: Aspect,
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
    /// Names that must stay exactly as written (React Native, Android, …): script prompts,
    /// proofreading. Script page → Names.
    #[serde(default)]
    pub keep_terms: Vec<String>,
    /// VoxCPM2 Voice Profile for this project (`None` = the default in Settings → Voices).
    #[serde(default)]
    pub voice_profile: Option<String>,
    /// "From document" projects: the PDF's summary and reading state.
    #[serde(default)]
    pub source: Option<SourceDoc>,
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
    /// Pending grammar/phrase suggestion from the proofreader (Script page), if any.
    #[serde(default)]
    pub proofread: Option<ProofSuggestion>,
    /// `hashing::proof_hash` of the text the proofreader last checked (skip if unchanged).
    #[serde(default)]
    pub proofread_hash: Option<String>,
    #[serde(default)]
    pub clip: ClipAsset,
    /// "From document" projects: PDF pages the scene's claims come from.
    #[serde(default)]
    pub source_pages: Vec<u32>,
    /// Background explanation that is not a claim from the document.
    #[serde(default)]
    pub general_knowledge: bool,
    /// "Real-world Tech Explainer": what the scene shows (`docs/11-real-world-visuals.md`).
    /// Older projects load as `ai_image`, which keeps using `image_prompt` + `image`.
    #[serde(default)]
    pub visual: SceneVisual,
}

/// What a scene shows.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum VisualKind {
    /// The image providers (`image_prompt`); the fallback.
    #[default]
    AiImage,
    RealPhoto,
    StockVideo,
    Diagram,
    Code,
    /// The user's own screenshot or photo.
    Screenshot,
}

/// A Mermaid diagram (or one of our own templates, `template: "comparison"`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DiagramSpec {
    /// flow | sequence | timeline | architecture | comparison
    pub template: String,
    pub mermaid: String,
}

/// Code or terminal output shown as an image.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CodeSpec {
    pub language: String,
    pub text: String,
    /// Terminal look (prompt, output) instead of an editor.
    #[serde(default)]
    pub terminal: bool,
    /// Lines appear one by one over the scene.
    #[serde(default)]
    pub type_out: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SceneVisual {
    pub kind: VisualKind,
    /// ENGLISH search keywords (also for Khmer scripts).
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub diagram: Option<DiagramSpec>,
    #[serde(default)]
    pub code: Option<CodeSpec>,
    /// Last search results for this scene (picker).
    #[serde(default)]
    pub candidates: Vec<Candidate>,
    /// Chosen asset id (`stock::assets::AssetRecord::id`).
    #[serde(default)]
    pub asset: Option<String>,
    /// Subject centre, 0..1 of the image, for the 16:9 crop. `None` = centre.
    #[serde(default)]
    pub focus: Option<[f32; 2]>,
    /// Chosen automatically (ranking or Gemini); shown with "check" until the user confirms.
    #[serde(default)]
    pub auto_picked: bool,
    #[serde(default)]
    pub status: AssetStatus,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub error_kind: Option<String>,
    /// Final still (diagram, code): `scenes/<id>/visual.png`.
    #[serde(default)]
    pub render_path: Option<String>,
    /// 384×216 grid thumbnail of the visual: `scenes/<id>/visual_thumb.jpg`.
    #[serde(default)]
    pub thumb_path: Option<String>,
    /// Hash of what `render_path` was drawn from (diagram / code text); stale when it differs.
    #[serde(default)]
    pub render_hash: Option<String>,
    /// Typed-out code: one picture per revealed line (`scenes/<id>/code/000.png`…), in order.
    #[serde(default)]
    pub frames: Vec<String>,
}

impl SceneVisual {
    /// Uses a downloaded stock file or the user's own file.
    pub fn uses_asset(&self) -> bool {
        matches!(self.kind, VisualKind::RealPhoto | VisualKind::StockVideo | VisualKind::Screenshot)
    }
}

impl Scene {
    /// Needs a picture from the image providers: AI-image scenes, and real-world scenes whose
    /// photo/clip search found nothing usable (fallback).
    pub fn needs_ai_image(&self) -> bool {
        let v = &self.visual;
        match v.kind {
            VisualKind::AiImage => true,
            VisualKind::RealPhoto | VisualKind::StockVideo => v.asset.is_none() && v.status == AssetStatus::Failed,
            VisualKind::Diagram | VisualKind::Code | VisualKind::Screenshot => false,
        }
    }
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
    /// Local model profile (`local_models.json`) when a provider is `local_sdcpp`.
    #[serde(default)]
    pub local_profile: Option<String>,
}

impl Default for ImageSnapshot {
    fn default() -> Self {
        Self {
            provider: ProviderId::CloudflareFlux,
            gemini_model: String::new(),
            steps: 4,
            key_scene_provider: None,
            scenes_per_image: 1,
            local_profile: None,
        }
    }
}

/// A proofreader's correction for one scene, waiting for Accept / Dismiss.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProofSuggestion {
    pub narration: String,
    pub spoken_text: String,
    /// Short explanation of what changed (project language).
    pub note: String,
}

/// The document a "From document" project is made from (`docs/10-document-source.md` §2).
/// The PDF itself is `source/document.pdf` in the project folder.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceDoc {
    /// Original file name (UI, credit line).
    pub file_name: String,
    pub size_bytes: u64,
    /// Pages of the whole file.
    pub page_count: u32,
    pub sha256: String,
    /// Pages read, 1-based and inclusive (at most `source::pdf::MAX_PAGES`).
    pub page_from: u32,
    pub page_to: u32,
    pub audience: Audience,
    /// Optional, e.g. "explain only the results section".
    #[serde(default)]
    pub focus: String,
    pub stage: SourceStage,
    /// One Gemini call each; `done` chunks are skipped on resume.
    pub chunks: Vec<SourceChunk>,
    #[serde(default)]
    pub summary: Option<SourceSummary>,
    /// Set while the reading job waits for the free-tier quota.
    #[serde(default)]
    pub pause: Option<QuotaPause>,
    /// "Source: …" line for the video description (set on confirm).
    #[serde(default)]
    pub credit: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Audience {
    #[default]
    General,
    Students,
    Experts,
}

impl Audience {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::General => "a general audience (no prior knowledge)",
            Self::Students => "students (some background, explain the key terms)",
            Self::Experts => "experts in the field (precise, technical)",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceStage {
    /// Reading job not finished (or paused / failed: run it again to resume).
    Reading,
    /// Summary ready; the user picks the points. Nothing else is generated yet.
    Review,
    /// Points confirmed; the script is written from them.
    Confirmed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceChunk {
    pub from: u32,
    pub to: u32,
    /// How the chunk was sent (set when it is read).
    #[serde(default)]
    pub mode: Option<ChunkMode>,
    #[serde(default)]
    pub sections: Vec<SourceSection>,
    /// Title / authors / year if these pages show them ("" otherwise).
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub authors: String,
    #[serde(default)]
    pub year: String,
    #[serde(default)]
    pub done: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ChunkMode {
    /// The locally extracted text (saves quota).
    Text,
    /// The PDF pages (scanned or garbled text).
    Pdf,
}

/// Notes on one section of the document, in English, with the pages they come from.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceSection {
    pub heading: String,
    pub notes: String,
    pub pages: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SourceSummary {
    pub title: String,
    #[serde(default)]
    pub authors: String,
    #[serde(default)]
    pub year: String,
    /// In the order the user wants them in the video.
    pub key_points: Vec<SourceItem>,
    #[serde(default)]
    pub facts: Vec<SourceItem>,
    #[serde(default)]
    pub terms: Vec<SourceItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceItem {
    pub id: String,
    /// Key terms only: the term as the document writes it.
    #[serde(default)]
    pub term: String,
    /// The point, fact or term explanation, in the project language.
    pub text: String,
    pub pages: Vec<u32>,
    #[serde(default = "yes")]
    pub selected: bool,
    /// The user's note for the script writer.
    #[serde(default)]
    pub note: String,
}

fn yes() -> bool {
    true
}

/// Free-tier quota wait (`docs/10-document-source.md` §5).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QuotaPause {
    /// RFC 3339 UTC: when trying again makes sense.
    pub resets_at_utc: String,
    /// Daily quota (midnight Pacific) rather than the per-minute rate limit.
    pub daily: bool,
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
    pub aspect: Aspect,
    pub style_preset: String,
    pub status: ProjectStatus,
    pub scene_count: usize,
    pub updated_at: String,
    /// First scene with a thumbnail (Projects page card), and a key that changes with it.
    pub cover_scene: Option<String>,
    pub cover_version: Option<String>,
}

impl From<&Project> for ProjectSummary {
    fn from(p: &Project) -> Self {
        let cover = p.scenes.iter().find(|s| s.image.thumb_path.is_some());
        Self {
            id: p.id.clone(),
            title: p.title.clone(),
            topic: p.topic.clone(),
            language: p.language,
            target_minutes: p.target_minutes,
            aspect: p.aspect,
            style_preset: p.style.preset.clone(),
            status: p.status,
            scene_count: p.scenes.len(),
            updated_at: p.updated_at.clone(),
            cover_scene: cover.map(|s| s.id.clone()),
            cover_version: cover.map(|s| format!("{}#{}", s.image.thumb_path.as_deref().unwrap_or(""), s.image.prompt_hash.as_deref().unwrap_or(""))),
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
            proofread: None,
            proofread_hash: None,
            clip: ClipAsset::default(),
            source_pages: vec![],
            general_knowledge: false,
            visual: SceneVisual::default(),
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
            aspect: Aspect::Wide,
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
            keep_terms: Vec::new(),
            voice_profile: None,
            source: None,
            scenes: vec![],
            render: RenderSettings::default(),
        }
    }
}
