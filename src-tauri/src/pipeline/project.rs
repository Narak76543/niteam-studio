//! `project.json` data model (see `docs/02-architecture.md`).

use serde::{Deserialize, Serialize};

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
pub struct VoiceSettings {
    pub model: String,
    pub voice_name: String,
    pub direction: String,
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
    pub path: Option<String>,
    pub duration_ms: Option<u64>,
    pub error: Option<String>,
    pub text_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ImageAsset {
    pub status: AssetStatus,
    pub path: Option<String>,
    pub error: Option<String>,
    pub prompt_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SubsAsset {
    pub status: AssetStatus,
    #[serde(default)]
    pub lines: Vec<SubLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SubLine {
    pub text: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ClipAsset {
    pub status: AssetStatus,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RenderSettings {
    pub resolution: String,
    pub music: Option<String>,
    pub music_volume: f32,
    pub encoder: Option<String>,
    pub status: String,
    pub output_path: Option<String>,
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

    /// On app start: anything left `running` by a crash goes back to `pending`.
    pub fn reset_running_assets(&mut self) -> bool {
        let mut changed = false;
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
                model: "m".into(),
                voice_name: "v".into(),
                direction: "d".into(),
            },
            status: ProjectStatus::ScriptDraft,
            created_at: String::new(),
            updated_at: String::new(),
            outline: None,
            scenes: vec![],
            render: RenderSettings::default(),
        }
    }
}
