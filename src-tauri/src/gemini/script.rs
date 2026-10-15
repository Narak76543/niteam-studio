//! Script generation prompts: one outline call, then one call per chapter.
//! Replies are JSON constrained by a schema, validated with serde + [`Validate`],
//! and retried once with the error appended if they don't parse.

use std::sync::atomic::{AtomicBool, Ordering};

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::client::TextModels;
use super::models::{Content, GenerateContentRequest, GenerationConfig};
use super::retry::RetryHooks;
use super::{GeminiClient, GeminiError};
use crate::pipeline::project::Language;
use crate::storage::ApiKey;

pub const MOODS: &[&str] = &[
    "curious", "dramatic", "calm", "hopeful", "tense", "joyful", "somber", "awe", "playful",
];

/// Set when the API rejects `responseFormat`; we then use responseMimeType + responseJsonSchema.
static USE_LEGACY_JSON_FIELDS: AtomicBool = AtomicBool::new(false);

pub struct ScriptBrief<'a> {
    pub topic: &'a str,
    pub language: Language,
    pub target_minutes: u32,
    pub style_label: &'a str,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OutlineReply {
    pub title: String,
    pub hook: String,
    pub chapters: Vec<OutlineChapterReply>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OutlineChapterReply {
    pub title: String,
    pub summary: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChapterReply {
    pub scenes: Vec<SceneReply>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SceneReply {
    pub narration: String,
    pub spoken_text: String,
    pub image_prompt: String,
    pub mood: String,
    #[serde(default)]
    pub fact_note: String,
}

/// Checks beyond what serde enforces. Errors are fed back to the model.
pub trait Validate {
    fn validate(&self) -> Result<(), String>;
}

impl Validate for OutlineReply {
    fn validate(&self) -> Result<(), String> {
        if self.title.trim().is_empty() || self.hook.trim().is_empty() {
            return Err("title and hook must not be empty".into());
        }
        if self.chapters.is_empty() {
            return Err("chapters must not be empty".into());
        }
        if self.chapters.iter().any(|c| c.title.trim().is_empty()) {
            return Err("every chapter needs a title".into());
        }
        Ok(())
    }
}

impl Validate for ChapterReply {
    fn validate(&self) -> Result<(), String> {
        if self.scenes.is_empty() {
            return Err("scenes must not be empty".into());
        }
        for (i, s) in self.scenes.iter().enumerate() {
            if s.narration.trim().is_empty()
                || s.spoken_text.trim().is_empty()
                || s.image_prompt.trim().is_empty()
            {
                return Err(format!(
                    "scene {} has an empty narration, spoken_text or image_prompt",
                    i + 1
                ));
            }
        }
        Ok(())
    }
}

pub fn outline_schema(chapter_count: usize) -> Value {
    json!({
        "type": "object",
        "properties": {
            "title": { "type": "string", "description": "Video title in the narration language" },
            "hook": { "type": "string", "description": "One-sentence hook idea for the first 10 seconds" },
            "chapters": {
                "type": "array",
                "minItems": chapter_count,
                "maxItems": chapter_count,
                "items": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string" },
                        "summary": { "type": "string", "description": "One line: what this chapter covers" }
                    },
                    "required": ["title", "summary"]
                }
            }
        },
        "required": ["title", "hook", "chapters"]
    })
}

pub fn chapter_schema(scene_count: usize) -> Value {
    json!({
        "type": "object",
        "properties": {
            "scenes": {
                "type": "array",
                "minItems": scene_count,
                "maxItems": scene_count,
                "items": {
                    "type": "object",
                    "properties": {
                        "narration": { "type": "string" },
                        "spoken_text": { "type": "string" },
                        "image_prompt": { "type": "string" },
                        "mood": { "type": "string", "enum": MOODS },
                        "fact_note": { "type": "string" }
                    },
                    "required": ["narration", "spoken_text", "image_prompt", "mood", "fact_note"]
                }
            }
        },
        "required": ["scenes"]
    })
}

fn language_rules(language: Language) -> &'static str {
    match language {
        Language::Km => {
            "LANGUAGE: Khmer.\n\
- Write `narration` in natural, spoken Khmer, the way a Cambodian storyteller talks — not a literal translation from English. Prefer common everyday words over formal or literary vocabulary.\n\
- End sentences with ។ . Do not insert zero-width spaces.\n\
- `spoken_text` is the same content written exactly as it must be SPOKEN in Khmer: numbers, years, percentages, units, abbreviations and foreign names written out in Khmer words. Examples: 1,500 → មួយពាន់ប្រាំរយ; year 1863 → ឆ្នាំមួយពាន់ប្រាំបីរយហុកសិបបី; 30% → សាមសិបភាគរយ; 5 km → ប្រាំគីឡូម៉ែត្រ; Mesopotamia → មេសូប៉ូតាមី; BC → មុនគ្រិស្តសករាជ; AD → គ្រិស្តសករាជ. No digits (0-9 or ០-៩) may remain in `spoken_text`.\n\
- `title` and chapter titles in Khmer. `fact_note` in Khmer."
        }
        Language::En => {
            "LANGUAGE: English.\n\
- Write `narration` in clear, warm, spoken English.\n\
- `spoken_text` is the same content written exactly as it must be SPOKEN: numbers, years, units and abbreviations written out as words (1863 → eighteen sixty-three; 5 km → five kilometers; 30% → thirty percent; BC → B.C.). No digits may remain in `spoken_text`."
        }
    }
}

