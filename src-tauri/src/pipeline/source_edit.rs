//! "From document" project operations (`docs/10-document-source.md` §2, §6): create from a
//! PDF, review edits (tick, reorder, note), confirm, go back. Pure functions on `Project`.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::ProjectError;
use super::editing::{NewProjectInput, new_project};
use super::project::{Aspect, Audience, Language, Project, ProjectStatus, SourceChunk, SourceDoc, SourceStage};
use crate::gemini::document;
use crate::source::pdf::{self, MAX_PAGES, PdfFile};

const MAX_FOCUS_CHARS: usize = 300;
const MAX_NOTE_CHARS: usize = 500;

#[derive(Debug, Clone, Deserialize)]
pub struct NewDocumentInput {
    /// The PDF the user picked (copied into the project folder).
    pub path: String,
    pub language: Language,
    pub target_minutes: u32,
    /// `"16:9"` (default) or `"9:16"`.
    #[serde(default)]
    pub aspect: Aspect,
    pub style_preset: String,
    pub audience: Audience,
    #[serde(default)]
    pub focus: String,
    /// Page range for PDFs longer than [`MAX_PAGES`] (1-based, inclusive).
    #[serde(default)]
    pub page_from: Option<u32>,
    #[serde(default)]
    pub page_to: Option<u32>,
}

/// Upload screen: what the picked file is.
#[derive(Debug, Clone, Serialize)]
pub struct PdfInfo {
    pub file_name: String,
    pub size_bytes: u64,
    pub page_count: u32,
    /// Longer than [`MAX_PAGES`]: a page range must be chosen.
    pub needs_range: bool,
    pub max_pages: u32,
}

fn invalid(msg: &str) -> ProjectError {
    ProjectError::Invalid(msg.into())
}

pub fn pdf_info(file_name: &str, bytes: &[u8]) -> Result<PdfInfo, ProjectError> {
    let page_count = PdfFile::load(bytes)?.page_count();
    Ok(PdfInfo {
        file_name: file_name.to_owned(),
        size_bytes: bytes.len() as u64,
        page_count,
        needs_range: page_count > MAX_PAGES,
        max_pages: MAX_PAGES,
    })
}

/// File name without folders and the `.pdf` extension (the project's topic and title).
pub fn display_name(file_name: &str) -> String {
    let base = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    let stem = base.strip_suffix(".pdf").or_else(|| base.strip_suffix(".PDF")).unwrap_or(base);
    stem.replace(['_', '-'], " ").trim().to_owned()
}

pub fn new_document_project(
    input: &NewDocumentInput,
    file_name: &str,
    bytes: &[u8],
    tts_model: &str,
) -> Result<Project, ProjectError> {
    let page_count = PdfFile::load(bytes)?.page_count();
    let (from, to) = match (input.page_from, input.page_to) {
        (Some(f), Some(t)) => (f, t),
        _ if page_count > MAX_PAGES => {
            return Err(invalid(&format!(
                "This PDF has {page_count} pages. Choose a page range of at most {MAX_PAGES} pages."
            )));
        }
        _ => (1, page_count),
    };
    pdf::check_range(from, to, page_count)?;
    let focus = input.focus.trim().to_owned();
    if focus.chars().count() > MAX_FOCUS_CHARS {
        return Err(invalid("The focus is too long (max 300 characters)."));
    }
    let name = display_name(file_name);
    let topic: String = if name.is_empty() { "Document".into() } else { name.chars().take(200).collect() };
    let mut project = new_project(
        &NewProjectInput {
            topic,
            language: input.language,
            target_minutes: input.target_minutes,
            aspect: input.aspect,
            style_preset: input.style_preset.clone(),
        },
        tts_model,
    )?;
    project.source = Some(SourceDoc {
        file_name: file_name.rsplit(['/', '\\']).next().unwrap_or(file_name).to_owned(),
        size_bytes: bytes.len() as u64,
        page_count,
        sha256: hex::encode(Sha256::digest(bytes)),
        page_from: from,
        page_to: to,
        audience: input.audience,
        focus,
        stage: SourceStage::Reading,
        chunks: pdf::chunks(from, to)
            .into_iter()
            .map(|(from, to)| SourceChunk {
                from,
                to,
                mode: None,
                sections: vec![],
                title: String::new(),
                authors: String::new(),
                year: String::new(),
                done: false,
            })
            .collect(),
        summary: None,
        pause: None,
        credit: String::new(),
    });
    Ok(project)
}

