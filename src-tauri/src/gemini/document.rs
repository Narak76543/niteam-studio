//! Reading a document (`docs/10-document-source.md` §3–4): section notes per chunk (PDF pages
//! or extracted text), then one text-only call that turns the notes into the Source summary,
//! and the source block the script prompts get. Request: `generateContent` with an inline
//! `application/pdf` part + JSON schema (live-checked 2026-10-17 on a free key).

use serde::Deserialize;
use serde_json::{Value, json};

use super::script::Validate;
use crate::pipeline::project::{Audience, Language, SourceSection, SourceSummary};

pub const READER_SYSTEM: &str = "You read documents (papers, articles, reports) for a team that turns \
them into narrated explainer videos. Be strictly faithful to the document: report only what it says, keep \
every number with its unit, name what is a result, a method, a definition or an opinion, and never add \
outside knowledge. Cite the PDF page numbers the information is on. Reply with JSON only, matching the schema.";

const MAX_SECTIONS: usize = 30;
const MAX_POINTS: usize = 12;
const MAX_FACTS: usize = 15;
const MAX_TERMS: usize = 12;
/// Section notes passed to the script prompts (the summary is the main input).
const MAX_SOURCE_NOTES_CHARS: usize = 12_000;

#[derive(Debug, Deserialize)]
pub struct ChunkReply {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub authors: String,
    #[serde(default)]
    pub year: String,
    pub sections: Vec<SectionReply>,
}

#[derive(Debug, Deserialize)]
pub struct SectionReply {
    pub heading: String,
    pub notes: String,
    pub pages: Vec<u32>,
}

impl Validate for ChunkReply {
    fn validate(&self) -> Result<(), String> {
        if self.sections.is_empty() {
            return Err("`sections` must not be empty".into());
        }
        if self.sections.len() > MAX_SECTIONS {
            return Err(format!("at most {MAX_SECTIONS} sections"));
        }
        if self.sections.iter().any(|s| s.notes.trim().is_empty()) {
            return Err("every section needs notes".into());
        }
        Ok(())
    }
}

impl ChunkReply {
    /// Sections with their pages limited to the chunk (a section without a valid page gets
    /// the chunk's first page, so it can still be cited).
    pub fn into_sections(self, from: u32, to: u32) -> Vec<SourceSection> {
        self.sections
            .into_iter()
            .map(|s| {
                let mut pages: Vec<u32> = s.pages.into_iter().filter(|p| (from..=to).contains(p)).collect();
                pages.sort_unstable();
                pages.dedup();
                if pages.is_empty() {
                    pages.push(from);
                }
                SourceSection { heading: s.heading.trim().to_owned(), notes: s.notes.trim().to_owned(), pages }
            })
            .collect()
    }
}

pub fn chunk_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "title": { "type": "string", "description": "the document's title if these pages show it, else \"\"" },
            "authors": { "type": "string", "description": "authors or organisation if shown, else \"\"" },
            "year": { "type": "string", "description": "publication year if shown, else \"\"" },
            "sections": {
                "type": "array",
                "maxItems": MAX_SECTIONS,
                "items": {
                    "type": "object",
                    "properties": {
                        "heading": { "type": "string" },
                        "notes": { "type": "string" },
                        "pages": { "type": "array", "items": { "type": "integer" } }
                    },
                    "required": ["heading", "notes", "pages"]
                }
            }
        },
        "required": ["title", "authors", "year", "sections"]
    })
}

fn chunk_rules(from: u32, to: u32) -> String {
    format!(
        "Write notes for every section of pages {from}–{to}, in ENGLISH, in reading order (one entry per \
section or sub-section; split long sections). Each `notes` is 40–150 words: the claims, results, methods and \
definitions with every number, unit, date, name and comparison exactly as written; say \"the authors argue\" \
for opinions and keep limitations the document states. Tables and figures: give their key numbers. Skip \
references lists, acknowledgements and page furniture. `pages`: the page numbers ({from}–{to}) the notes come from."
    )
}

