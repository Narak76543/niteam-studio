//! Visual Bible (`docs/07-image-generation.md` §2): one style / character / place sheet per
//! project, written by the text model right after the outline (before the chapter calls, so
//! scenes can refer to its keys), and the scene rules that go with it.

use std::collections::BTreeMap;

use rand::Rng;
use serde::Deserialize;
use serde_json::{Value, json};

use super::project::{Project, Shot, VisualBible};
use crate::gemini::client::ModelChain;
use crate::gemini::retry::RetryHooks;
use crate::gemini::script::{self, Validate};
use crate::gemini::{GeminiClient, GeminiError};
use crate::storage::ApiKey;

const MAX_ENTRIES: usize = 8;
const MAX_SEED: u32 = 99_999;

#[derive(Debug, Deserialize)]
pub struct BibleReply {
    pub style: String,
    #[serde(default)]
    pub characters: Vec<Entry>,
    #[serde(default)]
    pub places: Vec<Entry>,
    #[serde(default)]
    pub culture_notes: String,
}

#[derive(Debug, Deserialize)]
pub struct Entry {
    pub key: String,
    pub description: String,
}

/// `Old Monk` → `old_monk`.
pub fn normalize_key(key: &str) -> String {
    let mut out = String::new();
    for c in key.trim().to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    out.trim_end_matches('_').to_owned()
}

impl Validate for BibleReply {
    fn validate(&self) -> Result<(), String> {
        if self.style.trim().is_empty() {
            return Err("style must not be empty".into());
        }
        if self.characters.len() > MAX_ENTRIES || self.places.len() > MAX_ENTRIES {
            return Err(format!(
                "at most {MAX_ENTRIES} characters and {MAX_ENTRIES} places"
            ));
        }
        for e in self.characters.iter().chain(&self.places) {
            if normalize_key(&e.key).is_empty() || e.description.trim().is_empty() {
                return Err("every entry needs an English key and a description".into());
            }
        }
        Ok(())
    }
}

fn schema() -> Value {
    let entry = json!({
        "type": "object",
        "properties": {
            "key": { "type": "string", "description": "short snake_case English key, e.g. farmer" },
            "description": { "type": "string" }
        },
        "required": ["key", "description"]
    });
    json!({
        "type": "object",
        "properties": {
            "style": { "type": "string" },
            "characters": { "type": "array", "maxItems": MAX_ENTRIES, "items": entry },
            "places": { "type": "array", "maxItems": MAX_ENTRIES, "items": entry },
            "culture_notes": { "type": "string" }
        },
        "required": ["style", "characters", "places", "culture_notes"]
    })
}

/// Extra rules so characters and places fit the chosen style.
fn style_rules(style_prompt: &str) -> &'static str {
    if super::presets::is_stick_figure(style_prompt) {
        "STICK-FIGURE STYLE: every person is a simple stick figure (round head, dot eyes, line body). \
Describe characters ONLY by one or two simple, bold identifying props or colour accents that a stick figure \
can carry (e.g. `stick figure with a red-and-white checked krama scarf and a straw hat`, `stick figure \
with a small blue laptop`). Never describe skin, faces, hair texture, body build or realistic clothing \
detail. Describe places as simple line drawings with a few key shapes and at most two flat colour accents \
(e.g. `simple line drawing of a temple with three towers, light orange accent`). Keep `style` a \
stick-figure doodle style.\n"
    } else {
        ""
    }
}

fn prompt(project: &Project) -> String {
    let outline = project.outline.as_ref().map_or(String::new(), |o| {
        o.chapters
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{}. {} — {}\n", i + 1, c.title, c.summary))
            .collect()
    });
    let hook = project.outline.as_ref().map_or("", |o| o.hook.as_str());
    format!(
        "Create the VISUAL BIBLE for an illustrated explainer video, so every image looks consistent.\n\
TOPIC: {topic}\n\
TITLE: {title}\n\
HOOK: {hook}\n\
OUTLINE:\n{outline}\n\
CHOSEN ART STYLE: {style}\n\
{style_rules}\n\
Write everything in ENGLISH, with positive phrasing only (describe what is there, never what is absent).\n\
- `style`: one line expanding the chosen art style (medium, light, palette, texture). Keep its medium.\n\
- `characters`: every person or animal that appears in more than one scene, with a short snake_case key \
(e.g. `farmer`) and a precise visual description (age, build, skin, hair, clothing with colours) so it can be \
drawn the same way every time. Generic people only — no real living people; historical figures only as \
statues, carvings or unnamed generic figures. Use [] if the story has no recurring characters.\n\
- `places`: every recurring location, with a snake_case key and a visual description (architecture, \
landscape, plants, era).\n\
- `culture_notes`: if the topic is Cambodian / Khmer, concrete visual details that make images look \
authentically Cambodian (clothing, architecture, roof shapes, carvings, landscape); otherwise \"\".\n\
Never ask for text, letters or signs in images.",
        topic = project.topic,
        title = project.title,
        style = project.style.style_prompt,
        style_rules = style_rules(&project.style.style_prompt),
    )
}

pub fn from_reply(reply: BibleReply, seed: u32) -> VisualBible {
    let to_map = |entries: Vec<Entry>| -> BTreeMap<String, String> {
        entries
            .into_iter()
            .filter_map(|e| {
                let key = normalize_key(&e.key);
                let desc = e.description.trim().to_owned();
                (!key.is_empty() && !desc.is_empty()).then_some((key, desc))
            })
            .collect()
    };
    VisualBible {
        style: reply.style.trim().to_owned(),
        characters: to_map(reply.characters),
        places: to_map(reply.places),
        culture_notes: reply.culture_notes.trim().to_owned(),
        seed,
    }
}