/// One reviewed item (key point, fact or term).
#[derive(Debug, Clone, Deserialize)]
pub struct ItemEdit {
    pub id: String,
    pub selected: bool,
    #[serde(default)]
    pub note: String,
}

/// Review screen state: every item's tick and note, key points in the new order.
#[derive(Debug, Clone, Deserialize)]
pub struct ReviewEdit {
    pub key_point_order: Vec<String>,
    pub items: Vec<ItemEdit>,
}

fn source_in_review(p: &mut Project) -> Result<&mut SourceDoc, ProjectError> {
    let s = p.source.as_mut().ok_or_else(|| invalid("This project has no source document."))?;
    if s.stage != SourceStage::Review || s.summary.is_none() {
        return Err(ProjectError::WrongState("source_reading".into()));
    }
    Ok(s)
}

pub fn apply_review(p: &mut Project, edit: &ReviewEdit) -> Result<(), ProjectError> {
    let s = source_in_review(p)?;
    let Some(summary) = s.summary.as_mut() else { return Err(invalid("No summary yet.")) };
    // Reorder: listed ids first in the given order, unknown ids ignored, missing ones kept at the end.
    let mut ordered = Vec::with_capacity(summary.key_points.len());
    for id in &edit.key_point_order {
        if let Some(i) = summary.key_points.iter().position(|k| &k.id == id) {
            ordered.push(summary.key_points.remove(i));
        }
    }
    ordered.append(&mut summary.key_points);
    summary.key_points = ordered;
    for e in &edit.items {
        let note: String = e.note.trim().chars().take(MAX_NOTE_CHARS).collect();
        let lists = [&mut summary.key_points, &mut summary.facts, &mut summary.terms];
        if let Some(item) = lists.into_iter().flat_map(|l| l.iter_mut()).find(|i| i.id == e.id) {
            item.selected = e.selected;
            item.note = note;
        }
    }
    Ok(())
}

/// "Confirm and write script": at least one key point; sets the credit line and keeps the
/// selected terms that are written in Latin letters as names (Khmer narration keeps them).
pub fn confirm(p: &mut Project) -> Result<(), ProjectError> {
    let language = p.language;
    let s = source_in_review(p)?;
    let Some(summary) = s.summary.as_ref() else { return Err(invalid("No summary yet.")) };
    if !summary.key_points.iter().any(|k| k.selected) {
        return Err(invalid("Select at least one key point."));
    }
    s.credit = document::credit_line(summary, language);
    let terms: Vec<String> = summary
        .terms
        .iter()
        .filter(|t| t.selected && t.term.chars().any(|c| c.is_ascii_alphabetic()))
        .map(|t| t.term.clone())
        .collect();
    s.stage = SourceStage::Confirmed;
    for t in terms {
        if !p.keep_terms.iter().any(|k| k.eq_ignore_ascii_case(&t)) {
            p.keep_terms.push(t);
        }
    }
    Ok(())
}

/// Back to the review screen: the script written from the old selection is discarded.
pub fn reopen(p: &mut Project) -> Result<(), ProjectError> {
    if p.status != ProjectStatus::ScriptDraft {
        return Err(ProjectError::WrongState(super::editing::status_name(p.status).into()));
    }
    let s = p.source.as_mut().ok_or_else(|| invalid("This project has no source document."))?;
    if s.stage != SourceStage::Confirmed {
        return Err(ProjectError::WrongState("source_reading".into()));
    }
    s.stage = SourceStage::Review;
    p.outline = None;
    p.visual_bible = None;
    p.scenes.clear();
    Ok(())
}