const SYSTEM_PROMPT: &str = "You are the head writer of a faceless YouTube explainer channel in the \
'why / history / science' style. You write narration for one storyteller voice over a sequence of \
illustrations (one illustration per scene, each scene about 6–8 seconds of speech). The tone is \
storytelling: curious, vivid, warm, never dry. Facts must be accurate; when a claim is uncertain, \
disputed or a simplification, say so in that scene's `fact_note` (otherwise leave it empty). \
Reply with JSON only, matching the given schema.";

pub fn outline_prompt(brief: &ScriptBrief, chapter_count: usize, scene_count: usize) -> String {
    format!(
        "Plan a {minutes}-minute narrated explainer video.\n\
TOPIC: {topic}\n\
VISUAL STYLE (for your information only): {style}\n\
{rules}\n\n\
Return:\n\
- `title`: a catchy video title.\n\
- `hook`: the surprising question or fact that opens the video in the first 10 seconds.\n\
- `chapters`: exactly {chapter_count} chapters in story order, each with a short `title` and a one-line `summary`. \
The video will have about {scene_count} scenes in total, spread evenly across the chapters. \
The last chapter wraps up the story with a satisfying conclusion.",
        minutes = brief.target_minutes,
        topic = brief.topic,
        style = brief.style_label,
        rules = language_rules(brief.language),
    )
}

pub struct ChapterContext<'a> {
    pub title: &'a str,
    pub hook: &'a str,
    /// (title, summary) of every chapter, in order.
    pub chapters: Vec<(&'a str, &'a str)>,
    pub index: usize,
    pub scene_count: usize,
    /// Last narration line of the previous chapter, for continuity.
    pub previous_narration: Option<&'a str>,
}

pub fn chapter_prompt(brief: &ScriptBrief, ctx: &ChapterContext) -> String {
    let outline: String = ctx
        .chapters
        .iter()
        .enumerate()
        .map(|(i, (t, s))| format!("{}. {} — {}\n", i + 1, t, s))
        .collect();
    let total = ctx.chapters.len();
    let number = ctx.index + 1;
    let position = if ctx.index == 0 {
        "This is the FIRST chapter: scenes 1–2 must deliver the hook — open with the surprising question or fact so the viewer keeps watching."
    } else if number == total {
        "This is the LAST chapter: end the video with a satisfying conclusion that answers the hook."
    } else {
        "End this chapter's last scene with a small cliffhanger that pulls the viewer into the next chapter."
    };
    let previous = ctx
        .previous_narration
        .map(|p| {
            format!("The previous chapter ended with: \"{p}\". Continue naturally from there.\n")
        })
        .unwrap_or_default();
    format!(
        "Write the scenes for chapter {number} of {total} of the video \"{title}\".\n\
TOPIC: {topic}\n\
HOOK: {hook}\n\
FULL OUTLINE:\n{outline}\n\
CURRENT CHAPTER: {number}. {chapter_title}\n\
{previous}{position}\n\n\
{rules}\n\n\
Write exactly {scenes} scenes. For each scene:\n\
- `narration`: one or two short sentences (about 6–8 seconds when spoken).\n\
- `spoken_text`: the narration written for text-to-speech (see language rules).\n\
- `image_prompt`: in ENGLISH. Describe only what is visible: subject, setting, era, action, composition. \
No art-style words (no 'watercolor', 'photo', 'cartoon', 'illustration'), no text, letters, signs or captions in the image, \
no real living people. Describe recurring characters and places the same way every time so the images stay consistent.\n\
- `mood`: one of {moods}.\n\
- `fact_note`: empty, or a short note if the scene makes an uncertain or disputed claim the creator should check.",
        title = ctx.title,
        topic = brief.topic,
        hook = ctx.hook,
        chapter_title = ctx.chapters.get(ctx.index).map(|c| c.0).unwrap_or_default(),
        rules = language_rules(brief.language),
        scenes = ctx.scene_count,
        moods = MOODS.join(", "),
    )
}

