//! Script generation prompts: one outline call, then one call per chapter.
//! Replies are JSON constrained by a schema, validated with serde + [`Validate`],
//! and retried once with the error appended if they don't parse.

use std::sync::atomic::{AtomicBool, Ordering};

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::client::ModelChain;
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
    /// The art style draws people as stick figures (changes the `image_prompt` rules).
    pub stick_figure: bool,
    /// Names the user wants kept exactly as written (Script page → Names).
    pub keep_terms: &'a [String],
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
    /// Visual Bible character keys in this scene (07 §2).
    #[serde(default)]
    pub characters: Vec<String>,
    /// Visual Bible place key, or "".
    #[serde(default)]
    pub place: String,
    /// wide | medium | close | detail | aerial
    #[serde(default)]
    pub shot: String,
    #[serde(default)]
    pub key_scene: bool,
}

pub const SHOTS: &[&str] = &["wide", "medium", "close", "detail", "aerial"];

/// Visual Bible keys the chapter prompt may refer to: (key, description).
#[derive(Default)]
pub struct BibleKeys<'a> {
    pub characters: Vec<(&'a str, &'a str)>,
    pub places: Vec<(&'a str, &'a str)>,
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

/// Thai (U+0E00..U+0E7F) and Lao (U+0E80..U+0EFF) letters never belong in Khmer or English
/// narration, but lighter models sometimes mix them into Khmer words.
pub(crate) fn foreign_script(text: &str) -> Option<char> {
    text.chars().find(|c| ('\u{0E00}'..='\u{0EFF}').contains(c))
}

impl Validate for ChapterReply {
    fn validate(&self) -> Result<(), String> {
        if self.scenes.is_empty() {
            return Err("scenes must not be empty".into());
        }
        for (i, s) in self.scenes.iter().enumerate() {
            for (field, text) in [("narration", &s.narration), ("spoken_text", &s.spoken_text)] {
                if let Some(c) = foreign_script(text) {
                    return Err(format!(
                        "scene {} {field} contains a Thai/Lao character '{c}'; write it in Khmer script only",
                        i + 1
                    ));
                }
            }
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

pub fn chapter_schema(scene_count: usize, bible: &BibleKeys) -> Value {
    let character_keys: Vec<&str> = bible.characters.iter().map(|c| c.0).collect();
    let mut place_keys: Vec<&str> = bible.places.iter().map(|p| p.0).collect();
    place_keys.push("");
    let characters = if character_keys.is_empty() {
        json!({ "type": "array", "items": { "type": "string" } })
    } else {
        json!({ "type": "array", "items": { "type": "string", "enum": character_keys } })
    };
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
                        "fact_note": { "type": "string" },
                        "characters": characters,
                        "place": { "type": "string", "enum": place_keys },
                        "shot": { "type": "string", "enum": SHOTS },
                        "key_scene": { "type": "boolean" }
                    },
                    "required": ["narration", "spoken_text", "image_prompt", "mood", "fact_note",
                                 "characters", "place", "shot", "key_scene"]
                }
            }
        },
        "required": ["scenes"]
    })
}

/// Product / brand / technology names stay in their original Latin spelling in the
/// narration (subtitles); the spoken text spells them for the voice. Shared with the
/// proofreader.
pub fn names_rule(language: Language, keep_terms: &[String]) -> String {
    let base = match language {
        Language::Km => {
            "- NAMES STAY AS WRITTEN: names of products, brands, companies, apps, websites, programming languages, \
frameworks, software, devices and technical terms (for example React Native, Android, iPhone, Google, YouTube, \
JavaScript, Wi-Fi, AI) stay EXACTLY as written, in Latin letters with their usual capitalization, in `narration` \
and in titles. Never translate them and never transliterate them into Khmer letters there. In `spoken_text`, \
write them in Khmer letters the way Cambodians pronounce them, so the Khmer voice says them correctly."
        }
        Language::En => {
            "- NAMES STAY AS WRITTEN: names of products, brands, companies, apps, websites, programming languages, \
frameworks, software, devices and technical terms (for example React Native, Android, iPhone, JavaScript) keep \
their exact official spelling and capitalization in `narration`, `spoken_text` and titles."
        }
    };
    let terms: Vec<&str> = keep_terms.iter().map(|t| t.trim()).filter(|t| !t.is_empty()).collect();
    if terms.is_empty() {
        base.to_owned()
    } else {
        format!(
            "{base}\n- These names must appear EXACTLY as written (same letters and capitalization) wherever they occur in `narration`: {}.",
            terms.join(", ")
        )
    }
}

fn language_rules(brief: &ScriptBrief) -> String {
    format!("{}\n{}", language_rules_base(brief.language), names_rule(brief.language, brief.keep_terms))
}

