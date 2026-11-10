//! Script generation prompts: one outline call, then one call per chapter.
//! Replies are JSON constrained by a schema, validated with serde + [`Validate`],
//! and repaired once with targeted feedback; a second invalid response escalates to the next model.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::client::ModelChain;
use super::models::{Content, GenerateContentRequest, GenerationConfig};
use super::retry::{RetryHooks, RetryNotice, RetryReason};
use super::{GeminiClient, GeminiError};
use crate::pipeline::presets::{self, Figures};
use crate::pipeline::project::{Language, ScriptTone};
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
    /// Funny / dramatic / calm writing ([`tone_rules`]).
    pub tone: ScriptTone,
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

/// Plan step: a fact bank for the whole video and a beat sheet (one job per scene) keyed
/// `chapter_1…chapter_N`, so every scene has its own new content before any narration exists.
#[derive(Debug, Clone, Deserialize)]
pub struct PlanReply {
    pub facts: Vec<String>,
    pub chapters: HashMap<String, Vec<String>>,
}

impl PlanReply {
    /// Beats for the 0-based chapter `index` (empty when the model left it out).
    pub fn beats(&self, index: usize) -> &[String] {
        self.chapters.get(&plan_key(index)).map_or(&[], Vec::as_slice)
    }
}

fn plan_key(index: usize) -> String {
    format!("chapter_{}", index + 1)
}

