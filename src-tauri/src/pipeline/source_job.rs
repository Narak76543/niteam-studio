//! Reading a "From document" PDF (`docs/10-document-source.md` §3–5): one Gemini call per
//! 50-page chunk (extracted text when it is clean, else the PDF pages), then one text-only call
//! for the Source summary. `project.json` is saved after every chunk, so a pause, cancel or
//! crash resumes at the first chunk that isn't `done`. A used-up free-tier quota pauses the job
//! (`source.pause`) instead of failing it.

use std::path::PathBuf;
use std::sync::Arc;

use chrono::{Duration, Utc};
use serde::Serialize;

use super::ProjectError;
use super::project::{ChunkMode, Project, QuotaPause, SourceDoc, SourceItem, SourceStage, SourceSummary};
use super::store::ProjectStore;
use crate::gemini::client::ModelChain;
use crate::gemini::document::{
    self, ChunkReply, ItemReply, READER_SYSTEM, SummaryInput, SummaryReply,
};
use crate::gemini::models::Content;
use crate::gemini::retry::RetryHooks;
use crate::gemini::script::request_json_with;
use crate::gemini::{GeminiClient, GeminiError};
use crate::source::pdf::{PdfError, PdfFile};
use crate::source::{quality, reset};
use crate::storage::ApiKey;
use crate::storage::log::LogFile;

/// Wait after a per-minute rate limit that outlasted the retries.
const MINUTE_PAUSE_SECS: i64 = 60;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceJobStage {
    Reading,
    Summarizing,
    Done,
    Paused,
    Failed,
    Cancelled,
}

/// Payload of the `source-progress` event.
#[derive(Debug, Clone, Serialize)]
pub struct SourceProgress {
    pub project_id: String,
    pub stage: SourceJobStage,
    pub chunks_done: usize,
    pub chunks_total: usize,
    /// `Paused`: when trying again makes sense (RFC 3339 UTC), and whether it is the daily quota.
    pub resets_at_utc: Option<String>,
    pub daily: bool,
    pub error: Option<String>,
    pub error_kind: Option<String>,
}

pub struct SourceJob<'a> {
    pub store: &'a ProjectStore,
    /// `source/document.pdf` of the project.
    pub pdf_path: PathBuf,
    pub client: &'a GeminiClient,
    pub key: &'a ApiKey,
    /// `settings.models.document`.
    pub models: &'a ModelChain,
    pub hooks: &'a RetryHooks,
    pub log: &'a LogFile,
}

fn counts(p: &Project) -> (usize, usize) {
    p.source.as_ref().map_or((0, 0), |s| (s.chunks.iter().filter(|c| c.done).count(), s.chunks.len()))
}

fn source_of(p: &mut Project) -> Result<&mut SourceDoc, ProjectError> {
    p.source.as_mut().ok_or_else(|| ProjectError::Invalid("This project has no source document.".into()))
}

/// The pause for a quota error, or `None` for any other error.
pub fn pause_for(e: &GeminiError, now: chrono::DateTime<Utc>) -> Option<QuotaPause> {
    if e.kind() != "rate_limited" {
        return None;
    }
    let daily = e.is_daily_quota();
    // The server's retry delay when it sends one (the live daily-quota 429 pointed at midnight
    // UTC, not the documented midnight Pacific); else the documented reset.
    let server = e.retry_delay().or_else(|| e.retry_after()).map(|d| d.as_secs() as i64);
    let at = match (server, daily) {
        (Some(secs), _) => now + Duration::seconds(secs.max(MINUTE_PAUSE_SECS)),
        (None, true) => reset::next_pacific_midnight(now),
        (None, false) => now + Duration::seconds(MINUTE_PAUSE_SECS),
    };
    Some(QuotaPause { resets_at_utc: at.to_rfc3339(), daily })
}

fn to_items(prefix: &str, replies: Vec<ItemReply>, range: (u32, u32)) -> Vec<SourceItem> {
    replies
        .into_iter()
        .enumerate()
        .map(|(i, r)| SourceItem {
            id: format!("{prefix}{}", i + 1),
            term: String::new(),
            text: r.text.trim().to_owned(),
            pages: clean_pages(r.pages, range),
            selected: true,
            note: String::new(),
        })
        .collect()
}

