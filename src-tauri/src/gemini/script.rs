//! Script generation prompts: one outline call, then one call per chapter.
//! Replies are JSON constrained by a schema, validated with serde + [`Validate`],
//! and repaired once with targeted feedback; a second invalid response escalates to the next model.

use std::sync::atomic::{AtomicBool, Ordering};

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::client::ModelChain;
use super::models::{Content, GenerateContentRequest, GenerationConfig};
use super::retry::{RetryHooks, RetryNotice, RetryReason};
use super::{GeminiClient, GeminiError};
use crate::pipeline::presets::{self, Figures};
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
    /// How the art style draws people (changes the `image_prompt` rules).
    pub figures: Figures,
    /// Names the user wants kept exactly as written (Script page → Names).
    pub keep_terms: &'a [String],
    /// "From document" projects: `document::source_block` (selected points, rules).
    pub source: Option<&'a str>,
    /// "Real-world Tech Explainer": each scene also gets a visual type, English search
    /// keywords, a Mermaid diagram or a code snippet (`visual_rules`).
    pub real_world: bool,
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
    /// "From document" projects only (10 §6).
    #[serde(default)]
    pub source_pages: Vec<u32>,
    #[serde(default)]
    pub general_knowledge: bool,
    /// Real-world projects only: one of [`VISUAL_TYPES`].
    #[serde(default)]
    pub visual_type: String,
    /// English stock-search phrases.
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub mermaid: String,
    #[serde(default)]
    pub code_language: String,
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub terminal: bool,
}

/// What a real-world scene shows (`pipeline::project::VisualKind`, minus `screenshot`, which
/// only the user adds).
pub const VISUAL_TYPES: &[&str] = &["real_photo", "stock_video", "diagram", "code", "ai_image"];
/// Code languages the code renderer highlights; anything else is shown as plain text.
pub const CODE_LANGUAGES: &[&str] = &[
    "bash", "powershell", "python", "javascript", "typescript", "json", "yaml", "rust", "go", "java", "c", "cpp",
    "sql", "html", "css", "text",
];