fn is_response_format_rejected(err: &GeminiError) -> bool {
    match err {
        GeminiError::Api {
            status: 400,
            message,
            ..
        } => {
            let m = message.to_lowercase();
            m.contains("responseformat") || m.contains("response_format")
        }
        _ => false,
    }
}

/// Sends a JSON-schema request and parses the reply into `T`.
/// On a parse/validation failure, retries once with the error appended.
/// Transport-level retries and the fallback model are handled by the client.
pub async fn request_json<T: DeserializeOwned + Validate>(
    client: &GeminiClient,
    key: &ApiKey,
    models: &TextModels,
    prompt: String,
    schema: Value,
    hooks: &RetryHooks,
) -> Result<T, GeminiError> {
    let mut contents = vec![Content::user(prompt)];
    let mut last_error = String::new();
    for attempt in 0..2 {
        let raw = generate_text(client, key, models, &contents, &schema, hooks).await?;
        match parse_reply::<T>(&raw) {
            Ok(v) => return Ok(v),
            Err(e) if attempt == 0 => {
                last_error = e.clone();
                contents.push(Content::model(raw));
                contents.push(Content::user(format!(
                    "Your previous reply was invalid: {e}. Reply again with ONLY valid JSON that matches the schema."
                )));
            }
            Err(e) => last_error = e,
        }
    }
    Err(GeminiError::BadResponse(last_error))
}

async fn generate_text(
    client: &GeminiClient,
    key: &ApiKey,
    models: &TextModels,
    contents: &[Content],
    schema: &Value,
    hooks: &RetryHooks,
) -> Result<String, GeminiError> {
    let build = |legacy: bool| GenerateContentRequest {
        contents: contents.to_vec(),
        system_instruction: Some(Content::system(SYSTEM_PROMPT)),
        generation_config: Some(GenerationConfig::json(schema.clone(), legacy)),
    };
    let legacy = USE_LEGACY_JSON_FIELDS.load(Ordering::Relaxed);
    let response = match client
        .generate_text(key, models, &build(legacy), hooks)
        .await
    {
        Err(e) if !legacy && is_response_format_rejected(&e) => {
            USE_LEGACY_JSON_FIELDS.store(true, Ordering::Relaxed);
            client
                .generate_text(key, models, &build(true), hooks)
                .await?
        }
        other => other?,
    };
    response.text()
}

/// Parses model output, tolerating a ```json fence around it.
pub fn parse_reply<T: DeserializeOwned + Validate>(raw: &str) -> Result<T, String> {
    let trimmed = raw.trim();
    let body = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|s| s.strip_suffix("```"))
        .unwrap_or(trimmed);
    let value: T = serde_json::from_str(body).map_err(|e| format!("JSON error: {e}"))?;
    value.validate()?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brief() -> ScriptBrief<'static> {
        ScriptBrief {
            topic: "Where did rice come from?",
            language: Language::Km,
            target_minutes: 3,
            style_label: "storybook watercolor",
        }
    }

    #[test]
    fn parses_fenced_json() {
        let raw = "```json\n{\"title\":\"t\",\"hook\":\"h\",\"chapters\":[{\"title\":\"a\",\"summary\":\"b\"}]}\n```";
        let o: OutlineReply = parse_reply(raw).unwrap();
        assert_eq!(o.chapters.len(), 1);
    }

    #[test]
    fn validation_errors_are_reported() {
        let raw = r#"{"scenes":[{"narration":"x","spoken_text":"","image_prompt":"y","mood":"calm","fact_note":""}]}"#;
        let err = parse_reply::<ChapterReply>(raw).unwrap_err();
        assert!(err.contains("scene 1"));
        assert!(parse_reply::<ChapterReply>("not json").is_err());
    }

    #[test]
    fn khmer_prompt_contains_spoken_rules() {
        let p = outline_prompt(&brief(), 4, 26);
        assert!(p.contains("Khmer"));
        assert!(p.contains("exactly 4 chapters"));
    }

    #[test]
    fn first_and_last_chapter_get_position_rules() {
        let chapters = vec![("A", "a"), ("B", "b")];
        let mut ctx = ChapterContext {
            title: "T",
            hook: "H",
            chapters,
            index: 0,
            scene_count: 5,
            previous_narration: None,
        };
        assert!(chapter_prompt(&brief(), &ctx).contains("FIRST chapter"));
        ctx.index = 1;
        ctx.previous_narration = Some("prev");
        let p = chapter_prompt(&brief(), &ctx);
        assert!(p.contains("LAST chapter"));
        assert!(p.contains("prev"));
    }

    #[test]
    fn detects_rejected_response_format() {
        let e = GeminiError::Api {
            status: 400,
            message: "Invalid JSON payload received. Unknown name \"responseFormat\"".into(),
            retry_after: None,
        };
        assert!(is_response_format_rejected(&e));
    }
}