/// Fact numbers beats cite (`F12`, `(F3, F4)`), 1-based, sorted, without repeats.
pub fn cited_facts<S: AsRef<str>>(beats: &[S]) -> Vec<usize> {
    let mut out = Vec::new();
    for beat in beats {
        let chars: Vec<char> = beat.as_ref().chars().collect();
        for (i, c) in chars.iter().enumerate() {
            if *c != 'F' || (i > 0 && chars[i - 1].is_alphanumeric()) {
                continue;
            }
            let digits: String = chars[i + 1..].iter().take_while(|d| d.is_ascii_digit()).collect();
            let after = chars.get(i + 1 + digits.len());
            if digits.is_empty() || after.is_some_and(|a| a.is_alphanumeric()) {
                continue;
            }
            if let Ok(n) = digits.parse::<usize>() {
                out.push(n);
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// "F3: In 1812 …" → "In 1812 …" (the prompts number the facts themselves).
pub fn strip_fact_label(fact: &str) -> &str {
    let t = fact.trim_start();
    let Some(rest) = t.strip_prefix('F') else {
        return fact;
    };
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 {
        return fact;
    }
    match rest[digits..].trim_start().strip_prefix([':', '.', ')', '-', '—']) {
        Some(r) => r.trim_start(),
        None => fact,
    }
}

impl PlanReply {
    /// The beat sheet must move forward (Napoleon script, 2026-11-03: the plan handed out
    /// F1…F40 one per scene, the whole story was told by scene 40 and the rest retold it).
    /// Every fact belongs to ONE chapter, most beats cite a fact, and every fact is used. The
    /// bank's order is not checked: [`PlanReply::in_chapter_order`] fixes it.
    fn check_structure(&self) -> Result<(), String> {
        let n_facts = self.facts.len();
        let mut owner: Vec<Option<usize>> = vec![None; n_facts];
        for index in 0..self.chapters.len() {
            let key = plan_key(index);
            let beats = self.beats(index);
            let mut with_fact = 0;
            // A fact's beats come in one run: once the plan moves on, the fact is finished.
            let mut previous: Vec<usize> = Vec::new();
            let mut finished: Vec<usize> = Vec::new();
            for beat in beats {
                let cited = cited_facts(std::slice::from_ref(beat));
                if !cited.is_empty() {
                    with_fact += 1;
                    if let Some(f) = cited.iter().find(|f| finished.contains(f)) {
                        return Err(format!(
                            "`{key}` comes back to F{f} after moving on to other facts: give each fact its beats \
in a row (event, detail, consequence) and then finish it; never cycle through the chapter's facts again. Use \
more facts instead"
                        ));
                    }
                    finished.extend(previous.iter().filter(|p| !cited.contains(p)));
                    previous.clone_from(&cited);
                }
                for f in cited {
                    let Some(slot) = f.checked_sub(1).and_then(|i| owner.get_mut(i)) else {
                        return Err(format!("`{key}` cites F{f}, but the fact bank has only {n_facts} facts"));
                    };
                    match *slot {
                        Some(other) if other != index => {
                            return Err(format!(
                                "F{f} is used in both `{}` and `{key}`: every fact belongs to ONE chapter. Later \
chapters tell their own new facts and never retell an earlier chapter's events",
                                plan_key(other)
                            ));
                        }
                        _ => *slot = Some(index),
                    }
                }
            }
            if beats.len() >= 3 && with_fact * 10 < beats.len() * 6 {
                return Err(format!(
                    "`{key}`: only {with_fact} of {} beats cite a fact (Fn). Almost every beat must develop one of \
THIS chapter's facts (several beats may develop the same fact: a detail, the result, a reaction); no filler beats",
                    beats.len()
                ));
            }
            if cited_facts(beats).is_empty() {
                return Err(format!("`{key}` cites no facts: give every chapter its own facts that fit its summary"));
            }
        }
        let unused: Vec<String> =
            owner.iter().enumerate().filter(|(_, o)| o.is_none()).map(|(i, _)| format!("F{}", i + 1)).collect();
        if !unused.is_empty() {
            return Err(format!(
                "facts {} are never used in a beat: use every fact in its chapter or remove it from the bank",
                unused.join(", ")
            ));
        }
        Ok(())
    }

    /// Facts sorted by the chapter that uses them (keeping their order inside a chapter) and
    /// every chapter's beats with the citations renumbered to match, so chapter k's facts
    /// always come after chapter k−1's. Call after validation (every fact has one chapter).
    pub fn in_chapter_order(&self) -> (Vec<String>, Vec<Vec<String>>) {
        let chapters = self.chapters.len();
        let mut owner = vec![chapters; self.facts.len()];
        for index in 0..chapters {
            for f in cited_facts(self.beats(index)) {
                if let Some(slot) = f.checked_sub(1).and_then(|i| owner.get_mut(i)) {
                    *slot = (*slot).min(index);
                }
            }
        }
        let mut order: Vec<usize> = (0..self.facts.len()).collect();
        order.sort_by_key(|&i| (owner[i], i));
        let mut new_number = vec![0; self.facts.len()];
        for (new, &old) in order.iter().enumerate() {
            new_number[old] = new + 1;
        }
        let facts = order.iter().map(|&i| self.facts[i].clone()).collect();
        let beats = (0..chapters)
            .map(|index| {
                self.beats(index)
                    .iter()
                    .map(|b| renumber_citations(b, |n| n.checked_sub(1).and_then(|i| new_number.get(i)).copied()))
                    .collect()
            })
            .collect();
        (facts, beats)
    }
}

/// Rewrites every `Fn` citation in `beat` through `map` (unknown numbers stay as they are).
fn renumber_citations(beat: &str, map: impl Fn(usize) -> Option<usize>) -> String {
    let chars: Vec<char> = beat.chars().collect();
    let mut out = String::with_capacity(beat.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let starts = c == 'F' && (i == 0 || !chars[i - 1].is_alphanumeric());
        if starts {
            let digits: String = chars[i + 1..].iter().take_while(|d| d.is_ascii_digit()).collect();
            let end = i + 1 + digits.len();
            let clean_end = chars.get(end).is_none_or(|a| !a.is_alphanumeric());
            if !digits.is_empty() && clean_end {
                let n = digits.parse::<usize>().ok().and_then(&map);
                match n {
                    Some(n) => out.push_str(&format!("F{n}")),
                    None => out.push_str(&format!("F{digits}")),
                }
                i = end;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
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
    /// Style rules worth one repair, but not worth a slower fallback model: a reply that
    /// still breaks only these after the repair is kept. Checked after `validate`.
    fn soft_check(&self) -> Result<(), String> {
        Ok(())
    }
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
        let texts = [self.title.as_str(), self.hook.as_str()]
            .into_iter()
            .chain(self.chapters.iter().flat_map(|c| [c.title.as_str(), c.summary.as_str()]));
        for text in texts {
            if let Some(c) = foreign_script(text) {
                return Err(format!(
                    "the outline contains a foreign-script character '{c}'; write it in the required language only"
                ));
            }
        }
        Ok(())
    }
}

/// Thai/Lao, Hebrew/Arabic and Vietnamese letters never belong in Khmer or English
/// narration, but lighter models sometimes mix them into Khmer words or sentences.
pub(crate) fn foreign_script(text: &str) -> Option<char> {
    text.chars().find(|c| {
        ('\u{0590}'..='\u{06FF}').contains(c)
            || ('\u{0750}'..='\u{077F}').contains(c)
            // Devanagari, Bengali, Gurmukhi, Gujarati, Tamil… Sinhala (look-alike vowel signs).
            || ('\u{0900}'..='\u{0DFF}').contains(c)
            || ('\u{0E00}'..='\u{0EFF}').contains(c)
            || ('\u{1000}'..='\u{109F}').contains(c)
            || ('\u{1EA0}'..='\u{1EF9}').contains(c)
    })
}

impl Validate for PlanReply {
    fn validate(&self) -> Result<(), String> {
        // A short source document may only give a few facts; never push the model to pad.
        if self.facts.iter().all(|f| f.trim().is_empty()) {
            return Err("`facts` needs at least one verified fact".into());
        }
        for index in 0..self.chapters.len() {
            let beats = self.beats(index);
            if beats.is_empty() || beats.iter().any(|b| b.trim().is_empty()) {
                return Err(format!("`{}` needs one non-empty beat per scene", plan_key(index)));
            }
        }
        let texts = self.facts.iter().chain(self.chapters.values().flatten());
        for text in texts {
            if let Some(c) = foreign_script(text) {
                return Err(format!(
                    "the plan contains a foreign-script character '{c}'; write facts and beats in English"
                ));
            }
        }
        self.check_structure()
    }
}

/// A `snake_case` token (Visual Bible key, variable name) leaked into spoken text.
fn code_word(text: &str) -> Option<&str> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .find(|w| w.contains('_') && w.chars().any(|c| c.is_ascii_alphabetic()) && !w.starts_with('_') && !w.ends_with('_'))
}

/// A scene that speaks to the viewer with a question (Khmer «តើ…» or a question mark).
fn asks_viewer(narration: &str) -> bool {
    narration.contains('?') || narration.contains('\u{FF1F}') || narration.contains("តើ")
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
                        "scene {} {field} contains a foreign-script character '{c}'; write it in Khmer script only",
                        i + 1
                    ));
                }
            }
            if let Some(word) = code_word(&s.narration) {
                return Err(format!(
                    "scene {} narration contains the code word '{word}' (a Visual Bible key or variable name); \
write what it means in normal words instead",
                    i + 1
                ));
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

    /// Too many questions to the viewer: one repair, then kept (Napoleon runs, 2026-11-03,
    /// failed when this rule sent chapters to busy fallback models).
    fn soft_check(&self) -> Result<(), String> {
        let questions = self.scenes.iter().filter(|s| asks_viewer(&s.narration)).count();
        let allowed = self.scenes.len() / 5 + 1;
        if questions > allowed {
            return Err(format!(
                "{questions} of {} scenes ask the viewer a question; at most {allowed} may. Rewrite the others as plain statements that tell the story",
                self.scenes.len()
            ));
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

/// One property per chapter so each beat list gets its exact scene count.
pub fn plan_schema(scene_counts: &[u32]) -> Value {
    let chapters: serde_json::Map<String, Value> = scene_counts
        .iter()
        .enumerate()
        .map(|(i, &n)| {
            (
                plan_key(i),
                json!({ "type": "array", "minItems": n, "maxItems": n, "items": { "type": "string" } }),
            )
        })
        .collect();
    let required: Vec<String> = (0..scene_counts.len()).map(plan_key).collect();
    json!({
        "type": "object",
        "properties": {
            "facts": { "type": "array", "items": { "type": "string" } },
            "chapters": { "type": "object", "properties": chapters, "required": required }
        },
        "required": ["facts", "chapters"]
    })
}

/// The beat sheet + fact bank request (`docs/03-pipeline.md` step 1, "Plan").
pub fn plan_prompt(brief: &ScriptBrief, title: &str, hook: &str, chapters: &[(&str, &str, u32)]) -> String {
    let outline: String = chapters
        .iter()
        .enumerate()
        .map(|(i, (t, s, n))| format!("chapter_{}: {t} — {s} ({n} scenes)\n", i + 1))
        .collect();
    let total: u32 = chapters.iter().map(|c| c.2).sum();
    // About one fact per 2 scenes (one per 3 left too few: the Napoleon plan of 2026-11-03 then
    // cycled through each chapter's 4 facts twice), shared out by chapter size.
    let (min_facts, max_facts) = ((total / 2).clamp(8, 60), (total * 2 / 3).clamp(12, 80));
    let per_chapter: String = chapters
        .iter()
        .enumerate()
        .map(|(i, c)| format!("chapter_{}: about {}", i + 1, (c.2 * min_facts).div_ceil(total.max(1)).max(1)))
        .collect::<Vec<_>>()
        .join(", ");
    let (source, fact_count) = match brief.source {
        Some(s) => (
            format!(
                "\n{s}\nTake every fact from the source document above, keeping its [p. …] page mark at the end \
of the fact (e.g. \"… [p. 4]\"). General background knowledge may only explain a fact, never add one. \
Use as many facts as the document gives (up to {max_facts}); never pad the list.\n"
            ),
            format!("up to {max_facts}"),
        ),
        None => (String::new(), format!("{min_facts}-{max_facts}")),
    };
    let tone = tone_rules(brief.tone, brief.language);
    format!(
        "You are planning the script of a {minutes}-minute narrated video BEFORE any narration is written. \
A professional writer first decides what every scene says, so nothing is told twice and the story always moves forward.\n\
TOPIC: {topic}\nTITLE: {title}\nHOOK: {hook}\n\
CHAPTERS:\n{outline}{source}\n\
{tone}\n{tone_beats}\n\n\
Return, in ENGLISH (the narration is written later in the video language):\n\
1. `facts`: the FACT BANK — {fact_count} short, verifiable facts this video will use, each with its \
real name, year, place or number (e.g. \"1971: Intel releases the 4004, the first commercial microprocessor\"). \
Include ONLY facts you are confident are true; never invent statistics. If an exact number is uncertain, write a \
safe approximation (\"about\", \"more than\"). Each fact appears ONCE with ONE value; no two facts may give \
different numbers for the same thing. Do not write \"F1:\" in the text; number them in your head as F1, F2, … \
in list order.\n\
   GROUP THE FACTS BY CHAPTER, in story order: first the facts that belong to chapter_1's summary, then \
chapter_2's, and so on. Each chapter gets its own block of facts that fit ITS summary ({per_chapter}); the last \
chapter's facts are the outcome and what it changed. Never put a later chapter's event into an earlier block.\n\
   If the title or hook states a popular belief that is only part of the truth (e.g. \"beaten by the winter\"), \
the bank must also hold the facts that give the fuller answer, told where they happened in the story.\n\
2. `chapters`: for every chapter key, exactly the listed number of BEATS — one beat per scene, in story order. \
A beat is one sentence saying what that scene tells, starting with the fact it develops, e.g. \"F12 — the 4004 \
fits on a fingernail yet matches a room-sized 1946 computer\". Rules:\n\
- A chapter's beats use ONLY the facts of that chapter's block, and every fact is used in its chapter. A fact \
carries 1-3 beats IN A ROW (the event, then a concrete detail or number, then the consequence or the main \
character's reaction) and is then finished: never come back to a fact after moving on to the next one, and never \
cycle through the chapter's facts a second time. Cite the fact (Fn) in every such beat.\n\
- Every beat adds ONE new piece of information or one new step of the story. No beat may repeat, rephrase or \
summarise another beat, in any chapter. A chapter never retells an earlier chapter's events and never tells a \
later chapter's events early.\n\
- chapter_1 beats 1-2 are the hook (in the TONE's style, not a summary of the video); from beat 3 the story \
starts at its true beginning. No beat previews what a later chapter reveals.\n\
- The story moves forward: cause → effect, earlier → later, simple → deeper. Each chapter's last beat pulls into \
the next chapter; the very last 2-3 beats of the video answer the hook and close it.\n\
- At most ONE beat in every 6 is a question to the viewer (write it as \"QUESTION: …\"), and only when the next \
beat answers it. All other beats are statements.\n\
- Spread the {total} beats evenly: no filler beats (\"this changed everything\", \"the world was amazed\", \
\"nature is stronger than anything\"), every beat carries real content. The closing chapter adds new facts \
(what happened next, what it changed), not repeated morals.",
        minutes = brief.target_minutes,
        topic = brief.topic,
        tone = tone.rules,
        tone_beats = tone.beats,
    )
}

/// What the chosen [`ScriptTone`] adds to the outline, plan and chapter prompts.
pub struct ToneRules {
    /// The TONE section: voice, humour, feelings, image moments.
    pub rules: String,
    pub title: &'static str,
    pub hook: &'static str,
    /// How the chapters are arranged (outline prompt).
    pub arc: &'static str,
    /// Extra beat-sheet rule (plan prompt).
    pub beats: &'static str,
}

pub fn tone_rules(tone: ScriptTone, language: Language) -> ToneRules {
    match tone {
        ScriptTone::Funny => {
            let khmer = if language == Language::Km {
                "\n- Khmer: playful spoken Khmer, the way Cambodians joke with friends. A short exclamation (e.g. «អូយ!», \
«ម៉េចក៏អញ្ចឹង?») is fine now and then, never in every scene; never vulgar or rude slang."
            } else {
                ""
            };
            ToneRules {
                rules: format!(
                    "TONE: FUNNY CARTOON EXPLAINER — a comedy history / science channel for the whole family: real facts \
told through a funny story.\n\
- A funny MAIN CHARACTER (one or two) lives the story, e.g. a clumsy caveman, a confused villager, a proud inventor. \
Refer to them only by a simple role (\"the caveman\"), never by a personal name, and keep them through the whole video. They try things, fail, react and finally get it \
right; every attempt teaches one real fact.\n\
- Humour comes from situations and reactions: things going wrong, exaggerated surprise, modern comparisons \
(\"basically the first apartment block\"), and the narrator's light, amused comments. At most one joke per scene, \
and the scene still delivers its fact. A joke never changes a fact.\n\
- Kind, clean humour for all ages: never mock real peoples, cultures, religions, bodies or disabilities; nothing crude.\n\
- Voice: a playful friend telling a funny story; simple everyday words, short sentences, amused, never dramatic. \
No doom, no \"shocking truth\", no \"the fate of the world\".\n\
- Feelings: curiosity, laughter, \"aha!\", warmth.\n\
- `image_prompt`: the characters in a comic moment that matches the narration, with big, exaggerated reactions \
(faces and body language, as far as the art style allows).{khmer}"
                ),
                title: "one short, everyday curiosity question that the video answers (e.g. \"When did humans build the \
first house?\"), asked the way a curious kid would ask it",
                hook: "1-2 sentences that drop the viewer into a funny situation of the main character that raises the \
title question (e.g. it starts to rain and the caveman has nowhere to hide).",
                arc: "a funny journey: each chapter is the next attempt or stage in answering the title question, from \
the earliest to the latest, ending with a warm, funny payoff that answers the question in one clear line",
                beats: "- FUNNY beats: name what the main character does or suffers in that scene and the real fact it \
shows (e.g. \"Caveman's branch roof collapses in the rain — F2: early shelters were simple branch huts\").",
            }
        }
        ScriptTone::Dramatic => ToneRules {
            rules: "TONE: DRAMATIC STORYTELLING — a high-retention 'why / history / science / technology' channel.\n\
- NARRATIVE TENSION: find the conflict, contrast or hidden twist and make it the spine: old belief vs surprising \
reality, underdog vs giant, or a step-by-step revelation that keeps building.\n\
- Voice: a charismatic storyteller; short sentences that hit hard; fast facts, then a slow reveal.\n\
- Feelings: awe → curiosity → tension → surprise → satisfaction.\n\
- `image_prompt`: cinematic, emotional moments that match the narration."
                .to_owned(),
            title: "a compelling, curiosity-inducing title that frames a fascinating debate, mystery or comparison",
            hook: "1-2 sentences for the first 10 seconds: contrast a common belief with a surprising reality.",
            arc: "a dramatic narrative arc with escalating tension, ending in a resonant conclusion that resolves the \
central tension",
            beats: "",
        },
        ScriptTone::Calm => ToneRules {
            rules: "TONE: CALM DOCUMENTARY — a clear, warm narrator, like a good teacher or a nature documentary.\n\
- The spine is one clear question, answered step by step in a logical order.\n\
- Voice: calm, friendly and precise; simple sentences; curiosity without hype. No jokes, no shock words, no \
cliffhanger drama; chapters link with a gentle \"next\".\n\
- Feelings: curiosity, understanding, quiet wonder.\n\
- `image_prompt`: clear, explanatory scenes that show exactly what the narration describes."
                .to_owned(),
            title: "a clear, inviting title that says what the viewer will understand",
            hook: "1-2 sentences: an interesting observation or question that makes the viewer want to understand the topic.",
            arc: "a clear learning path from the basics to the deeper ideas, ending with a short, calm answer to the \
opening question",
            beats: "",
        },
    }
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
- Write `narration` in natural, CONVERSATIONAL spoken Khmer — the way a charismatic Cambodian storyteller talks to a friend, NOT a literal translation from English. Use vivid, everyday words. Short punchy sentences for impact. Vary the rhythm: fast facts, then a slow dramatic reveal. NO formal or academic language.\n\
- End sentences with ។ . Do not insert zero-width spaces.\n\
- QUESTIONS in `narration` are rare (at most one per 6 scenes); a real question ends with ។? or ? so the TTS voice rises. Everything else is a plain statement. Do not open scenes with «តើអ្នកដឹងទេថា» / «តើអ្នកអាចស្រមៃ» over and over.\n\
- PACING in `spoken_text`: use commas `,` for natural breath pauses and short sentences. `...` (three dots) only for the rare dramatic pause before a major reveal (at most once every 10 scenes). NEVER put `...` in `narration`.\n\
- Write ONLY Khmer script, Latin letters (names/terms) and digits. Never use Thai, Lao, Arabic, Vietnamese or any other script, and no random English filler words in the middle of Khmer sentences.\n\
- Avoid filler intensifiers (យក្ស, មហាសាល, ដ៏គួរឱ្យរន្ធត់, ដ៏អស្ចារ្យ) unless they carry real meaning; prefer one precise fact.\n\
- Write EVERY number with DIGITS (0-9), in BOTH `narration` and `spoken_text`: years `ឆ្នាំ 1969`, quantities `1,500 នាក់`, decimals `3.5`, percentages `30%`. NEVER spell a number or a year out in Khmer number words (write `ឆ្នាំ 1863`, never `ឆ្នាំមួយពាន់ប្រាំបីរយ…`): spelled-out Khmer numbers usually come out wrong, while the app reads the digits in correct Khmer itself.\n\
- `spoken_text` is the same content written exactly as it must be SPOKEN: write units in Khmer words (km → គីឡូម៉ែត្រ) and translate English abbreviations (BC → មុនគ្រិស្តសករាជ, AD → គ្រិស្តសករាជ). Foreign names of people, places, missions, and technical terms stay in Latin letters in BOTH fields (see names rule below).\n\
- `title` and chapter titles in Khmer. `fact_note` in Khmer."
        }
        Language::En => {
            "LANGUAGE: English.\n\
- Write `narration` in clear, warm, CONVERSATIONAL spoken English — the way a passionate, brilliant friend explains something amazing. Short punchy sentences. Vivid everyday language. Vary the rhythm: fast facts, then a slow reveal. NO formal or academic language.\n\
- QUESTIONS in `narration` are rare (at most one per 6 scenes); a real question ends with ? so the TTS voice rises. Everything else is a plain statement.\n\
- PACING in `spoken_text`: use commas `,` for natural breath pauses and short sentences. `...` only for the rare dramatic pause before a major reveal (at most once every 10 scenes). NEVER put `...` in `narration`.\n\
- `spoken_text` is the same content written exactly as it must be SPOKEN: numbers, years, units and abbreviations written out as words (1863 → eighteen sixty-three; 5 km → five kilometers; 30% → thirty percent; BC → B.C.). No digits may remain in `spoken_text`."
        }
    }
}

const SYSTEM_PROMPT: &str = "You are the head writer of a top-performing faceless YouTube explainer channel \
in the 'why / history / science / technology' style — the kind where viewers leave comments like \
\"I watched this 3 times\" or \"I couldn't stop watching\". \
You write narration for one storyteller voice over a sequence of illustrations (one per scene, 6-8 seconds each).\n\
\n\
YOUR WRITING METHOD — follow every rule in every video:\n\
\n\
1. STORY SPINE — every video follows one clear thread (a question, a problem, a journey or a twist, as the TONE \
section of the task describes). NEVER write a flat list of facts.\n\
\n\
2. THE HOOK (scenes 1-2) — grab the viewer in the way the TONE section describes. \
NEVER open with a definition, background, or \"In this video we will...\"\n\
\n\
3. TONE — the TONE section of the task decides the voice, the humour and the feelings; follow it in every scene. \
Always conversational, the way a person talks, never the way a textbook reads.\n\
\n\
4. RHETORICAL QUESTIONS ARE RARE — at most ONE question per 6 scenes, only right before a real reveal that the \
next scene answers. Most scenes are plain, confident statements that TELL the story. Never ask a question you do \
not answer in the next 1-2 scenes, never ask two questions in a row, and never repeat the same question pattern \
(\"Did you know...?\", \"Can you imagine...?\"). End real questions with a '?'.\n\
\n\
5. FLOW — each scene continues the previous one with a natural link (cause and effect, \"so\", \"but\", \"then\", a \
time jump). Only a chapter's LAST scene needs a cliffhanger. NEVER end scenes with a bland summary like \
\"This was an important moment.\"\n\
\n\
6. SPOKEN_TEXT PACING — in `spoken_text`, use commas `,` for breath pauses and break long sentences into short \
ones. Use the ellipsis `...` at most once every 10 scenes, only before a major reveal. `narration` (the subtitle) \
never contains `...`.\n\
\n\
7. FEELING — every chapter makes the viewer FEEL something (the TONE section says what), not just learn facts.\n\
\n\
8. TRUE, CONCRETE FACTS — anchor claims with a specific year, name, place or mechanism, but ONLY facts you are \
confident are real. NEVER invent statistics, quotes, dates or names to sound precise. If you are unsure of an exact \
figure, use a safe approximation (\"more than half\", \"about 90%\") or explain the mechanism instead, and say so in \
`fact_note`. A figure that appears more than once (a total, a price, a share) must be the SAME number every time. \
Never write vague filler (\"an unbelievable amount\", \"a huge number of things\").\n\
\n\
9. NO TRAILERS, NO REPEATS — the video moves forward. Never preview, summarise or name events of later chapters \
(the first chapter is the START of the story, not a trailer of the whole video), and never retell a fact, example or \
idea that an earlier scene already covered. Every scene adds one NEW piece of information or one new step.\n\
\n\
10. CHAPTER ENDINGS — a chapter's last scene pulls the viewer into the next chapter. The final chapter answers \
the hook with a short conclusion built on what the video showed — no new claims, no list of hypothetical disasters.\n\
\n\
11. NO INVENTED PEOPLE — the only people named are REAL people who are part of this topic's true story, with \
their real role. NEVER create a fictional named character, guide, friend, relative (\"Uncle ...\", \"Aunt ...\"), \
student or \"someone called ...\", and never give the narrator a name or a family role. This is true in every \
language: do not slip in a local-sounding name (for example a Khmer name in a video about another country). When a \
scene needs a person, use a generic role (\"a farmer\", \"a factory worker\", \"the minister\") or speak to the viewer \
as \"you\". If the viewer would have to ask \"who is that?\", the name must not be there.\n\
\n\
When a claim is uncertain, disputed or a simplification, say so in that scene's `fact_note` (otherwise leave it empty). \
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
    let tone = tone_rules(brief.tone, brief.language);
    format!(
        "Plan a {minutes}-minute high-retention narrated explainer video.\n\
TOPIC: {topic}\n\
VISUAL STYLE (for your information only): {style}\n\
{tone_rules}\n\
{rules}\n{source}\n\
Return:\n\
- `title`: {tone_title}.\n\
- `hook`: {tone_hook} It must tease, not summarise the video.\n\
- `chapters`: exactly {chapter_count} chapters structured as {tone_arc}:\n\
  * Chapter 1 opens the story at its true beginning (the first event, person or problem). It is NOT a preview or trailer of the whole video.\n\
  * Middle chapters follow the story forward: turning points, unseen mechanisms, conflicts, discoveries.\n\
  * Each chapter covers a distinct phase with its own events, examples and numbers: no two chapters may share a subject, example or statistic, and no chapter may mention what a later chapter reveals.\n\
  * Each chapter needs a short `title` and a fact-specific `summary` listing the 3-5 concrete, verifiable facts/events (real names, years, places) that chapter will cover, and its closing tension hook. Use only facts you are confident are true. Never invent a named person: name only real people who belong to the topic.\n\
The video will have about {scene_count} scenes in total, spread evenly across the chapters.",
        minutes = brief.target_minutes,
        topic = brief.topic,
        style = brief.style_label,
        tone_rules = tone.rules,
        tone_title = tone.title,
        tone_hook = tone.hook,
        tone_arc = tone.arc,
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
    /// This request's beats from the plan step, one per scene (empty for older scripts).
    pub beats: &'a [String],
    /// The whole video's fact bank (empty for older scripts).
    pub facts: &'a [String],
    /// 1-based fact numbers the plan gives this whole chapter (empty = no limit).
    pub chapter_facts: &'a [usize],
}

/// The scene plan and fact bank block of a chapter prompt ("" without a plan).
fn plan_section(ctx: &ChapterContext) -> String {
    let mut out = String::new();
    if !ctx.facts.is_empty() {
        out.push_str(
            "FACT BANK (the only figures, dates and names you may state; copy every number exactly, never change it; \
a [p. …] mark gives the `source_pages` of the scene that uses the fact):\n",
        );
        for (i, f) in ctx.facts.iter().enumerate() {
            out.push_str(&format!("F{}. {}\n", i + 1, strip_fact_label(f).trim()));
        }
        if !ctx.chapter_facts.is_empty() {
            let list: Vec<String> = ctx.chapter_facts.iter().map(|n| format!("F{n}")).collect();
            out.push_str(&format!(
                "THIS CHAPTER'S FACTS: {}. Every other fact belongs to another chapter: earlier chapters already \
told theirs, later chapters will tell theirs. Never state them here, not even as a summary or a preview.\n",
                list.join(", ")
            ));
        }
    }
    if !ctx.beats.is_empty() {
        out.push_str(
            "SCENE PLAN — scene k tells beat k, in this order. Tell ONLY that beat (use the facts it cites); \
do not add other beats' facts, do not jump ahead in the story and do not retell earlier beats. A beat marked QUESTION is the only scene \
that may ask the viewer a question:\n",
        );
        for (i, b) in ctx.beats.iter().enumerate() {
            out.push_str(&format!("Scene {}: {}\n", i + 1, b.trim()));
        }
        if ctx.beats.len() < ctx.scene_count {
            out.push_str("Scenes after the plan continue with the next new step of this chapter.\n");
        }
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out
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
accents and what they are doing, never through faces or bodies. The main stick man is always the same \
character: show his feeling with the eyebrows, eyes and smile (curious, surprised, worried, proud) and a \
clear pose that matches the narration. Because a stick man carries almost no detail, the TOPIC must be carried \
by the props and the setting: STRICT TOPIC RELEVANCE — every image_prompt names the exact subject of its \
narration (the specific object, machine, place, person, animal, event or number it talks about) and at least \
two concrete props or setting details that belong only to this video's topic and era; the stick man interacts \
with that subject. Self-check each prompt: if the same picture could illustrate a different video (a man at a \
desk, a crowd, a generic city), it is wrong — rewrite it around the narration's own subject. "
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
        "This is the FIRST chapter: scenes 1–2 must deliver the hook — open with the surprising contrast or fact so the viewer keeps watching. From scene 3 on, START THE STORY at its beginning and stay inside this chapter's focus: do not preview or list what later chapters will reveal."
    } else if number == total && is_chapter_end {
        "This is the LAST chapter: end the video with a short, satisfying conclusion that answers the hook using what the video already showed. No new claims and no list of hypothetical disasters."
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
{plan}\
{tone}\n\n\
STORYTELLING CRAFT (apply every rule to every scene):\n\
1. TONE: follow the TONE section above in every scene; it decides the voice, the humour and the feelings. NEVER write a flat sequence of facts.\n\
2. SPOKEN WORDS: write each `narration` the way a person SPEAKS, not writes: short sentences, vivid everyday words, no academic or formal language.\n\
3. QUESTIONS ARE RARE: at most ONE question per 6 scenes, placed right before a reveal that the next scene answers. Most scenes are plain statements that tell the story. Never two questions in a row; never reuse the same question pattern.\n\
4. FLOW: each scene continues the last one (cause, effect, contrast, time jump). Only this chapter's LAST scene needs a cliffhanger. NEVER end a scene with a bland summary like 'This was an important moment.'.\n\
5. FEELING: every scene makes the viewer feel what the TONE section asks for, not just learn a fact.\n\
6. TRUE FACTS: anchor claims with a real year, name, place or mechanism, but NEVER invent statistics, quotes or dates. If unsure of an exact figure, use a safe approximation or explain the mechanism, and flag it in `fact_note`. A number used before must stay identical. No vague filler.\n\
7. SCENE PROGRESSION: each scene delivers ONE new fact or narrative beat. Never repeat an event, example, statistic or idea already covered (see ALREADY COVERED), and never preview events of later chapters.\n\
8. No `...` in `narration`; at most one in `spoken_text` per 10 scenes.\n\
9. NO INVENTED PEOPLE: name only real people who belong to this topic's true story. Never invent a named character, guide, friend or relative (no \"Uncle ...\", no made-up names in any language), and never name the narrator; use a generic role (\"a farmer\") or speak to the viewer as \"you\".\n\n\
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
        plan = plan_section(ctx),
        tone = tone_rules(brief.tone, brief.language).rules,
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
                    // Only a soft rule still broken: keep the reply rather than wait on a
                    // slower (often busy) fallback model.
                    if let Ok(value) = parse_lenient::<T>(&repaired) {
                        return Ok((value, repair_model));
                    }
                    // foreign-script slips are random per scene: a fresh request on the same fast
                    // model usually passes, and it avoids waiting on a slower model.
                    if repair_error.contains("foreign-script character") {
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

/// Appended to the system prompt for the same-model retry after a foreign-script slip.
const SCRIPT_PURITY_REMINDER: &str = "\n\nCRITICAL: write every word only in the script the task requires (for Khmer: Khmer script). \
Never output Thai, Lao, Myanmar, Hindi, Punjabi, Arabic or Vietnamese characters, even for words that look similar. Keep official names in Latin letters and preserve digits.";

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
    if error.contains("foreign-script character") {
        format!(
            "Your previous JSON was rejected: {error}. Repair the specific scene and field named in the error. Replace the foreign-script character with the correct spelling in the language required by the original prompt. For Khmer, write Khmer words in Khmer script; keep official names in Latin letters and preserve digits. Keep all other scene content and facts unchanged. Return the complete JSON only, matching the schema."
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
    let value: T = parse_lenient(raw)?;
    value.soft_check()?;
    Ok(value)
}

/// [`parse_reply`] without [`Validate::soft_check`].
fn parse_lenient<T: DeserializeOwned + Validate>(raw: &str) -> Result<T, String> {
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
            tone: ScriptTone::Dramatic,
        }
    }

    #[test]
    fn tone_changes_every_prompt() {
        let funny = ScriptBrief { tone: ScriptTone::Funny, ..brief() };
        let outline = outline_prompt(&funny, 3, 15);
        assert!(outline.contains("FUNNY CARTOON EXPLAINER"));
        assert!(outline.contains("everyday curiosity question"));
        assert!(outline.contains("«អូយ!»"), "Khmer projects get the Khmer humour line");
        assert!(!outline.contains("DRAMATIC STORYTELLING"));
        let plan = plan_prompt(&funny, "T", "H", &[("A", "a", 3)]);
        assert!(plan.contains("FUNNY beats"));
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
            bible: BibleKeys::default(),
            beats: &[],
            facts: &[],
            chapter_facts: &[],
        };
        assert!(chapter_prompt(&funny, &ctx).contains("MAIN CHARACTER"));
        assert!(chapter_prompt(&funny, &ctx).contains("NO INVENTED PEOPLE"));
        assert!(SYSTEM_PROMPT.contains("NO INVENTED PEOPLE"));
        assert!(chapter_prompt(&brief(), &ctx).contains("DRAMATIC STORYTELLING"));
        let calm = ScriptBrief { tone: ScriptTone::Calm, language: Language::En, ..brief() };
        let calm_outline = outline_prompt(&calm, 3, 15);
        assert!(calm_outline.contains("CALM DOCUMENTARY"));
        assert!(!calm_outline.contains("«អូយ!»"));
        assert!(!SYSTEM_PROMPT.contains("shocking"), "the shared rules stay tone-neutral");
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
            beats: &[],
            facts: &[],
            chapter_facts: &[],
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
            beats: &[],
            facts: &[],
            chapter_facts: &[],
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
            beats: &[],
            facts: &[],
            chapter_facts: &[],
        };
        let p = chapter_prompt(&brief(), &ctx);
        assert!(p.contains("must EXPLAIN this scene's narration"));
        assert!(p.contains("side-by-side comparison"));
        assert!(!p.contains("stick figures: show who"));
        assert!(!p.contains("ONE simple cartoon design"));
        let stick = chapter_prompt(&ScriptBrief { figures: Figures::StickFigure, ..brief() }, &ctx);
        assert!(stick.contains("stick figures: show who"));
        assert!(stick.contains("STRICT TOPIC RELEVANCE"));
        assert!(!p.contains("STRICT TOPIC RELEVANCE"));
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
        assert!(err.contains("foreign-script"), "{err}");
        let ok = r#"{"scenes":[{"narration":"ញើសឈាមរបស់បុព្វបុរស","spoken_text":"x","image_prompt":"y","mood":"calm","fact_note":""}]}"#;
        assert!(parse_reply::<ChapterReply>(ok).is_ok());
    }

    #[test]
    fn plan_schema_fixes_each_chapters_beat_count() {
        let schema = plan_schema(&[3, 4]);
        let chapters = &schema["properties"]["chapters"];
        assert_eq!(chapters["properties"]["chapter_1"]["minItems"], 3);
        assert_eq!(chapters["properties"]["chapter_2"]["maxItems"], 4);
        assert_eq!(chapters["required"][1], "chapter_2");
        let p = plan_prompt(&brief(), "T", "H", &[("A", "a", 3), ("B", "b", 4)]);
        assert!(p.contains("chapter_2: B — b (4 scenes)"));
        assert!(p.contains("FACT BANK — 8-12 short"));
        let doc = plan_prompt(&ScriptBrief { source: Some("SOURCE DOCUMENT: \"X\""), ..brief() }, "T", "H", &[("A", "a", 3)]);
        assert!(doc.contains("SOURCE DOCUMENT: \"X\""));
        assert!(doc.contains("[p. …] page mark"));
        assert!(doc.contains("FACT BANK — up to 12 short"));
    }

    #[test]
    fn plan_reply_needs_facts_and_every_chapters_beats() {
        let ok = r#"{"facts":["a","b","c"],"chapters":{"chapter_1":["F1 x","F2 y"],"chapter_2":["F3 z"]}}"#;
        let plan: PlanReply = parse_reply(ok).unwrap();
        assert_eq!(plan.beats(1), ["F3 z".to_owned()]);
        assert!(plan.beats(5).is_empty());
        let none = r#"{"facts":[" "],"chapters":{"chapter_1":["x"]}}"#;
        assert!(parse_reply::<PlanReply>(none).unwrap_err().contains("facts"));
        assert!(parse_reply::<PlanReply>(r#"{"facts":["a"],"chapters":{"chapter_1":["F1 x"]}}"#).is_ok());
        let gap = r#"{"facts":["a","b","c"],"chapters":{"chapter_1":["F1 x"],"chapter_3":["F2 y"]}}"#;
        assert!(parse_reply::<PlanReply>(gap).unwrap_err().contains("chapter_2"));
    }

    fn plan(facts: usize, chapters: &[&[&str]]) -> Result<PlanReply, String> {
        let facts: Vec<String> = (1..=facts).map(|i| format!("fact {i}")).collect();
        let chapters: serde_json::Map<String, Value> = chapters
            .iter()
            .enumerate()
            .map(|(i, beats)| (plan_key(i), json!(beats)))
            .collect();
        parse_reply::<PlanReply>(&json!({ "facts": facts, "chapters": chapters }).to_string())
    }

    #[test]
    fn plan_facts_move_forward_one_chapter_each() {
        // Good: each chapter has its own block, facts carry several beats, a question is allowed.
        assert!(plan(4, &[&["F1 a", "F1 b", "F2 c", "QUESTION: why?"], &["F3 d", "F3 e", "F4 f"]]).is_ok());
        // The Napoleon failure: the climax facts told again in a later chapter.
        let retold = plan(3, &[&["F1 a", "F2 b", "F3 c"], &["F3 again"]]).unwrap_err();
        assert!(retold.contains("F3 is used in both `chapter_1` and `chapter_2`"), "{retold}");
        // Out of bank order is accepted: `in_chapter_order` fixes it.
        let mixed = plan(4, &[&["F1 a", "F3 b"], &["F2 c", "F4 d"]]).unwrap();
        let (facts, beats) = mixed.in_chapter_order();
        assert_eq!(facts, ["fact 1", "fact 3", "fact 2", "fact 4"]);
        assert_eq!(beats, [vec!["F1 a", "F2 b"], vec!["F3 c", "F4 d"]]);
        // The second Napoleon failure: the chapter cycles through its facts again.
        let cycle = plan(2, &[&["F1 a", "F2 b", "F1 c", "F2 d"]]).unwrap_err();
        assert!(cycle.contains("comes back to F1"), "{cycle}");
        // A beat may carry two facts; a question in between does not break a run.
        assert!(plan(3, &[&["F1 a", "F1, F2 b", "QUESTION: why?", "F2 c", "F3 d"]]).is_ok());
        // Filler: most beats cite nothing.
        let filler = plan(1, &[&["F1 a", "nature wins", "winter wins", "the end"]]).unwrap_err();
        assert!(filler.contains("only 1 of 4 beats"), "{filler}");
        // A chapter without facts, an unknown fact, an unused fact.
        assert!(plan(1, &[&["F1 a"], &["moral"]]).unwrap_err().contains("cites no facts"));
        assert!(plan(1, &[&["F1 a", "F7 b"]]).unwrap_err().contains("only 1 facts"));
        assert!(plan(3, &[&["F1 a", "F2 b"]]).unwrap_err().contains("F3 are never used"));
    }

    #[test]
    fn too_many_questions_is_a_soft_rule() {
        let scene = |n: &str| format!(r#"{{"narration":"{n}","spoken_text":"x","image_prompt":"y","mood":"calm","fact_note":""}}"#);
        let raw = format!(r#"{{"scenes":[{},{}]}}"#, scene("តើហេតុអ្វី?"), scene("ហេតុអ្វី?"));
        assert!(parse_reply::<ChapterReply>(&raw).unwrap_err().contains("ask the viewer a question"));
        assert!(parse_lenient::<ChapterReply>(&raw).is_ok());
        // Hard rules stay hard.
        let raw = format!(r#"{{"scenes":[{}]}}"#, scene("សេះ russian_steppe"));
        assert!(parse_lenient::<ChapterReply>(&raw).is_err());
    }

    #[test]
    fn renumbers_citations_only() {
        let map = |n: usize| (n == 3).then_some(7);
        assert_eq!(renumber_citations("F3 — x (F3, F12) FIFA F3a", map), "F7 — x (F7, F12) FIFA F3a");
    }

    #[test]
    fn code_words_in_narration_are_rejected() {
        assert_eq!(code_word("វាលស្មៅ frozen russian_steppe ត្រជាក់"), Some("russian_steppe"));
        assert_eq!(code_word("Napoleon 1812 snake_"), None);
        let raw = r#"{"scenes":[{"narration":"សេះ russian_steppe","spoken_text":"x","image_prompt":"y","mood":"calm","fact_note":""}]}"#;
        assert!(parse_reply::<ChapterReply>(raw).unwrap_err().contains("code word 'russian_steppe'"));
    }

    #[test]
    fn reads_fact_citations_and_strips_fact_labels() {
        assert_eq!(cited_facts(&["F12 — x (F3, F4)", "F3/F1: y", "QUESTION: why?"]), vec![1, 3, 4, 12]);
        // Not citations: words, model names, a lone F.
        assert!(cited_facts(&["FIFA and F-16 and F", "the 3F12 code", "F1a"]).is_empty());
        assert_eq!(strip_fact_label("F3: In 1812 …"), "In 1812 …");
        assert_eq!(strip_fact_label("F12 — x"), "x");
        assert_eq!(strip_fact_label("Fire in Moscow"), "Fire in Moscow");
        assert_eq!(strip_fact_label("F1 racing cars"), "F1 racing cars");
    }

    #[test]
    fn chapter_prompt_lists_the_beats_and_fact_bank() {
        let beats = vec!["F1 — the first chip".to_owned(), "QUESTION: why so small?".to_owned()];
        let facts = vec!["1958: Jack Kilby builds the first integrated circuit".to_owned()];
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
            bible: BibleKeys::default(),
            beats: &beats,
            facts: &facts,
            chapter_facts: &[],
        };
        let p = chapter_prompt(&brief(), &ctx);
        assert!(p.contains("F1. 1958: Jack Kilby"));
        assert!(!p.contains("THIS CHAPTER'S FACTS"));
        let own = ChapterContext { chapter_facts: &[1, 2], ..ctx };
        assert!(chapter_prompt(&brief(), &own).contains("THIS CHAPTER'S FACTS: F1, F2."));
        assert!(p.contains("Scene 2: QUESTION: why so small?"));
        assert!(p.contains("Scenes after the plan continue"));
        let none = ChapterContext { beats: &[], facts: &[], chapter_facts: &[], ..own };
        assert!(!chapter_prompt(&brief(), &none).contains("SCENE PLAN"));
    }

    #[test]
    fn rejects_arabic_and_vietnamese_letters() {
        // …and Gurmukhi / Devanagari / Myanmar vowel signs that look like Khmer ones.
        for bad in ["សំឡេង سرم", "AI bắt đầu", "ពੀរ", "អਤិមា", "ក ि", "ក ာ"] {
            let raw = format!(
                r#"{{"scenes":[{{"narration":"{bad}","spoken_text":"x","image_prompt":"y","mood":"calm","fact_note":""}}]}}"#
            );
            let err = parse_reply::<ChapterReply>(&raw).unwrap_err();
            assert!(err.contains("foreign-script"), "{err}");
        }
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
            beats: &[],
            facts: &[],
            chapter_facts: &[],
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