/// First turn for a chunk sent as PDF pages.
pub fn chunk_prompt_pdf(from: u32, to: u32) -> String {
    format!(
        "The attached PDF holds pages {from}–{to} of a longer document: its first page is page {from}, so \
number pages from there.\n{}",
        chunk_rules(from, to)
    )
}

/// First turn for a chunk sent as its extracted text (`=== Page N ===` markers).
pub fn chunk_prompt_text(from: u32, to: u32, pages: &[String]) -> String {
    let body: String = pages
        .iter()
        .zip(from..=to)
        .map(|(text, n)| format!("=== Page {n} ===\n{}\n", text.trim()))
        .collect();
    format!(
        "Below is the text of pages {from}–{to} of a document, extracted from its PDF (layout lost; \
tables may be flattened).\n{}\n\nDOCUMENT TEXT:\n{body}",
        chunk_rules(from, to)
    )
}

#[derive(Debug, Deserialize)]
pub struct SummaryReply {
    pub title: String,
    #[serde(default)]
    pub authors: String,
    #[serde(default)]
    pub year: String,
    pub key_points: Vec<ItemReply>,
    #[serde(default)]
    pub facts: Vec<ItemReply>,
    #[serde(default)]
    pub terms: Vec<TermReply>,
}

#[derive(Debug, Deserialize)]
pub struct ItemReply {
    pub text: String,
    pub pages: Vec<u32>,
}

#[derive(Debug, Deserialize)]
pub struct TermReply {
    pub term: String,
    pub explanation: String,
    pub pages: Vec<u32>,
}

impl Validate for SummaryReply {
    fn validate(&self) -> Result<(), String> {
        if self.title.trim().is_empty() {
            return Err("`title` must not be empty".into());
        }
        if self.key_points.is_empty() || self.key_points.len() > MAX_POINTS {
            return Err(format!("give 1–{MAX_POINTS} key points"));
        }
        let texts = self.key_points.iter().chain(&self.facts).map(|i| i.text.as_str());
        if texts.chain(self.terms.iter().map(|t| t.term.as_str())).any(|t| t.trim().is_empty()) {
            return Err("every item needs text".into());
        }
        Ok(())
    }
}

pub fn summary_schema() -> Value {
    let item = json!({
        "type": "object",
        "properties": {
            "text": { "type": "string" },
            "pages": { "type": "array", "items": { "type": "integer" } }
        },
        "required": ["text", "pages"]
    });
    json!({
        "type": "object",
        "properties": {
            "title": { "type": "string" },
            "authors": { "type": "string" },
            "year": { "type": "string" },
            "key_points": { "type": "array", "minItems": 1, "maxItems": MAX_POINTS, "items": item },
            "facts": { "type": "array", "maxItems": MAX_FACTS, "items": item },
            "terms": {
                "type": "array",
                "maxItems": MAX_TERMS,
                "items": {
                    "type": "object",
                    "properties": {
                        "term": { "type": "string" },
                        "explanation": { "type": "string" },
                        "pages": { "type": "array", "items": { "type": "integer" } }
                    },
                    "required": ["term", "explanation", "pages"]
                }
            }
        },
        "required": ["title", "authors", "year", "key_points", "facts", "terms"]
    })
}

pub struct SummaryInput<'a> {
    pub file_name: &'a str,
    /// Title / authors / year seen on the pages ("" if not found).
    pub title: &'a str,
    pub authors: &'a str,
    pub year: &'a str,
    pub sections: &'a [SourceSection],
    pub language: Language,
    pub audience: Audience,
    pub focus: &'a str,
    pub target_minutes: u32,
}

fn pages_text(pages: &[u32]) -> String {
    pages.iter().map(u32::to_string).collect::<Vec<_>>().join(", ")
}

fn notes_list(sections: &[SourceSection]) -> String {
    sections.iter().map(|s| format!("- [p. {}] {}: {}\n", pages_text(&s.pages), s.heading, s.notes)).collect()
}

fn language_name(l: Language) -> &'static str {
    match l {
        Language::Km => "KHMER",
        Language::En => "ENGLISH",
    }
}