fn language_rules_base(language: Language) -> &'static str {
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
        rules = language_rules(brief),
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
    /// Shots of the previous chapter's last two scenes (to keep shots varied).
    pub previous_shots: Vec<&'a str>,
    pub bible: BibleKeys<'a>,
}

pub fn bible_section(b: &BibleKeys) -> String {
    let list = |items: &[(&str, &str)]| -> String {
        if items.is_empty() {
            "(none)\n".to_owned()
        } else {
            items.iter().map(|(k, d)| format!("- {k}: {d}\n")).collect()
        }
    };
    format!(
        "VISUAL BIBLE (refer to these by key; their look is added to the image automatically):\nCharacters:\n{}Places:\n{}",
        list(&b.characters),
        list(&b.places)
    )
}

/// How each scene's `image_prompt` is written (`docs/07-image-generation.md` §3): the image
/// must explain THIS scene's narration with topic-specific detail, not a generic mood shot.
pub fn image_prompt_rules(stick_figure: bool) -> String {
    let people = if stick_figure {
        "People are drawn as simple stick figures: show who they are through props, tools, clothing \
accents and what they are doing, never through faces or bodies. "
    } else {
        ""
    };
    format!(
        "- `image_prompt`: in ENGLISH, 40–80 words. The image must EXPLAIN this scene's narration: show the exact \
thing it talks about (the named object, material, invention, event, place, number or comparison), so a viewer \
with the sound off still understands the scene. Build it like an explainer illustration:\n\
  1. Main subject doing the key action of the narration, specific to the topic and its era/place.\n\
  2. Two to four concrete supporting details that only belong to this topic (period tools, materials, \
animals, plants, buildings, clothing, instruments) — never generic filler like \"artifacts\" or \"items\".\n\
  3. A recognisable setting/background (where and when), different from the previous scene unless the story stays there.\n\
  4. When the idea is abstract (a cause, a process, a comparison, a quantity, a change over time), turn it into \
something visible: side-by-side comparison, before/after, step-by-step sequence of objects left to right, \
cutaway or cross-section, a magnified inset circle, a size comparison next to a familiar object, a simple map shape, \
or a row of objects showing growth.\n\
  {people}Refer to Visual Bible characters and places by their key word (e.g. \"the farmer\") instead of describing \
their look again. Use positive phrasing only (say \"clear blue sky\", not \"no birds\"). Never mention text, letters, \
numbers written on things, signs, labels or captions. No art-style, lighting or camera words (watercolor, photo, \
cartoon, illustration, cinematic, lighting, detailed, 4k) — the style is added separately. Keep sensitive topics \
tasteful and educational (objects, diagrams, historical settings; no nudity). No real living people; historical \
figures only as statues, carvings or generic figures.\n"
    )
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
    let previous_shots = if ctx.previous_shots.is_empty() {
        String::new()
    } else {
        format!(
            "The previous scenes used these shots: {}.\n",
            ctx.previous_shots.join(", ")
        )
    };
    let key_rule = if number == total {
        "true for the FIRST scene of this chapter and for the LAST scene of this chapter (the video's ending); false otherwise"
    } else {
        "true for the FIRST scene of this chapter; false otherwise"
    };
    format!(
        "Write the scenes for chapter {number} of {total} of the video \"{title}\".\n\
TOPIC: {topic}\n\
HOOK: {hook}\n\
FULL OUTLINE:\n{outline}\n\
CURRENT CHAPTER: {number}. {chapter_title}\n\
{previous}{position}\n\n\
{bible}\n\
{rules}\n\n\
Write exactly {scenes} scenes. For each scene:\n\
- `narration`: one or two short sentences (about 6–8 seconds when spoken).\n\
- `spoken_text`: the narration written for text-to-speech (see language rules).\n\
{image_rules}\
- `mood`: one of {moods}.\n\
- `fact_note`: empty, or a short note if the scene makes an uncertain or disputed claim the creator should check.\n\
- `characters`: keys of the Visual Bible characters visible in the scene ([] if none).\n\
- `place`: the Visual Bible place key where the scene happens, or \"\" if none fits.\n\
- `shot`: one of {shots}. Vary shots: never more than 2 consecutive scenes with the same shot.\n\
{previous_shots}\
- `key_scene`: {key_rule}.",
        bible = bible_section(&ctx.bible),
        image_rules = image_prompt_rules(brief.stick_figure),
        shots = SHOTS.join(", "),
        title = ctx.title,
        topic = brief.topic,
        hook = ctx.hook,
        chapter_title = ctx.chapters.get(ctx.index).map(|c| c.0).unwrap_or_default(),
        rules = language_rules(brief),
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

/// Sends a JSON-schema request over the text model chain and parses the reply into `T`.
/// On a parse/validation failure, retries once with the error appended.
/// Returns the value and the model that produced it (for the log).
pub async fn request_json<T: DeserializeOwned + Validate>(
    client: &GeminiClient,
    key: &ApiKey,
    chain: &ModelChain,
    prompt: String,
    schema: Value,
    hooks: &RetryHooks,
) -> Result<(T, String), GeminiError> {
    let mut contents = vec![Content::user(prompt)];
    let mut last_error = String::new();
    for attempt in 0..2 {
        let (raw, model) = generate_text(client, key, chain, &contents, &schema, hooks).await?;
        match parse_reply::<T>(&raw) {
            Ok(v) => return Ok((v, model)),
            Err(e) if attempt == 0 => {
                last_error = format!("{model}: {e}");
                contents.push(Content::model(raw));
                contents.push(Content::user(format!(
                    "Your previous reply was invalid: {e}. Reply again with ONLY valid JSON that matches the schema."
                )));
            }
            Err(e) => last_error = format!("{model}: {e}"),
        }
    }
    Err(GeminiError::BadResponse(last_error))
}

async fn generate_text(
    client: &GeminiClient,
    key: &ApiKey,
    chain: &ModelChain,
    contents: &[Content],
    schema: &Value,
    hooks: &RetryHooks,
) -> Result<(String, String), GeminiError> {
    let build = |legacy: bool| GenerateContentRequest {
        contents: contents.to_vec(),
        system_instruction: Some(Content::system(SYSTEM_PROMPT)),
        generation_config: Some(GenerationConfig::json(schema.clone(), legacy)),
    };
    let legacy = USE_LEGACY_JSON_FIELDS.load(Ordering::Relaxed);
    let (response, model) = match client
        .generate_text(key, chain, &build(legacy), hooks)
        .await
    {
        Err(e) if !legacy && is_response_format_rejected(&e) => {
            USE_LEGACY_JSON_FIELDS.store(true, Ordering::Relaxed);
            client
                .generate_text(key, chain, &build(true), hooks)
                .await?
        }
        other => other?,
    };
    Ok((response.text()?, model))
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
            stick_figure: false,
            keep_terms: &[],
        }
    }

    #[test]
    fn image_prompts_must_explain_the_narration() {
        let ctx = ChapterContext {
            title: "T",
            hook: "H",
            chapters: vec![("A", "a")],
            index: 0,
            scene_count: 5,
            previous_narration: None,
            previous_shots: vec![],
            bible: BibleKeys { characters: vec![], places: vec![] },
        };
        let p = chapter_prompt(&brief(), &ctx);
        assert!(p.contains("must EXPLAIN this scene's narration"));
        assert!(p.contains("side-by-side comparison"));
        assert!(!p.contains("stick figures: show who"));
        let stick = chapter_prompt(&ScriptBrief { stick_figure: true, ..brief() }, &ctx);
        assert!(stick.contains("stick figures: show who"));
        assert!(stick.contains("- `mood`:"), "rules are followed by the next field");
    }

    #[test]
    fn names_stay_in_latin_in_narration_and_user_terms_are_listed() {
        let terms = vec!["React Native".to_owned(), " ".to_owned(), "Android".to_owned()];
        let km = names_rule(Language::Km, &terms);
        assert!(km.contains("Latin letters"));
        assert!(km.contains("Never translate"));
        assert!(km.ends_with("in `narration`: React Native, Android."), "{km}");
        assert!(!names_rule(Language::En, &[]).contains("must appear EXACTLY"));
        let b = ScriptBrief { keep_terms: &terms, ..brief() };
        assert!(outline_prompt(&b, 2, 9).contains("React Native, Android"));
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
    fn rejects_thai_letters_mixed_into_khmer() {
        let raw = r#"{"scenes":[{"narration":"ញើសឈាមរបស់บรรพបុរាណ","spoken_text":"x","image_prompt":"y","mood":"calm","fact_note":""}]}"#;
        let err = parse_reply::<ChapterReply>(raw).unwrap_err();
        assert!(err.contains("Thai/Lao"), "{err}");
        let ok = r#"{"scenes":[{"narration":"ញើសឈាមរបស់បុព្វបុរស","spoken_text":"x","image_prompt":"y","mood":"calm","fact_note":""}]}"#;
        assert!(parse_reply::<ChapterReply>(ok).is_ok());
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
            previous_shots: vec![],
            bible: BibleKeys {
                characters: vec![("farmer", "old farmer")],
                places: vec![],
            },
        };
        let p = chapter_prompt(&brief(), &ctx);
        assert!(p.contains("FIRST chapter"));
        assert!(p.contains("- farmer: old farmer"));
        assert!(p.contains("never more than 2 consecutive scenes"));
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
            body: String::new(),
            retry_after: None,
        };
        assert!(is_response_format_rejected(&e));
    }
}