/// "Read again": forget the notes and the summary (e.g. after a bad read).
pub fn reread(p: &mut Project) -> Result<(), ProjectError> {
    if p.status != ProjectStatus::ScriptDraft || !p.scenes.is_empty() {
        return Err(ProjectError::WrongState(super::editing::status_name(p.status).into()));
    }
    let s = p.source.as_mut().ok_or_else(|| invalid("This project has no source document."))?;
    s.stage = SourceStage::Reading;
    s.summary = None;
    s.pause = None;
    for c in &mut s.chunks {
        c.done = false;
        c.mode = None;
        c.sections.clear();
    }
    p.outline = None;
    p.visual_bible = None;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::project::{SourceItem, SourceSummary};

    fn item(id: &str) -> SourceItem {
        SourceItem { id: id.into(), term: String::new(), text: id.into(), pages: vec![1], selected: true, note: String::new() }
    }

    fn reviewed() -> Project {
        let mut p = crate::pipeline::project::tests::sample();
        p.source = Some(SourceDoc {
            file_name: "Rice_report-2024.pdf".into(),
            size_bytes: 10,
            page_count: 3,
            sha256: String::new(),
            page_from: 1,
            page_to: 3,
            audience: Audience::General,
            focus: String::new(),
            stage: SourceStage::Review,
            chunks: vec![],
            summary: Some(SourceSummary {
                title: "Rice report".into(),
                authors: "Team".into(),
                year: "2024".into(),
                key_points: vec![item("k1"), item("k2"), item("k3")],
                facts: vec![item("f1")],
                terms: vec![
                    SourceItem { term: "SRI".into(), ..item("t1") },
                    SourceItem { term: "ស្រូវ".into(), ..item("t2") },
                ],
            }),
            pause: None,
            credit: String::new(),
        });
        p
    }

    #[test]
    fn names_come_from_the_file() {
        assert_eq!(display_name("C:\\Docs\\Rice_report-2024.pdf"), "Rice report 2024");
        assert_eq!(display_name("paper.PDF"), "paper");
    }

    #[test]
    fn review_reorders_ticks_and_notes() {
        let mut p = reviewed();
        let edit = ReviewEdit {
            key_point_order: vec!["k3".into(), "nope".into(), "k1".into()],
            items: vec![
                ItemEdit { id: "k1".into(), selected: false, note: String::new() },
                ItemEdit { id: "f1".into(), selected: true, note: "  check this  ".into() },
            ],
        };
        apply_review(&mut p, &edit).unwrap();
        let s = p.source.as_ref().unwrap().summary.as_ref().unwrap();
        let ids: Vec<&str> = s.key_points.iter().map(|k| k.id.as_str()).collect();
        assert_eq!(ids, ["k3", "k1", "k2"]);
        assert!(!s.key_points[1].selected);
        assert_eq!(s.facts[0].note, "check this");
    }

    #[test]
    fn confirm_needs_a_point_and_sets_credit_and_terms() {
        let mut p = reviewed();
        let none = ReviewEdit {
            key_point_order: vec![],
            items: ["k1", "k2", "k3"].iter().map(|id| ItemEdit { id: (*id).into(), selected: false, note: String::new() }).collect(),
        };
        apply_review(&mut p, &none).unwrap();
        assert!(confirm(&mut p).is_err());
        let mut p = reviewed();
        p.language = Language::En;
        confirm(&mut p).unwrap();
        let s = p.source.as_ref().unwrap();
        assert_eq!(s.stage, SourceStage::Confirmed);
        assert_eq!(s.credit, "Source: Team, \"Rice report\" (2024)");
        assert_eq!(p.keep_terms, vec!["SRI".to_owned()], "only Latin-script terms are kept as names");
        assert!(apply_review(&mut p, &none).is_err(), "no edits after confirming");
        p.scenes.push(crate::pipeline::project::Scene::new("s001".into(), 0));
        reopen(&mut p).unwrap();
        assert!(p.scenes.is_empty());
        assert_eq!(p.source.as_ref().unwrap().stage, SourceStage::Review);
    }

    #[test]
    fn long_pdfs_need_a_range() {
        let input = NewDocumentInput {
            path: "x.pdf".into(),
            language: Language::Km,
            target_minutes: 3,
            aspect: Aspect::default(),
            style_preset: "stickman".into(),
            audience: Audience::General,
            focus: String::new(),
            page_from: None,
            page_to: None,
        };
        // Not a PDF at all.
        assert!(new_document_project(&input, "x.pdf", b"nope", "tts").is_err());
    }
}