pub fn summary_prompt(i: &SummaryInput) -> String {
    let focus = if i.focus.trim().is_empty() {
        String::new()
    } else {
        format!("The user's focus: \"{}\". Pick points that serve it; leave out the rest.\n", i.focus.trim())
    };
    let khmer = if i.language == Language::Km {
        "Khmer readers say many technical terms in English (e.g. AI, DNA, GDP, SRI, machine learning): keep \
those in English (Latin letters) inside the Khmer text, as a Khmer presenter would say them.\n"
    } else {
        ""
    };
    format!(
        "Build the SOURCE SUMMARY for a {minutes}-minute narrated video about this document, for {audience}.\n\
FILE: {file}\nTITLE SEEN: {title}\nAUTHORS SEEN: {authors}\nYEAR SEEN: {year}\n{focus}\n\
SECTION NOTES (from the document, with PDF page numbers):\n{notes}\n\
Use ONLY these notes. Write `key_points`, `facts` and term `explanation`s in {lang}. {khmer}\
- `title`: the document's title (keep its original language and wording); `authors`, `year`: as found, else \"\".\n\
- `key_points`: 5–12 points (fewer only if the document is very short) in a good order for the video, each one \
self-contained sentence or two, with the `pages` of the notes it comes from.\n\
- `facts`: up to {max_facts} key facts and numbers, exactly as in the notes (numbers, units, dates), with `pages`.\n\
- `terms`: up to {max_terms} key terms the viewer needs: `term` as the document writes it, a one-sentence \
`explanation`, and `pages`.",
        minutes = i.target_minutes,
        audience = i.audience.as_str(),
        file = i.file_name,
        title = i.title,
        authors = i.authors,
        year = i.year,
        notes = notes_list(i.sections),
        lang = language_name(i.language),
        max_facts = MAX_FACTS,
        max_terms = MAX_TERMS,
    )
}