/// Diagram template of a Mermaid source from its first line: `flow`, `sequence` or
/// `timeline`; `None` for anything else (we only ask for these three).
pub fn mermaid_template(src: &str) -> Option<&'static str> {
    let first = src.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with("%%"))?;
    let head = first.split_whitespace().next()?;
    match head {
        "flowchart" | "graph" => Some("flow"),
        "sequenceDiagram" => Some("sequence"),
        "timeline" => Some("timeline"),
        _ => None,
    }
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
            let n = i + 1;
            match s.visual_type.as_str() {
                "real_photo" | "stock_video" if s.keywords.iter().all(|k| k.trim().is_empty()) => {
                    return Err(format!("scene {n} is {} but has no `keywords`", s.visual_type));
                }
                "diagram" if mermaid_template(&s.mermaid).is_none() => {
                    return Err(format!(
                        "scene {n} is a diagram but `mermaid` does not start with `flowchart LR`, `flowchart TD`, `sequenceDiagram` or `timeline`"
                    ));
                }
                "code" if s.code.trim().is_empty() => return Err(format!("scene {n} is code but `code` is empty")),
                _ => {}
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

pub fn chapter_schema(scene_count: usize, bible: &BibleKeys, with_source: bool, real_world: bool) -> Value {
    let mut schema = scene_schema(scene_count, bible);
    if real_world {
        let item = &mut schema["properties"]["scenes"]["items"];
        // Keep this extension deliberately simple: Gemini can reject complex structured-output
        // schemas with a generic INVALID_ARGUMENT. The prompt describes the allowed values, and
        // visual_from_reply validates/normalizes them after parsing.
        item["properties"]["visual_type"] = json!({ "type": "string" });
        item["properties"]["keywords"] = json!({ "type": "array", "items": { "type": "string" } });
        item["properties"]["mermaid"] = json!({ "type": "string" });
        item["properties"]["code_language"] = json!({ "type": "string" });
        item["properties"]["code"] = json!({ "type": "string" });
        item["properties"]["terminal"] = json!({ "type": "boolean" });
        if let Some(req) = item["required"].as_array_mut() {
            for f in ["visual_type", "keywords", "mermaid", "code_language", "code", "terminal"] {
                req.push(json!(f));
            }
        }
    }
    if with_source {
        let item = &mut schema["properties"]["scenes"]["items"];
        item["properties"]["source_pages"] = json!({ "type": "array", "items": { "type": "integer" } });
        item["properties"]["general_knowledge"] = json!({ "type": "boolean" });
        if let Some(req) = item["required"].as_array_mut() {
            req.push(json!("source_pages"));
            req.push(json!("general_knowledge"));
        }
    }
    schema
}

fn scene_schema(scene_count: usize, bible: &BibleKeys) -> Value {
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
            "- NAMES, TECH & PROPER NOUNS STAY IN ENGLISH: names of people (e.g. Neil Armstrong, Buzz Aldrin, Michael Collins), \
spacecraft, rockets and missions (e.g. Apollo 11, Saturn V, Eagle, Columbia), organizations (NASA), products, \
brands, companies, apps, programming languages, devices, and technical terms stay in LATIN letters with their official \
spelling and capitalization in BOTH `narration` (subtitles) and `spoken_text` (TTS). Never translate them and DO NOT \
transliterate them into Khmer script in `spoken_text`. Keep them in English so the speech synthesis engine (TTS/VoxCPM) \
speaks them directly in natural, clean English. Technical initialisms (AI, API, CPU, GPU, USB) also stay in Latin letters \
in both fields. The digits of designations and years (e.g. Apollo 11, 1969) stay as plain digits — never spell them out in Khmer number words."
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
- Write EVERY number with DIGITS (0-9), in BOTH `narration` and `spoken_text`: years `ឆ្នាំ 1969`, quantities `1,500 នាក់`, decimals `3.5`, percentages `30%`. NEVER spell a number or a year out in Khmer number words (write `ឆ្នាំ 1863`, never `ឆ្នាំមួយពាន់ប្រាំបីរយ…`): spelled-out Khmer numbers usually come out wrong, while the app reads the digits in correct Khmer itself.\n\
- `spoken_text` is the same content written exactly as it must be SPOKEN: write units in Khmer words (km → គីឡូម៉ែត្រ) and translate English abbreviations (BC → មុនគ្រិស្តសករាជ, AD → គ្រិស្តសករាជ). Foreign names of people, places, missions, and technical terms stay in Latin letters in BOTH fields (see names rule below).\n\
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
storytelling: curious, vivid, warm, never dry. Ground every scene in concrete historical, scientific \
or technical facts: specific years, dates, named individuals, locations, mechanisms, and cause-and-effect \
progress. NEVER write repetitive filler or re-tell already covered events. When a claim is uncertain, \
disputed or a simplification, say so in that scene's `fact_note` (otherwise leave it empty). \
Reply with JSON only, matching the given schema.";

/// The source document block plus the rule for the outline ("" for topic projects).
fn outline_source(brief: &ScriptBrief) -> String {
    brief.source.map_or(String::new(), |s| {
        format!(
            "\n{s}\nPlan the chapters around the KEY POINTS of the source document, in their order: every point \
is covered and nothing outside the document is presented as its content.\n"
        )
    })
}

pub fn outline_prompt(brief: &ScriptBrief, chapter_count: usize, scene_count: usize) -> String {
    format!(
        "Plan a {minutes}-minute narrated explainer video.\n\
TOPIC: {topic}\n\
VISUAL STYLE (for your information only): {style}\n\
{rules}\n{source}\n\
Return:\n\
- `title`: a catchy video title.\n\
- `hook`: the surprising question or fact that opens the video in the first 10 seconds.\n\
- `chapters`: exactly {chapter_count} chapters in strict chronological and logical progression. \
Each chapter must cover a distinct phase or aspect of the story without overlapping: \
never rush ahead to events that belong to later chapters, and never duplicate coverage. \
Each chapter needs a short `title` and a clear, fact-specific one-line `summary` outlining the key events, \
years, people, and turning points for that chapter. \
The video will have about {scene_count} scenes in total, spread evenly across the chapters. \
The last chapter wraps up the story with a satisfying conclusion.",
        minutes = brief.target_minutes,
        topic = brief.topic,
        style = brief.style_label,
        rules = language_rules(brief),
        source = outline_source(brief),
    )
}

pub struct ChapterContext<'a> {
    pub title: &'a str,
    pub hook: &'a str,
    /// (title, summary) of every chapter, in order.
    pub chapters: Vec<(&'a str, &'a str)>,
    pub index: usize,
    pub scene_count: usize,
    pub segment_start: usize,
    pub chapter_total_scenes: usize,
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
pub fn image_prompt_rules(figures: Figures, topic: &str, title: &str) -> String {
    let topical_rules = presets::topic_visual_rules(&[topic, title]);
    let people = match figures {
        Figures::StickFigure => {
            "People are drawn as simple stick figures: show who they are through props, tools, clothing \
accents and what they are doing, never through faces or bodies. "
        }
        Figures::SharedCartoon => {
            "All people share ONE simple cartoon design that is added to the image automatically: show who \
they are only through their clothing (type and colour), at most one prop or headwear, and what they are doing; never \
describe faces, skin, hair, age or bodies. Describe places as soft painted landscapes or simple buildings. Keep the \
exact object, event or action named in this scene visually dominant; do not substitute a generic person in scenery. \
Use only scene-specific props that help explain the narration, omit unrelated recurring motifs, and include only the \
people needed by the narration (no default crowd or classroom). "
        }
        Figures::Free => "",
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
{topical_rules}\n\
Give every scene in this batch a different setting, composition and visible action while keeping the same guide.\n\
  {people}Refer to Visual Bible characters and places by their key word (e.g. \"the farmer\") instead of describing \
their look again. Use positive phrasing only (say \"clear blue sky\", not \"no birds\"). Never mention text, letters, \
numbers written on things, signs, labels or captions. No art-style, lighting or camera words (watercolor, photo, \
cartoon, illustration, cinematic, lighting, detailed, 4k) — the style is added separately. Keep sensitive topics \
tasteful and educational (objects, diagrams, historical settings; no nudity). No real living people. Historical \
figures must be depicted with authentic historical likeness and era-appropriate attire (e.g. Thomas Edison as an adult 19th-century inventor).\n"
    )
}

/// How real-world scenes choose and describe their visual (`docs/11-real-world-visuals.md`).
pub fn visual_rules(language: Language) -> String {
    let labels = match language {
        Language::Km => "in Khmer (keep product and technology names in Latin letters)",
        Language::En => "in English",
    };
    format!(
        "VISUALS: this video uses REAL photos and short stock video clips, plus diagrams and code. For each scene:\n\
- `visual_type`: one of {types}.\n\
  - real_photo (most scenes): a real photograph exists of what the narration talks about (equipment, places, \
machines, cables, buildings, devices, people at work as anonymous figures).\n\
  - stock_video: only when motion helps (a ship at sea, waves, a data center walk-through, blinking network \
lights, traffic); at most 1 scene in 5.\n\
  - diagram: when the scene explains a process, a path, an architecture, an order of steps or a timeline; about \
one per chapter.\n\
  - code: only when the topic involves a command, configuration or program output a viewer could type (for example \
a traceroute or ping in a terminal); at most 2 in the whole video.\n\
  - ai_image: only when no real photograph could exist (inside a chip at atomic scale, the far future, an abstract idea).\n\
- `keywords`: for real_photo and stock_video, 2–4 short ENGLISH search phrases for stock photo sites, concrete and \
visual (things a photographer could shoot), most specific first: e.g. \"submarine cable cross section\", \
\"cable laying ship\", \"data center server racks\". No abstract words, no people's names, no years. For other types, \
1–2 phrases are still useful as a fallback.\n\
- `mermaid`: for diagram only (else \"\"): valid Mermaid that starts with `flowchart LR`, `flowchart TD`, \
`sequenceDiagram` or `timeline`. At most 8 nodes or steps. Node ids are plain ASCII letters (A, B, C); every label \
in double quotes, at most 5 words, {labels}. No styling, classDef, click, links, emoji, HTML or markdown. Use \
`\\n` nowhere; keep each label on one line.\n\
- `code_language` and `code`: for code only (else \"text\" and \"\"): at most 12 lines, realistic and correct. \
For a terminal set `terminal` true, start each command with `$ ` and include short realistic output. Use documentation \
addresses only (example.com, 192.0.2.x, 203.0.113.x); never real keys, passwords or private data.\n\
- `image_prompt` is still required for every scene: it is the fallback picture when no photo fits.\n",
        types = VISUAL_TYPES.join(", "),
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
    let is_chapter_start = ctx.segment_start == 0;
    let is_chapter_end = ctx.segment_start + ctx.scene_count >= ctx.chapter_total_scenes.max(ctx.scene_count);

    let position = if ctx.index == 0 && is_chapter_start {
        "This is the FIRST chapter: scenes 1–2 must deliver the hook — open with the surprising question or fact so the viewer keeps watching."
    } else if number == total && is_chapter_end {
        "This is the LAST chapter: end the video with a satisfying conclusion that answers the hook."
    } else if is_chapter_end {
        "End this chapter's last scene with a small cliffhanger that pulls the viewer into the next chapter."
    } else {
        "Advance the narrative chronologically toward the chapter's key event; do not wrap up or conclude the chapter yet."
    };
    let previous = ctx
        .previous_narration
        .map(|p| {
            if is_chapter_start {
                format!("The previous chapter ended with: \"{p}\". Continue naturally from there.\n")
            } else {
                format!("The immediately preceding scene ended with: \"{p}\". Continue naturally from there without repeating.\n")
            }
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
    let chapter_title = ctx.chapters.get(ctx.index).map_or("", |c| c.0);
    let chapter_summary = ctx.chapters.get(ctx.index).map_or("", |c| c.1);
    let chapter_header = if is_chapter_start {
        format!(
            "Write scenes 1–{count} (out of {total_in_ch}) for chapter {number} of {total} of the video \"{title}\".\n\
CURRENT CHAPTER: {number}. {chapter_title}\n\
CHAPTER FOCUS & BOUNDARY: {chapter_summary}\n\
CRITICAL SCOPE RULE: Cover ONLY the events and facts designated for Chapter {number}. DO NOT jump ahead to future chapters ({next}..{total}). DO NOT repeat events already covered in earlier chapters.\n",
            count = ctx.scene_count,
            total_in_ch = ctx.chapter_total_scenes.max(ctx.scene_count),
            number = number,
            total = total,
            title = ctx.title,
            chapter_title = chapter_title,
            chapter_summary = chapter_summary,
            next = (number + 1).min(total),
        )
    } else {
        format!(
            "CONTINUATION OF CHAPTER {number}: {chapter_title}.\n\
CHAPTER FOCUS & BOUNDARY: {chapter_summary}\n\
Write scenes {start_1}–{end_1} (out of {total_in_ch}) for the video \"{title}\".\n\
CRITICAL INSTRUCTION: DO NOT restart this chapter. Scenes 1–{start} are ALREADY written. Continue the story chronologically from scene {start}.\n\
CRITICAL SCOPE RULE: Stay strictly within Chapter {number}'s topic. DO NOT repeat scenes already written. DO NOT jump ahead to future chapters ({next}..{total}).\n",
            number = number,
            chapter_title = chapter_title,
            chapter_summary = chapter_summary,
            start_1 = ctx.segment_start + 1,
            end_1 = ctx.segment_start + ctx.scene_count,
            total_in_ch = ctx.chapter_total_scenes.max(ctx.segment_start + ctx.scene_count),
            title = ctx.title,
            start = ctx.segment_start,
            next = (number + 1).min(total),
        )
    };
    format!(
        "{chapter_header}\
TOPIC: {topic}\n\
HOOK: {hook}\n\
FULL OUTLINE:\n{outline}\n\
{previous}{position}\n\n\
STORYTELLING & FACTUAL SPECIFICITY:\n\
1. PROGRESSION: Each scene must deliver NEW facts or narrative beats that advance the story forward. Never loop back or repeat the same event, idea, or phrasing across different scenes.\n\
2. CONCRETE FACTS: Anchor the narration with specific historical dates, years, named individuals, locations, missions, measurements, and cause-and-effect explanations. Avoid vague or repetitive filler.\n\
3. STORY FLOW: Flow naturally from the previous scene, maintaining curiosity and excitement without jumping disjointedly.\n\n\
{bible}\n\
{rules}\n{source}\n\
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
- `key_scene`: {key_rule}.{source_fields}{visuals}",
        chapter_header = chapter_header,
        topic = brief.topic,
        hook = ctx.hook,
        outline = outline,
        previous = previous,
        position = position,
        bible = bible_section(&ctx.bible),
        rules = language_rules(brief),
        source = brief.source.map_or(String::new(), |s| format!("\n{s}")),
        scenes = ctx.scene_count,
        image_rules = image_prompt_rules(brief.figures, brief.topic, ctx.title),
        moods = MOODS.join(", "),
        shots = SHOTS.join(", "),
        previous_shots = previous_shots,
        key_rule = key_rule,
        source_fields = if brief.source.is_some() {
            "\n- `source_pages`: page numbers from the source the scene's claims come from ([] if none).\n\
- `general_knowledge`: true only for background explanation that is not from the document."
        } else {
            ""
        },
        visuals = if brief.real_world { format!("\n{}", visual_rules(brief.language)) } else { String::new() },
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
/// On a parse/validation failure, asks the answering model for a focused repair. If that
/// response still fails validation, tries the next configured model with the same repair context.
/// Returns the value and the model that produced it (for the log).
pub async fn request_json<T: DeserializeOwned + Validate>(
    client: &GeminiClient,
    key: &ApiKey,
    chain: &ModelChain,
    prompt: String,
    schema: Value,
    hooks: &RetryHooks,
) -> Result<(T, String), GeminiError> {
    request_json_with(client, key, chain, SYSTEM_PROMPT, Content::user(prompt), schema, hooks).await
}

/// [`request_json`] with its own system prompt and first turn (e.g. a PDF + instructions).
pub async fn request_json_with<T: DeserializeOwned + Validate>(
    client: &GeminiClient,
    key: &ApiKey,
    chain: &ModelChain,
    system: &str,
    first: Content,
    schema: Value,
    hooks: &RetryHooks,
) -> Result<(T, String), GeminiError> {
    let mut contents = vec![first];
    let (raw, model) = generate_text(client, key, chain, system, &contents, &schema, hooks).await?;
    match parse_reply::<T>(&raw) {
        Ok(value) => Ok((value, model)),
        Err(error) => {
            notify_invalid(hooks, &model, 2, &error);
            contents.push(Content::model(raw));
            contents.push(Content::user(repair_instruction(&error)));
            let (repaired, repair_model) =
                generate_text(client, key, chain, system, &contents, &schema, hooks).await?;
            match parse_reply::<T>(&repaired) {
                Ok(value) => Ok((value, repair_model)),
                Err(repair_error) => {
                    // Thai/Lao slips are random per scene: a fresh request on the same fast
                    // model usually passes, and it avoids waiting on a slower model.
                    if repair_error.contains("Thai/Lao character") {
                        notify_invalid(hooks, &repair_model, 3, &repair_error);
                        let strict = format!("{system}{SCRIPT_PURITY_REMINDER}");
                        match generate_text(client, key, chain, &strict, &contents[..1], &schema, hooks).await {
                            Ok((fresh, fresh_model)) => {
                                if let Ok(value) = parse_reply::<T>(&fresh) {
                                    return Ok((value, fresh_model));
                                }
                            }
                            Err(GeminiError::Cancelled) => return Err(GeminiError::Cancelled),
                            Err(_) => {}
                        }
                    }
                    let last_error = format!("{repair_model}: {repair_error}");
                    let Some(next_chain) = chain.after(&repair_model) else {
                        return Err(GeminiError::BadResponse(last_error));
                    };
                    notify_invalid(hooks, &repair_model, 3, &repair_error);
                    contents.push(Content::model(repaired));
                    contents.push(Content::user(repair_instruction(&repair_error)));
                    let (escalated, escalated_model) = generate_text(
                        client,
                        key,
                        &next_chain,
                        system,
                        &contents,
                        &schema,
                        hooks,
                    )
                    .await?;
                    match parse_reply::<T>(&escalated) {
                        Ok(value) => Ok((value, escalated_model)),
                        Err(error) => Err(GeminiError::BadResponse(format!(
                            "{escalated_model}: {error}; previous model {last_error}"
                        ))),
                    }
                }
            }
        }
    }
}

/// Appended to the system prompt for the same-model retry after a Thai/Lao slip.
const SCRIPT_PURITY_REMINDER: &str = "\n\nCRITICAL: write every word only in the script the task requires (for Khmer: Khmer script). \
Never output Thai or Lao characters, even for words that look similar. Keep official names in Latin letters and preserve digits.";

/// Makes the otherwise silent repair / escalation steps visible in the log and the UI.
fn notify_invalid(hooks: &RetryHooks, model: &str, step: u32, error: &str) {
    hooks.notify(&RetryNotice {
        model: model.to_owned(),
        previous_model: None,
        reason: RetryReason::InvalidReply,
        attempt: step,
        max_attempts: 3,
        delay_ms: 0,
        status: None,
        error: error.chars().take(300).collect(),
    });
}

fn repair_instruction(error: &str) -> String {
    if error.contains("Thai/Lao character") {
        format!(
            "Your previous JSON was rejected: {error}. Repair the specific scene and field named in the error. Replace the Thai/Lao character with the correct spelling in the language required by the original prompt. For Khmer, write Khmer words in Khmer script; keep official names in Latin letters and preserve digits. Keep all other scene content and facts unchanged. Return the complete JSON only, matching the schema."
        )
    } else {
        format!(
            "Your previous reply was invalid: {error}. Correct only the reported problem, preserve the meaning and all valid content, and return the complete JSON only, matching the schema."
        )
    }
}

async fn generate_text(
    client: &GeminiClient,
    key: &ApiKey,
    chain: &ModelChain,
    system: &str,
    contents: &[Content],
    schema: &Value,
    hooks: &RetryHooks,
) -> Result<(String, String), GeminiError> {
    let build = |legacy: bool| GenerateContentRequest {
        contents: contents.to_vec(),
        system_instruction: Some(Content::system(system)),
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
            figures: Figures::Free,
            keep_terms: &[],
            source: None,
            real_world: false,
        }
    }

    #[test]
    fn real_world_scenes_get_visual_fields_and_rules() {
        let ctx = ChapterContext {
            title: "t",
            hook: "h",
            chapters: vec![("c1", "s1")],
            index: 0,
            scene_count: 3,
            segment_start: 0,
            chapter_total_scenes: 3,
            previous_narration: None,
            previous_shots: vec![],
            bible: BibleKeys::default(),
        };
        let schema = chapter_schema(3, &ctx.bible, false, true);
        let item = &schema["properties"]["scenes"]["items"];
        assert_eq!(item["properties"]["visual_type"]["type"], "string");
        assert!(item["required"].as_array().unwrap().iter().any(|r| r == "keywords"));
        let plain = chapter_schema(3, &ctx.bible, false, false);
        assert!(plain["properties"]["scenes"]["items"]["properties"].get("visual_type").is_none());
        let with = chapter_prompt(&ScriptBrief { real_world: true, ..brief() }, &ctx);
        assert!(with.contains("ENGLISH search phrases") && with.contains("flowchart LR") && with.contains("in Khmer"));
        assert!(!chapter_prompt(&brief(), &ctx).contains("VISUALS:"));
    }

    #[test]
    fn mermaid_templates_and_visual_validation() {
        assert_eq!(mermaid_template("flowchart LR\n A[\"x\"] --> B"), Some("flow"));
        assert_eq!(mermaid_template("%% c\ngraph TD\n A-->B"), Some("flow"));
        assert_eq!(mermaid_template("sequenceDiagram\n A->>B: hi"), Some("sequence"));
        assert_eq!(mermaid_template("timeline\n 1991 : web"), Some("timeline"));
        assert_eq!(mermaid_template("pie\n \"a\": 1"), None);
        let scene = |visual_type: &str, keywords: Vec<String>, mermaid: &str, code: &str| SceneReply {
            narration: "n".into(),
            spoken_text: "s".into(),
            image_prompt: "p".into(),
            mood: "calm".into(),
            fact_note: String::new(),
            characters: vec![],
            place: String::new(),
            shot: "wide".into(),
            key_scene: false,
            source_pages: vec![],
            general_knowledge: false,
            visual_type: visual_type.into(),
            keywords,
            mermaid: mermaid.into(),
            code_language: "bash".into(),
            code: code.into(),
            terminal: true,
        };
        let ok = |s: SceneReply| ChapterReply { scenes: vec![s] }.validate();
        assert!(ok(scene("real_photo", vec!["submarine cable".into()], "", "")).is_ok());
        assert!(ok(scene("real_photo", vec![" ".into()], "", "")).is_err());
        assert!(ok(scene("diagram", vec![], "flowchart LR\n A --> B", "")).is_ok());
        assert!(ok(scene("diagram", vec![], "A --> B", "")).is_err());
        assert!(ok(scene("code", vec![], "", "")).is_err());
        assert!(ok(scene("", vec![], "", "")).is_ok(), "non real-world replies have no visual fields");
    }

    #[test]
    fn source_projects_get_the_source_block_and_page_fields() {
        let ctx = ChapterContext {
            title: "T",
            hook: "H",
            chapters: vec![("A", "a")],
            index: 0,
            scene_count: 3,
            segment_start: 0,
            chapter_total_scenes: 3,
            previous_narration: None,
            previous_shots: vec![],
            bible: BibleKeys { characters: vec![], places: vec![] },
        };
        let block = "SOURCE DOCUMENT: \"Rice\"\nSOURCE RULES: never add facts";
        let b = ScriptBrief { source: Some(block), ..brief() };
        assert!(outline_prompt(&b, 2, 9).contains("SOURCE DOCUMENT: \"Rice\""));
        assert!(outline_prompt(&b, 2, 9).contains("Plan the chapters around the KEY POINTS"));
        let chapter = chapter_prompt(&b, &ctx);
        assert!(chapter.contains("SOURCE RULES: never add facts"));
        assert!(chapter.contains("`general_knowledge`: true only for background"));
        assert!(!outline_prompt(&brief(), 2, 9).contains("SOURCE DOCUMENT"));
        assert!(!chapter_prompt(&brief(), &ctx).contains("source_pages"));
        let with = chapter_schema(3, &ctx.bible, true, false);
        let required = with["properties"]["scenes"]["items"]["required"].as_array().unwrap();
        assert!(required.contains(&json!("source_pages")) && required.contains(&json!("general_knowledge")));
        let without = chapter_schema(3, &ctx.bible, false, false);
        assert!(without["properties"]["scenes"]["items"]["properties"]["source_pages"].is_null());
    }

    #[test]
    fn image_prompts_must_explain_the_narration() {
        let ctx = ChapterContext {
            title: "T",
            hook: "H",
            chapters: vec![("A", "a")],
            index: 0,
            scene_count: 5,
            segment_start: 0,
            chapter_total_scenes: 5,
            previous_narration: None,
            previous_shots: vec![],
            bible: BibleKeys { characters: vec![], places: vec![] },
        };
        let p = chapter_prompt(&brief(), &ctx);
        assert!(p.contains("must EXPLAIN this scene's narration"));
        assert!(p.contains("side-by-side comparison"));
        assert!(!p.contains("stick figures: show who"));
        assert!(!p.contains("ONE simple cartoon design"));
        let stick = chapter_prompt(&ScriptBrief { figures: Figures::StickFigure, ..brief() }, &ctx);
        assert!(stick.contains("stick figures: show who"));
        assert!(stick.contains("- `mood`:"), "rules are followed by the next field");
        let soft = chapter_prompt(&ScriptBrief { figures: Figures::SharedCartoon, ..brief() }, &ctx);
        assert!(soft.contains("ONE simple cartoon design"));
        assert!(!soft.contains("stick figures: show who"));
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
        // Numbers stay as digits (the app voices them: docs/04-khmer.md §4) — the model
        // must never spell years out in Khmer number words (it hallucinates them).
        assert!(p.contains("ឆ្នាំ 1969"));
        assert!(p.contains("NEVER spell"));
        assert!(!p.contains("or accompanied by the English designation"));
        assert!(!p.contains("written out in Khmer words"));
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
            segment_start: 0,
            chapter_total_scenes: 5,
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