fn clean_pages(mut pages: Vec<u32>, (from, to): (u32, u32)) -> Vec<u32> {
    pages.retain(|p| (from..=to).contains(p));
    pages.sort_unstable();
    pages.dedup();
    pages
}

/// Summary reply → stored summary (ids `k1…`, `f1…`, `t1…`; everything selected).
pub fn to_summary(r: SummaryReply, range: (u32, u32)) -> SourceSummary {
    SourceSummary {
        title: r.title.trim().to_owned(),
        authors: r.authors.trim().to_owned(),
        year: r.year.trim().to_owned(),
        key_points: to_items("k", r.key_points, range),
        facts: to_items("f", r.facts, range),
        terms: r
            .terms
            .into_iter()
            .enumerate()
            .map(|(i, t)| SourceItem {
                id: format!("t{}", i + 1),
                term: t.term.trim().to_owned(),
                text: t.explanation.trim().to_owned(),
                pages: clean_pages(t.pages, range),
                selected: true,
                note: String::new(),
            })
            .collect(),
    }
}

impl SourceJob<'_> {
    pub async fn run(&self, project_id: &str, emit: impl Fn(SourceProgress)) {
        let result = self.run_inner(project_id, &emit).await;
        let mut progress = SourceProgress {
            project_id: project_id.to_owned(),
            stage: SourceJobStage::Done,
            chunks_done: 0,
            chunks_total: 0,
            resets_at_utc: None,
            daily: false,
            error: None,
            error_kind: None,
        };
        match result {
            Ok(()) => self.log.info("document read; summary ready for review"),
            Err(ProjectError::Gemini(GeminiError::Cancelled)) => {
                self.log.info("document reading cancelled");
                progress.stage = SourceJobStage::Cancelled;
            }
            Err(ProjectError::Gemini(e)) if pause_for(&e, Utc::now()).is_some() => {
                let pause = pause_for(&e, Utc::now());
                let saved = self
                    .store
                    .update(project_id, |p| {
                        source_of(p)?.pause = pause.clone();
                        Ok(())
                    })
                    .await;
                if let Some(q) = &pause {
                    self.log.info(&format!(
                        "free-tier {} limit reached; reading paused until {} (no key switching): {}",
                        if q.daily { "daily" } else { "per-minute" },
                        q.resets_at_utc,
                        e.detail()
                    ));
                    progress.resets_at_utc = Some(q.resets_at_utc.clone());
                    progress.daily = q.daily;
                }
                progress.stage = if saved.is_ok() { SourceJobStage::Paused } else { SourceJobStage::Failed };
                progress.error_kind = Some(e.kind().to_owned());
            }
            Err(e) => {
                self.log.error(&format!("document reading failed [{}]: {}", e.kind(), e.detail()));
                progress.stage = SourceJobStage::Failed;
                progress.error = Some(e.detail());
                progress.error_kind = Some(e.kind().to_owned());
            }
        }
        if let Ok(p) = self.store.get(project_id).await {
            (progress.chunks_done, progress.chunks_total) = counts(&p);
        }
        emit(progress);
    }

    fn progress(&self, id: &str, stage: SourceJobStage, p: &Project) -> SourceProgress {
        let (done, total) = counts(p);
        SourceProgress {
            project_id: id.to_owned(),
            stage,
            chunks_done: done,
            chunks_total: total,
            resets_at_utc: None,
            daily: false,
            error: None,
            error_kind: None,
        }
    }

    async fn run_inner(&self, id: &str, emit: &impl Fn(SourceProgress)) -> Result<(), ProjectError> {
        let (mut project, ()) = self
            .store
            .update(id, |p| {
                let s = source_of(p)?;
                if s.stage != SourceStage::Reading {
                    return Err(ProjectError::WrongState("source_review".into()));
                }
                s.pause = None;
                Ok(())
            })
            .await?;
        let path = self.pdf_path.clone();
        let pdf = Arc::new(
            tokio::task::spawn_blocking(move || {
                let bytes = std::fs::read(&path).map_err(|e| PdfError::Unreadable(e.to_string()))?;
                PdfFile::load(&bytes)
            })
            .await
            .map_err(|e| PdfError::Unreadable(e.to_string()))??,
        );

        let total = counts(&project).1;
        for index in 0..total {
            let Some(chunk) = project.source.as_ref().and_then(|s| s.chunks.get(index)).cloned() else {
                break;
            };
            if chunk.done {
                continue;
            }
            emit(self.progress(id, SourceJobStage::Reading, &project));
            let (from, to) = (chunk.from, chunk.to);
            let doc = pdf.clone();
            let (content, mode, verdict) = tokio::task::spawn_blocking(move || {
                let texts = doc.page_texts(from, to);
                let verdict = quality::check(&texts);
                if verdict == quality::Verdict::Good {
                    Ok((Content::user(document::chunk_prompt_text(from, to, &texts)), ChunkMode::Text, verdict))
                } else {
                    let bytes = doc.extract_range(from, to)?;
                    Ok::<_, PdfError>((
                        Content::user_with_pdf(&bytes, document::chunk_prompt_pdf(from, to)),
                        ChunkMode::Pdf,
                        verdict,
                    ))
                }
            })
            .await
            .map_err(|e| PdfError::Unreadable(e.to_string()))??;
            self.log.info(&format!(
                "pages {from}–{to}: sent as {} ({})",
                match mode {
                    ChunkMode::Text => "extracted text",
                    ChunkMode::Pdf => "PDF pages",
                },
                verdict.reason()
            ));
            let (reply, model): (ChunkReply, String) = request_json_with(
                self.client,
                self.key,
                self.models,
                READER_SYSTEM,
                content,
                document::chunk_schema(),
                self.hooks,
            )
            .await?;
            self.log.info(&format!("pages {from}–{to}: {} section(s), read by {model}", reply.sections.len()));
            project = self
                .store
                .update(id, |p| {
                    let c = source_of(p)?
                        .chunks
                        .get_mut(index)
                        .ok_or_else(|| ProjectError::Invalid("The page chunks changed.".into()))?;
                    c.mode = Some(mode);
                    c.title = reply.title.trim().to_owned();
                    c.authors = reply.authors.trim().to_owned();
                    c.year = reply.year.trim().to_owned();
                    c.sections = reply.into_sections(from, to);
                    c.done = true;
                    Ok(())
                })
                .await?
                .0;
        }

        emit(self.progress(id, SourceJobStage::Summarizing, &project));
        let s = project.source.as_ref().ok_or_else(|| ProjectError::Invalid("no source".into()))?;
        let first = |f: fn(&super::project::SourceChunk) -> &str| {
            s.chunks.iter().map(f).find(|v| !v.is_empty()).unwrap_or("").to_owned()
        };
        let (title, authors, year) = (first(|c| &c.title), first(|c| &c.authors), first(|c| &c.year));
        let sections: Vec<_> = s.chunks.iter().flat_map(|c| c.sections.iter().cloned()).collect();
        let prompt = document::summary_prompt(&SummaryInput {
            file_name: &s.file_name,
            title: &title,
            authors: &authors,
            year: &year,
            sections: &sections,
            language: project.language,
            audience: s.audience,
            focus: &s.focus,
            target_minutes: project.target_minutes,
        });
        let range = (s.page_from, s.page_to);
        let (reply, model): (SummaryReply, String) = request_json_with(
            self.client,
            self.key,
            self.models,
            READER_SYSTEM,
            Content::user(prompt),
            document::summary_schema(),
            self.hooks,
        )
        .await?;
        self.log.info(&format!("source summary: {} key points, written by {model}", reply.key_points.len()));
        let summary = to_summary(reply, range);
        self.store
            .update(id, |p| {
                let s = source_of(p)?;
                s.summary = Some(summary);
                s.stage = SourceStage::Review;
                Ok(())
            })
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::DateTime;

    fn api(body: &str) -> GeminiError {
        GeminiError::Api { status: 429, message: "quota".into(), body: body.into(), retry_after: None }
    }

    #[test]
    fn quota_errors_pause_until_the_reset() {
        let now = DateTime::parse_from_rfc3339("2026-10-17T02:00:00Z").unwrap().with_timezone(&Utc);
        let daily = pause_for(&api("GenerateRequestsPerDayPerProjectPerModel-FreeTier"), now).unwrap();
        assert!(daily.daily);
        assert_eq!(daily.resets_at_utc, "2026-10-17T07:00:00+00:00");
        let minute = pause_for(&api("PerMinute"), now).unwrap();
        assert!(!minute.daily);
        assert_eq!(minute.resets_at_utc, "2026-10-17T02:01:00+00:00");
        let chained = GeminiError::ChainExhausted { tried: vec!["m".into()], last: Box::new(api("PerDay")) };
        assert!(pause_for(&chained, now).unwrap().daily);
        assert!(pause_for(&GeminiError::Timeout, now).is_none());
        let told = pause_for(&api(r#"PerDay "retryDelay": "76787s""#), now).unwrap();
        assert_eq!(told.resets_at_utc, "2026-10-17T23:19:47+00:00", "the server's delay wins");
    }

    /// A small text PDF (Helvetica, one line per page).
    fn tiny_pdf(pages: usize) -> Vec<u8> {
        use lopdf::content::{Content as PdfContent, Operation};
        use lopdf::{Document, Object, Stream, dictionary};
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let font = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" });
        let resources = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font } });
        let mut kids: Vec<Object> = vec![];
        for i in 0..pages {
            let text = format!("Page {i}: rice yields rose by thirty percent in the trial provinces this year. ").repeat(3);
            let content = PdfContent {
                operations: vec![
                    Operation::new("BT", vec![]),
                    Operation::new("Tf", vec!["F1".into(), 9.into()]),
                    Operation::new("Td", vec![20.into(), 700.into()]),
                    Operation::new("Tj", vec![Object::string_literal(text)]),
                    Operation::new("ET", vec![]),
                ],
            };
            let stream = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
            kids.push(doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages_id, "Contents" => stream }).into());
        }
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => kids, "Count" => pages as i64, "Resources" => resources,
                "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog);
        let mut out = Vec::new();
        doc.save_to(&mut out).unwrap();
        out
    }

    fn reply(json: serde_json::Value) -> Result<crate::net::transport::HttpResponse, crate::net::transport::TransportError> {
        let body = serde_json::json!({
            "candidates": [{ "content": { "role": "model", "parts": [{ "text": json.to_string() }] }, "finishReason": "STOP" }]
        });
        crate::net::transport::mock::status(200, &body.to_string())
    }

    fn chunk_ok(page: u32) -> serde_json::Value {
        serde_json::json!({ "title": "", "authors": "", "year": "",
            "sections": [{ "heading": "Results", "notes": "Yields rose 30 %.", "pages": [page] }] })
    }

    #[tokio::test]
    async fn a_daily_quota_pauses_and_the_next_run_resumes_at_the_open_chunk() {
        use crate::net::transport::mock::{MockTransport, status};
        use crate::pipeline::source_edit::{NewDocumentInput, new_document_project};
        use crate::storage::AppPaths;
        use crate::storage::settings::ConcurrencySettings;

        let dir = tempfile::tempdir().unwrap();
        let store = ProjectStore::new(AppPaths::new(dir.path().to_path_buf()));
        let bytes = tiny_pdf(2);
        let input = NewDocumentInput {
            path: "rice.pdf".into(),
            language: crate::pipeline::project::Language::En,
            target_minutes: 2,
            style_preset: "stickman".into(),
            audience: crate::pipeline::project::Audience::General,
            focus: String::new(),
            page_from: None,
            page_to: None,
        };
        let mut project = new_document_project(&input, "rice.pdf", &bytes, "tts").unwrap();
        // Two one-page chunks instead of one 50-page chunk.
        let src = project.source.as_mut().unwrap();
        let template = src.chunks[0].clone();
        src.chunks = vec![
            crate::pipeline::project::SourceChunk { from: 1, to: 1, ..template.clone() },
            crate::pipeline::project::SourceChunk { from: 2, to: 2, ..template },
        ];
        store.insert(&project).await.unwrap();
        let pdf_path = dir.path().join("doc.pdf");
        std::fs::write(&pdf_path, &bytes).unwrap();
        let key = ApiKey::parse("AIzaSyTESTTESTTESTTESTTESTTEST123").unwrap();
        let log = LogFile::new(dir.path().join("log.txt"));
        let chain = ModelChain::new(&["gemini-3.8-flash"]);
        let hooks = RetryHooks::default();
        let daily = r#"{"error":{"code":429,"message":"quota","details":[{"violations":[{"quotaId":"GenerateRequestsPerDayPerProjectPerModel-FreeTier"}]},{"retryDelay":"3600s"}]}}"#;

        let mock = Arc::new(MockTransport::new(vec![reply(chunk_ok(1)), status(429, daily)]));
        let client = GeminiClient::with_transport(mock.clone(), &ConcurrencySettings::default());
        let job = SourceJob { store: &store, pdf_path: pdf_path.clone(), client: &client, key: &key, models: &chain, hooks: &hooks, log: &log };
        let last = std::sync::Mutex::new(None);
        job.run(&project.id, |p| *last.lock().unwrap() = Some(p)).await;
        let p1 = last.lock().unwrap().take().unwrap();
        assert_eq!(p1.stage, SourceJobStage::Paused);
        assert!(p1.daily && p1.resets_at_utc.is_some());
        assert_eq!((p1.chunks_done, p1.chunks_total), (1, 2));
        assert_eq!(mock.urls().len(), 2, "the daily quota is not retried");
        let saved = store.get(&project.id).await.unwrap();
        let s = saved.source.as_ref().unwrap();
        assert!(s.pause.is_some() && s.chunks[0].done && !s.chunks[1].done);
        assert!(s.chunks[0].mode.is_some());

        let summary = serde_json::json!({ "title": "Rice", "authors": "", "year": "",
            "key_points": [{ "text": "Yields rose 30 %.", "pages": [1, 2] }], "facts": [], "terms": [] });
        let mock = Arc::new(MockTransport::new(vec![reply(chunk_ok(2)), reply(summary)]));
        let client = GeminiClient::with_transport(mock.clone(), &ConcurrencySettings::default());
        let job = SourceJob { store: &store, pdf_path, client: &client, key: &key, models: &chain, hooks: &hooks, log: &log };
        job.run(&project.id, |p| *last.lock().unwrap() = Some(p)).await;
        assert_eq!(last.lock().unwrap().take().unwrap().stage, SourceJobStage::Done);
        assert_eq!(mock.urls().len(), 2, "chunk 1 is not read again");
        project = store.get(&project.id).await.unwrap();
        let s = project.source.as_ref().unwrap();
        assert_eq!(s.stage, SourceStage::Review);
        assert!(s.pause.is_none());
        assert_eq!(s.summary.as_ref().unwrap().key_points[0].pages, vec![1, 2]);
    }

    #[test]
    fn summary_items_get_ids_and_pages_in_range() {
        let r: SummaryReply = serde_json::from_value(serde_json::json!({
            "title": " Rice ", "authors": "", "year": "2024",
            "key_points": [{ "text": "A", "pages": [5, 2, 99] }, { "text": "B", "pages": [] }],
            "facts": [{ "text": "75 %", "pages": [1] }],
            "terms": [{ "term": "SRI", "explanation": "a method", "pages": [3] }]
        }))
        .unwrap();
        let s = to_summary(r, (2, 10));
        assert_eq!(s.title, "Rice");
        assert_eq!(s.key_points[0].id, "k1");
        assert_eq!(s.key_points[0].pages, vec![2, 5]);
        assert!(s.key_points.iter().all(|k| k.selected));
        assert!(s.facts[0].pages.is_empty(), "page 1 is outside the read range");
        assert_eq!((s.terms[0].id.as_str(), s.terms[0].term.as_str(), s.terms[0].text.as_str()), ("t1", "SRI", "a method"));
    }
}