/// Asks the text model for the bible. Returns it and the model that wrote it.
pub async fn generate(
    client: &GeminiClient,
    key: &ApiKey,
    chain: &ModelChain,
    project: &Project,
    hooks: &RetryHooks,
) -> Result<(VisualBible, String), GeminiError> {
    let (reply, model): (BibleReply, String) =
        script::request_json(client, key, chain, prompt(project), schema(), hooks).await?;
    let seed = rand::thread_rng().gen_range(1..=MAX_SEED);
    Ok((from_reply(reply, seed), model))
}

/// Key scenes (07 §2): the first scene, each chapter's first scene and the last scene.
pub fn apply_key_scenes(project: &mut Project) {
    let last = project.scenes.len().saturating_sub(1);
    let mut prev_chapter = None;
    for (i, s) in project.scenes.iter_mut().enumerate() {
        let chapter_start = prev_chapter != Some(s.chapter);
        s.key_scene = i == 0 || chapter_start || i == last;
        prev_chapter = Some(s.chapter);
    }
}

/// No more than 2 consecutive scenes with the same shot: the 3rd of a run is changed.
pub fn vary_shots(project: &mut Project) {
    for i in 2..project.scenes.len() {
        let (a, b) = (project.scenes[i - 2].shot, project.scenes[i - 1].shot);
        if let Some(s) = project.scenes.get_mut(i)
            && s.shot == a
            && s.shot == b
        {
            let next = Shot::ALL
                .iter()
                .copied()
                .find(|x| *x != a)
                .unwrap_or(Shot::Medium);
            s.shot = next;
        }
    }
}

/// Drops scene references to bible keys that don't exist (after a bible edit).
pub fn prune_scene_keys(project: &mut Project) {
    let Some(bible) = project.visual_bible.clone() else {
        return;
    };
    for s in &mut project.scenes {
        s.characters.retain(|k| bible.characters.contains_key(k));
        if s.place
            .as_ref()
            .is_some_and(|p| !bible.places.contains_key(p))
        {
            s.place = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::project::Scene;
    use crate::pipeline::project::tests::sample;

    #[test]
    fn keys_are_normalized() {
        assert_eq!(normalize_key("Old Monk"), "old_monk");
        assert_eq!(normalize_key("  farmer "), "farmer");
        assert_eq!(normalize_key("king-jayavarman VII"), "king_jayavarman_vii");
        assert_eq!(normalize_key("ស្រូវ"), "");
    }

    #[test]
    fn reply_becomes_bible() {
        let reply: BibleReply = serde_json::from_value(json!({
            "style": "warm watercolor",
            "characters": [{"key": "Farmer", "description": "old farmer, krama"}],
            "places": [{"key": "rice paddy", "description": "flooded green paddies"}],
            "culture_notes": "Khmer roofs"
        }))
        .unwrap();
        assert!(reply.validate().is_ok());
        let b = from_reply(reply, 42);
        assert_eq!(
            b.characters.get("farmer").map(String::as_str),
            Some("old farmer, krama")
        );
        assert!(b.places.contains_key("rice_paddy"));
        assert_eq!(b.seed, 42);
    }

    fn scenes(chapters: &[u32], shots: &[Shot]) -> Project {
        let mut p = sample();
        for (i, (c, sh)) in chapters.iter().zip(shots).enumerate() {
            let mut s = Scene::new(format!("s{:03}", i + 1), *c);
            s.shot = *sh;
            p.scenes.push(s);
        }
        p
    }

    #[test]
    fn key_scenes_are_first_chapter_starts_and_last() {
        let mut p = scenes(&[0, 0, 0, 1, 1, 2, 2], &[Shot::Medium; 7]);
        apply_key_scenes(&mut p);
        let keys: Vec<bool> = p.scenes.iter().map(|s| s.key_scene).collect();
        assert_eq!(keys, vec![true, false, false, true, false, true, true]);
    }

    #[test]
    fn shots_never_repeat_three_times() {
        let mut p = scenes(
            &[0; 6],
            &[
                Shot::Wide,
                Shot::Wide,
                Shot::Wide,
                Shot::Wide,
                Shot::Close,
                Shot::Close,
            ],
        );
        vary_shots(&mut p);
        let shots: Vec<Shot> = p.scenes.iter().map(|s| s.shot).collect();
        for w in shots.windows(3) {
            assert!(!(w[0] == w[1] && w[1] == w[2]), "{shots:?}");
        }
        assert_eq!(shots[0], Shot::Wide);
    }

    #[test]
    fn pruning_removes_unknown_keys() {
        let mut p = scenes(&[0], &[Shot::Wide]);
        p.visual_bible = Some(VisualBible {
            style: "s".into(),
            characters: BTreeMap::from([("farmer".to_owned(), "d".to_owned())]),
            places: BTreeMap::new(),
            culture_notes: String::new(),
            seed: 1,
        });
        p.scenes[0].characters = vec!["farmer".into(), "ghost".into()];
        p.scenes[0].place = Some("nowhere".into());
        prune_scene_keys(&mut p);
        assert_eq!(p.scenes[0].characters, vec!["farmer".to_owned()]);
        assert_eq!(p.scenes[0].place, None);
    }
}
