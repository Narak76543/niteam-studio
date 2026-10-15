//! Project creation and script-editing operations (pure functions on `Project`).

use serde::Deserialize;

use super::ProjectError;
use super::hashing::{self, normalize, normalize_spoken};
use super::presets;
use super::project::{
    ImageSnapshot, Language, Project, ProjectStatus, RenderSettings, SCHEMA_VERSION, Scene, Shot,
    Style, VisualBible, VoiceSettings,
};
use super::store::now;
use super::visual_bible;
use crate::gemini::script::MOODS;
use crate::tts::EngineKind;

pub const MIN_MINUTES: u32 = 1;
pub const MAX_MINUTES: u32 = 20;
const MAX_TOPIC_CHARS: usize = 500;
const MAX_FIELD_CHARS: usize = 4_000;
pub const DEFAULT_VOICE_DIRECTION: &str = "calm, warm storyteller, medium pace";

#[derive(Debug, Clone, Deserialize)]
pub struct NewProjectInput {
    pub topic: String,
    pub language: Language,
    pub target_minutes: u32,
    pub style_preset: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SceneEdit {
    pub id: String,
    pub narration: String,
    pub spoken_text: String,
    pub image_prompt: String,
    pub mood: String,
    /// Image fields (07 §2); `None` keeps the current values.
    #[serde(default)]
    pub visual: Option<SceneVisualEdit>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SceneVisualEdit {
    pub characters: Vec<String>,
    pub place: Option<String>,
    pub shot: Shot,
}

fn invalid(msg: &str) -> ProjectError {
    ProjectError::Invalid(msg.to_owned())
}

pub fn new_project(input: &NewProjectInput, tts_model: &str) -> Result<Project, ProjectError> {
    let topic = normalize(&input.topic);
    if topic.is_empty() {
        return Err(invalid("Please enter a topic."));
    }
    if topic.chars().count() > MAX_TOPIC_CHARS {
        return Err(invalid("The topic is too long (max 500 characters)."));
    }
    if !(MIN_MINUTES..=MAX_MINUTES).contains(&input.target_minutes) {
        return Err(invalid("Length must be between 1 and 20 minutes."));
    }
    let preset =
        presets::find(&input.style_preset).ok_or_else(|| invalid("Unknown style preset."))?;
    let ts = now();
    Ok(Project {
        schema_version: SCHEMA_VERSION,
        id: ulid::Ulid::new().to_string(),
        title: topic.clone(),
        topic,
        language: input.language,
        target_minutes: input.target_minutes,
        style: Style {
            preset: preset.id.to_owned(),
            style_prompt: preset.style_prompt.to_owned(),
        },
        // Replaced by the Settings voice (resolved for the language) when voices are generated.
        voice: VoiceSettings {
            engine: EngineKind::default(),
            model: tts_model.to_owned(),
            voice_name: String::new(),
            direction: DEFAULT_VOICE_DIRECTION.to_owned(),
            rate: 0,
            pitch: 0,
        },
        status: ProjectStatus::ScriptDraft,
        created_at: ts.clone(),
        updated_at: ts,
        outline: None,
        visual_bible: None,
        image_settings: ImageSnapshot::default(),
        image_pause: None,
        keep_terms: Vec::new(),
        voice_profile: None,
        scenes: vec![],
        render: RenderSettings::default(),
    })
}

fn require_draft(p: &Project) -> Result<(), ProjectError> {
    if p.status == ProjectStatus::ScriptDraft {
        Ok(())
    } else {
        Err(ProjectError::WrongState(status_name(p.status).into()))
    }
}

pub fn status_name(s: ProjectStatus) -> &'static str {
    match s {
        ProjectStatus::ScriptDraft => "script_draft",
        ProjectStatus::Generating => "generating",
        ProjectStatus::Review => "review",
        ProjectStatus::Rendering => "rendering",
        ProjectStatus::Done => "done",
        ProjectStatus::PausedQuota => "paused_quota",
    }
}

fn scene_index(p: &Project, id: &str) -> Result<usize, ProjectError> {
    p.scenes
        .iter()
        .position(|s| s.id == id)
        .ok_or(ProjectError::SceneNotFound)
}

fn check_len(text: &str) -> Result<(), ProjectError> {
    if text.chars().count() > MAX_FIELD_CHARS {
        Err(invalid("Text is too long (max 4000 characters)."))
    } else {
        Ok(())
    }
}

/// Applies a user edit, normalizes text and marks changed assets stale.
pub fn apply_scene_edit(p: &mut Project, edit: &SceneEdit) -> Result<(), ProjectError> {
    require_draft(p)?;
    let idx = scene_index(p, &edit.id)?;
    for t in [&edit.narration, &edit.spoken_text, &edit.image_prompt] {
        check_len(t)?;
    }
    let mood = if MOODS.contains(&edit.mood.as_str()) {
        edit.mood.clone()
    } else {
        "calm".to_owned()
    };
    if let Some(scene) = p.scenes.get_mut(idx) {
        let narration = normalize(&edit.narration);
        let spoken = normalize_spoken(&edit.spoken_text);
        if narration != scene.narration || spoken != scene.spoken_text {
            // A proofreading suggestion was for the old text.
            scene.proofread = None;
        }
        scene.narration = narration;
        scene.spoken_text = spoken;
        scene.image_prompt = normalize(&edit.image_prompt);
        scene.mood = mood;
    }
    if let Some(v) = &edit.visual {
        apply_visual(p, idx, v);
    }
    hashing::mark_stale(p, idx);
    Ok(())
}

/// Inserts an empty scene after `after` (or at the end). Returns the new id.
pub fn add_scene(p: &mut Project, after: Option<&str>) -> Result<String, ProjectError> {
    require_draft(p)?;
    let id = p.next_scene_id();
    let (pos, chapter) = match after {
        Some(a) => {
            let i = scene_index(p, a)?;
            (i + 1, p.scenes.get(i).map_or(0, |s| s.chapter))
        }
        None => (p.scenes.len(), p.scenes.last().map_or(0, |s| s.chapter)),
    };
    p.scenes.insert(pos, Scene::new(id.clone(), chapter));
    p.renumber();
    visual_bible::apply_key_scenes(p);
    Ok(id)
}

pub fn delete_scene(p: &mut Project, id: &str) -> Result<(), ProjectError> {
    require_draft(p)?;
    let idx = scene_index(p, id)?;
    p.scenes.remove(idx);
    p.renumber();
    visual_bible::apply_key_scenes(p);
    Ok(())
}

/// `ids` must be a permutation of the current scene ids.
pub fn reorder(p: &mut Project, ids: &[String]) -> Result<(), ProjectError> {
    require_draft(p)?;
    if ids.len() != p.scenes.len() {
        return Err(invalid("Scene list is out of date. Reload and try again."));
    }
    let mut reordered = Vec::with_capacity(ids.len());
    for id in ids {
        let idx = p
            .scenes
            .iter()
            .position(|s| &s.id == id)
            .ok_or_else(|| invalid("Scene list is out of date. Reload and try again."))?;
        reordered.push(p.scenes.swap_remove(idx));
    }
    p.scenes = reordered;
    p.renumber();
    visual_bible::apply_key_scenes(p);
    Ok(())
}

fn apply_visual(p: &mut Project, idx: usize, v: &SceneVisualEdit) {
    let bible = p.visual_bible.clone().unwrap_or_default();
    if let Some(scene) = p.scenes.get_mut(idx) {
        scene.characters = v
            .characters
            .iter()
            .filter(|k| bible.characters.contains_key(*k))
            .cloned()
            .collect();
        scene.place = v.place.clone().filter(|k| bible.places.contains_key(k));
        scene.shot = v.shot;
    }
}

fn allows_image_edits(p: &Project) -> Result<(), ProjectError> {
    if p.status == ProjectStatus::Rendering {
        Err(ProjectError::WrongState("rendering".into()))
    } else {
        Ok(())
    }
}

/// Edits one image prompt (also after approval, from the Scenes page). The image becomes
/// stale if its final prompt changed.
pub fn update_image_prompt(
    p: &mut Project,
    scene_id: &str,
    image_prompt: &str,
) -> Result<(), ProjectError> {
    allows_image_edits(p)?;
    check_len(image_prompt)?;
    let idx = scene_index(p, scene_id)?;
    if let Some(s) = p.scenes.get_mut(idx) {
        s.image_prompt = normalize(image_prompt);
    }
    hashing::mark_all_images_stale(p);
    Ok(())
}

/// Replaces the Visual Bible (Look & feel tab). Keys are normalized, scene references to
/// removed keys are dropped, and images whose prompt changed become stale.
pub fn update_bible(p: &mut Project, bible: &VisualBible) -> Result<(), ProjectError> {
    allows_image_edits(p)?;
    if bible.style.trim().is_empty() {
        return Err(invalid("The style must not be empty."));
    }
    let clean = |m: &std::collections::BTreeMap<String, String>| {
        m.iter()
            .map(|(k, d)| (visual_bible::normalize_key(k), normalize(d)))
            .filter(|(k, d)| !k.is_empty() && !d.is_empty())
            .collect()
    };
    p.visual_bible = Some(VisualBible {
        style: normalize(&bible.style),
        characters: clean(&bible.characters),
        places: clean(&bible.places),
        culture_notes: normalize(&bible.culture_notes),
        seed: bible.seed,
    });
    visual_bible::prune_scene_keys(p);
    hashing::mark_all_images_stale(p);
    Ok(())
}

/// Switches the art style preset (Look & feel). The Visual Bible is rewritten for it next
/// (`visual_bible_generate`), which marks the images whose prompt changed as outdated.
pub fn set_style(p: &mut Project, preset_id: &str) -> Result<(), ProjectError> {
    allows_image_edits(p)?;
    let preset = presets::find(preset_id).ok_or_else(|| invalid("Unknown style preset."))?;
    p.style = Style {
        preset: preset.id.to_owned(),
        style_prompt: preset.style_prompt.to_owned(),
    };
    Ok(())
}

/// 07 §5 "Scenes per image": 1 or 2.
pub fn set_scenes_per_image(p: &mut Project, n: u8) -> Result<(), ProjectError> {
    allows_image_edits(p)?;
    if !(1..=2).contains(&n) {
        return Err(invalid("Scenes per image must be 1 or 2."));
    }
    p.image_settings.scenes_per_image = n;
    hashing::mark_all_images_stale(p);
    Ok(())
}

/// script_draft → generating. Every scene must be complete.
pub fn approve(p: &mut Project) -> Result<(), ProjectError> {
    require_draft(p)?;
    if p.scenes.is_empty() {
        return Err(invalid("The script has no scenes yet."));
    }
    if p.outline.is_some() && !p.script_complete() {
        return Err(invalid("Script generation has not finished yet."));
    }
    if let Some(s) = p
        .scenes
        .iter()
        .find(|s| s.narration.is_empty() || s.spoken_text.is_empty() || s.image_prompt.is_empty())
    {
        return Err(ProjectError::Invalid(format!(
            "Scene {} is missing narration, spoken text or image prompt.",
            s.order
        )));
    }
    p.status = ProjectStatus::Generating;
    Ok(())
}

/// "Edit script": back to script_draft from any later step except rendering.
pub fn reopen(p: &mut Project) -> Result<(), ProjectError> {
    match p.status {
        ProjectStatus::Generating
        | ProjectStatus::Review
        | ProjectStatus::Done
        | ProjectStatus::PausedQuota => {
            p.status = ProjectStatus::ScriptDraft;
            Ok(())
        }
        ProjectStatus::ScriptDraft => Ok(()),
        ProjectStatus::Rendering => Err(ProjectError::WrongState("rendering".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::project::tests::sample;

    fn input() -> NewProjectInput {
        NewProjectInput {
            topic: "  តើស្រូវកើតមកពីណា?  ".into(),
            language: Language::Km,
            target_minutes: 3,
            style_preset: "storybook_watercolor".into(),
        }
    }

    fn with_scenes(n: usize) -> Project {
        let mut p = sample();
        for _ in 0..n {
            let id = p.next_scene_id();
            let mut s = Scene::new(id, 0);
            s.narration = "n".into();
            s.spoken_text = "s".into();
            s.image_prompt = "i".into();
            p.scenes.push(s);
        }
        p.renumber();
        p
    }

    #[test]
    fn new_project_validates_and_trims() {
        let p = new_project(&input(), "tts").unwrap();
        assert_eq!(p.topic, "តើស្រូវកើតមកពីណា?");
        assert_eq!(p.id.len(), 26);
        let mut bad = input();
        bad.target_minutes = 21;
        assert!(new_project(&bad, "tts").is_err());
        let mut bad = input();
        bad.style_preset = "nope".into();
        assert!(new_project(&bad, "tts").is_err());
    }

    #[test]
    fn add_delete_reorder() {
        let mut p = with_scenes(3);
        let id = add_scene(&mut p, Some("s001")).unwrap();
        assert_eq!(id, "s004");
        assert_eq!(p.scenes[1].id, "s004");
        assert_eq!(p.scenes[1].order, 2);
        delete_scene(&mut p, "s002").unwrap();
        let ids: Vec<String> = ["s003", "s004", "s001"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        reorder(&mut p, &ids).unwrap();
        assert_eq!(
            p.scenes.iter().map(|s| s.order).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert_eq!(p.scenes[0].id, "s003");
        assert!(reorder(&mut p, &ids[..2]).is_err());
    }

    #[test]
    fn approve_requires_complete_scenes_and_locks_editing() {
        let mut p = with_scenes(2);
        p.scenes[1].image_prompt.clear();
        assert!(approve(&mut p).is_err());
        p.scenes[1].image_prompt = "x".into();
        approve(&mut p).unwrap();
        assert_eq!(p.status, ProjectStatus::Generating);
        assert!(delete_scene(&mut p, "s001").is_err());
        reopen(&mut p).unwrap();
        assert_eq!(p.status, ProjectStatus::ScriptDraft);
    }

    #[test]
    fn bible_edit_normalizes_prunes_and_marks_images_stale() {
        let mut p = with_scenes(2);
        p.visual_bible = Some(VisualBible {
            style: "watercolor".into(),
            characters: [("farmer".to_owned(), "old".to_owned())].into(),
            places: Default::default(),
            culture_notes: String::new(),
            seed: 1,
        });
        p.scenes[0].characters = vec!["farmer".into()];
        let hash = hashing::prompt_hash(&p, 0).unwrap();
        p.scenes[0].image.status = crate::pipeline::project::AssetStatus::Done;
        p.scenes[0].image.prompt_hash = Some(hash);
        let new = VisualBible {
            style: " flat 2D ".into(),
            characters: [("Old Monk".to_owned(), "saffron robe".to_owned())].into(),
            places: Default::default(),
            culture_notes: String::new(),
            seed: 1,
        };
        update_bible(&mut p, &new).unwrap();
        let b = p.visual_bible.as_ref().unwrap();
        assert_eq!(b.style, "flat 2D");
        assert!(b.characters.contains_key("old_monk"));
        assert!(p.scenes[0].characters.is_empty(), "removed key pruned");
        assert_eq!(
            p.scenes[0].image.status,
            crate::pipeline::project::AssetStatus::Stale
        );
        assert!(set_scenes_per_image(&mut p, 3).is_err());
    }

    #[test]
    fn key_scenes_follow_structure_changes() {
        let mut p = with_scenes(3);
        add_scene(&mut p, None).unwrap();
        let keys: Vec<bool> = p.scenes.iter().map(|s| s.key_scene).collect();
        assert_eq!(keys, vec![true, false, false, true]);
    }

    #[test]
    fn edit_normalizes_and_rejects_unknown_mood() {
        let mut p = with_scenes(1);
        apply_scene_edit(
            &mut p,
            &SceneEdit {
                id: "s001".into(),
                narration: " ស្រូវ ".into(),
                spoken_text: "ស្រូវ\u{200B}".into(),
                image_prompt: "rice".into(),
                mood: "weird".into(),
                visual: None,
            },
        )
        .unwrap();
        assert_eq!(p.scenes[0].narration, "ស្រូវ");
        assert_eq!(p.scenes[0].spoken_text, "ស្រូវ");
        assert_eq!(p.scenes[0].mood, "calm");
    }

    #[test]
    fn set_style_switches_preset_but_not_while_rendering() {
        let mut p = sample();
        set_style(&mut p, "stickman").unwrap();
        assert_eq!(p.style.preset, "stickman");
        assert!(p.style.style_prompt.contains("stick figure"));
        assert!(set_style(&mut p, "nope").is_err());
        p.status = ProjectStatus::Rendering;
        assert!(set_style(&mut p, "flat_2d").is_err());
    }
}
