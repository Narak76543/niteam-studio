//! Rewrites the `image_prompt` of existing scenes with the current rules
//! (`script::image_prompt_rules`), so older projects get on-topic, detailed images without
//! rewriting the narration (Scenes page → "Improve image prompts"). Same structured-JSON
//! request as the script (`script::request_json`).

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::script::{BibleKeys, Validate, bible_section, image_prompt_rules};
use crate::pipeline::presets::Figures;

/// One scene sent for a new image prompt.
#[derive(Debug, Clone, Serialize)]
pub struct PromptInput<'a> {
    pub scene_id: &'a str,
    pub narration: &'a str,
    pub current_image_prompt: &'a str,
    pub characters: &'a [String],
    pub place: &'a str,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PromptReply {
    pub scenes: Vec<PromptOut>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PromptOut {
    pub scene_id: String,
    pub image_prompt: String,
}

impl Validate for PromptReply {
    fn validate(&self) -> Result<(), String> {
        for s in &self.scenes {
            if s.image_prompt.trim().is_empty() {
                return Err(format!("scene {} has an empty image_prompt", s.scene_id));
            }
        }
        Ok(())
    }
}

pub fn schema(scene_ids: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": {
            "scenes": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "scene_id": { "type": "string", "enum": scene_ids },
                        "image_prompt": { "type": "string" }
                    },
                    "required": ["scene_id", "image_prompt"]
                }
            }
        },
        "required": ["scenes"]
    })
}

pub struct PromptContext<'a> {
    pub topic: &'a str,
    pub title: &'a str,
    pub figures: Figures,
    pub bible: BibleKeys<'a>,
    /// Image prompt of the scene before this batch (keeps settings varied across batches).
    pub previous: Option<&'a str>,
}

pub fn prompt(ctx: &PromptContext, scenes: &[PromptInput<'_>]) -> String {
    let scenes_json = serde_json::to_string_pretty(scenes).unwrap_or_default();
    let previous = ctx
        .previous
        .map(|p| format!("The scene before these showed: \"{p}\"\n"))
        .unwrap_or_default();
    format!(
        "You are the art director of a narrated explainer video. Each scene is one illustration shown while \
its narration is spoken. Rewrite the image prompt of every scene below so the picture explains its narration.\n\
TOPIC: {topic}\n\
TITLE: {title}\n\n\
{bible}\n\
Rules:\n\
{rules}\
- Keep the scene's Visual Bible `characters` and `place` (refer to them by key word).\n\
- Consecutive scenes should not look the same: vary setting, objects and viewpoint.\n\
{previous}\
Return `scenes` with one entry per scene below (same `scene_id`), each with the new `image_prompt`.\n\n\
SCENES:\n{scenes_json}",
        topic = ctx.topic,
        title = ctx.title,
        bible = bible_section(&ctx.bible),
        rules = image_prompt_rules(ctx.figures),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_carries_rules_scenes_and_context() {
        let chars = vec!["farmer".to_owned()];
        let scenes = [PromptInput {
            scene_id: "s001",
            narration: "ស្រូវ",
            current_image_prompt: "rice field",
            characters: &chars,
            place: "",
        }];
        let ctx = PromptContext {
            topic: "Rice",
            title: "T",
            figures: Figures::StickFigure,
            bible: BibleKeys { characters: vec![("farmer", "stick figure with a straw hat")], places: vec![] },
            previous: Some("a river"),
        };
        let p = prompt(&ctx, &scenes);
        assert!(p.contains("\"scene_id\": \"s001\""));
        assert!(p.contains("must EXPLAIN this scene's narration"));
        assert!(p.contains("stick figures: show who"));
        assert!(p.contains("- farmer: stick figure with a straw hat"));
        assert!(p.contains("The scene before these showed: \"a river\""));
    }

    #[test]
    fn empty_prompts_are_rejected_and_ids_restricted() {
        let r = |t: &str| PromptReply { scenes: vec![PromptOut { scene_id: "s001".into(), image_prompt: t.into() }] };
        assert!(r(" ").validate().is_err());
        assert!(r("a farmer").validate().is_ok());
        assert_eq!(schema(&["s001"])["properties"]["scenes"]["items"]["properties"]["scene_id"]["enum"], json!(["s001"]));
    }
}