/// The block the outline and chapter prompts get (`script::ScriptBrief::source`): the
/// selected points in the user's order with their notes, then selected facts and terms, then
/// the section notes as background, and the faithfulness rules.
pub fn source_block(summary: &SourceSummary, sections: &[SourceSection], audience: Audience, focus: &str) -> String {
    let items = |list: &[crate::pipeline::project::SourceItem], terms: bool| -> String {
        list.iter()
            .filter(|i| i.selected)
            .enumerate()
            .map(|(n, i)| {
                let head = if terms { format!("{} — {}", i.term, i.text) } else { i.text.clone() };
                let note = if i.note.trim().is_empty() {
                    String::new()
                } else {
                    format!(" (creator's note: {})", i.note.trim())
                };
                format!("{}. {head} [p. {}]{note}\n", n + 1, pages_text(&i.pages))
            })
            .collect()
    };
    let mut notes = notes_list(sections);
    if notes.chars().count() > MAX_SOURCE_NOTES_CHARS {
        notes = notes.chars().take(MAX_SOURCE_NOTES_CHARS).collect::<String>() + "…\n";
    }
    let focus = if focus.trim().is_empty() { String::new() } else { format!("FOCUS: {}\n", focus.trim()) };
    let by = [summary.authors.trim(), summary.year.trim()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(", ");
    format!(
        "SOURCE DOCUMENT: \"{title}\"{by}\nAUDIENCE: {audience}\n{focus}\
KEY POINTS TO COVER, in this order:\n{points}\
KEY FACTS:\n{facts}\
KEY TERMS:\n{terms}\
SECTION NOTES (background from the document):\n{notes}\n\
SOURCE RULES:\n\
- This video explains the source document. Every claim, number and result must come from the points, facts or \
notes above. Never add facts, numbers, names or results that are not in them.\n\
- Short background explanation (what a term means, why something matters) is allowed, but it is general \
knowledge, not a claim from the document: mark such scenes `general_knowledge: true` and never present them as \
the document's findings.\n\
- The title, the hook and the ending follow the same rules. Claims about the document's later impact, \
popularity or what came after it (\"every modern AI uses this\", \"still the standard today\") are NOT in the \
document: leave them out, or put them in a scene marked `general_knowledge: true` with `source_pages: []`.\n\
- `source_pages`: the page numbers (from the [p. …] marks) each scene's claims come from; [] for general-knowledge scenes.\n",
        title = summary.title,
        by = if by.is_empty() { String::new() } else { format!(" ({by})") },
        audience = audience.as_str(),
        points = items(&summary.key_points, false),
        facts = items(&summary.facts, false),
        terms = items(&summary.terms, true),
    )
}

/// "Source: …" line for the video description.
pub fn credit_line(summary: &SourceSummary, language: Language) -> String {
    let label = match language {
        Language::Km => "ប្រភព៖",
        Language::En => "Source:",
    };
    let mut parts = Vec::new();
    if !summary.authors.trim().is_empty() {
        parts.push(summary.authors.trim().to_owned());
    }
    let title = format!("\"{}\"", summary.title.trim());
    match summary.year.trim() {
        "" => parts.push(title),
        y => parts.push(format!("{title} ({y})")),
    }
    format!("{label} {}", parts.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::project::SourceItem;

    fn item(id: &str, text: &str, selected: bool) -> SourceItem {
        SourceItem { id: id.into(), term: String::new(), text: text.into(), pages: vec![2, 3], selected, note: String::new() }
    }

    #[test]
    fn chunk_pages_stay_inside_the_chunk() {
        let reply: ChunkReply = serde_json::from_value(json!({
            "title": "", "authors": "", "year": "",
            "sections": [
                { "heading": "Results", "notes": "Yield rose 30 %.", "pages": [53, 52, 52, 7] },
                { "heading": "Method", "notes": "SRI trials.", "pages": [] }
            ]
        }))
        .unwrap();
        assert!(reply.validate().is_ok());
        let s = reply.into_sections(51, 100);
        assert_eq!(s[0].pages, vec![52, 53]);
        assert_eq!(s[1].pages, vec![51]);
    }

    #[test]
    fn text_chunks_carry_page_markers() {
        let p = chunk_prompt_text(51, 52, &["alpha".into(), "beta".into()]);
        assert!(p.contains("=== Page 51 ===\nalpha\n=== Page 52 ===\nbeta"));
        assert!(chunk_prompt_pdf(51, 100).contains("its first page is page 51"));
    }

    #[test]
    fn summary_prompt_carries_language_focus_and_notes() {
        let sections = vec![SourceSection { heading: "Results".into(), notes: "Yield +30 %".into(), pages: vec![4] }];
        let input = SummaryInput {
            file_name: "rice.pdf",
            title: "Rice",
            authors: "",
            year: "2024",
            sections: &sections,
            language: Language::Km,
            audience: Audience::Students,
            focus: "results only",
            target_minutes: 3,
        };
        let p = summary_prompt(&input);
        assert!(p.contains("in KHMER"));
        assert!(p.contains("keep those in English"));
        assert!(p.contains("\"results only\""));
        assert!(p.contains("- [p. 4] Results: Yield +30 %"));
        assert!(p.contains("students"));
    }

    #[test]
    fn source_block_keeps_order_and_only_selected_items() {
        let summary = SourceSummary {
            title: "Rice in Cambodia".into(),
            authors: "Research team".into(),
            year: "2024".into(),
            key_points: vec![item("k2", "Second", true), item("k1", "First", false), item("k3", "Third", true)],
            facts: vec![item("f1", "75 %", true)],
            terms: vec![SourceItem { term: "SRI".into(), note: "say it in English".into(), ..item("t1", "a rice method", true) }],
        };
        let b = source_block(&summary, &[], Audience::General, "");
        assert!(b.contains("1. Second [p. 2, 3]\n2. Third [p. 2, 3]\n"));
        assert!(!b.contains("First"));
        assert!(b.contains("1. SRI — a rice method [p. 2, 3] (creator's note: say it in English)"));
        assert!(b.contains("\"Rice in Cambodia\" (Research team, 2024)"));
        assert!(b.contains("general_knowledge: true"));
        assert_eq!(credit_line(&summary, Language::En), "Source: Research team, \"Rice in Cambodia\" (2024)");
        let bare = SourceSummary { authors: String::new(), year: String::new(), ..summary };
        assert_eq!(credit_line(&bare, Language::Km), "ប្រភព៖ \"Rice in Cambodia\"");
    }
}
