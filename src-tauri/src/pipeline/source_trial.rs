//! Live check of "From document" (`docs/10-document-source.md` §9): reads each PDF with the
//! real Gemini API (free-tier key from Credential Manager) into a temp project store, prints the
//! chunk modes and the summary, then confirms the first one and writes its script. Ignored:
//!
//! ```text
//! NITEAN_PDFS="<a.pdf>;<b.pdf>" [NITEAN_LANG=km|en] [NITEAN_SCRIPT=1]
//! cargo test --manifest-path src-tauri/Cargo.toml source_trial -- --ignored --nocapture
//! ```

use std::sync::Arc;

use super::project::{Audience, Language, SourceStage};
use super::script_job::ScriptJob;
use super::source_edit::{NewDocumentInput, ReviewEdit, confirm, new_document_project, apply_review};
use super::source_job::SourceJob;
use super::store::ProjectStore;
use crate::gemini::GeminiClient;
use crate::gemini::client::ModelChain;
use crate::gemini::retry::{RetryHooks, RetryNotice};
use crate::storage::log::LogFile;
use crate::storage::settings::Settings;
use crate::storage::{AppPaths, secrets};

#[tokio::test]
#[ignore = "calls the real Gemini API"]
async fn source_trial() {
    let key = secrets::get_api_key().expect("keyring").expect("no Gemini key saved");
    let settings = Settings::default();
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(dir.path().to_path_buf());
    let store = ProjectStore::new(paths.clone());
    let client = GeminiClient::new(&settings.concurrency).unwrap();
    let reader = ModelChain::new(&settings.models.document);
    let language = match std::env::var("NITEAN_LANG").as_deref() {
        Ok("en") => Language::En,
        _ => Language::Km,
    };
    let hooks = RetryHooks::new(
        Default::default(),
        Arc::new(|n: &RetryNotice| eprintln!("[retry] {} {}/{}: {}", n.model, n.attempt, n.max_attempts, n.error)),
    );
    let mut first = None;
    for path in std::env::var("NITEAN_PDFS").expect("NITEAN_PDFS").split(';') {
        let bytes = std::fs::read(path).unwrap();
        let input = NewDocumentInput {
            path: path.into(),
            language,
            target_minutes: 1,
            style_preset: "soft_2d_explainer".into(),
            audience: Audience::General,
            focus: String::new(),
            page_from: None,
            page_to: None,
        };
        let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
        let project = new_document_project(&input, name, &bytes, "tts").unwrap();
        store.insert(&project).await.unwrap();
        let pdf = paths.project_dir(&project.id).join("document.pdf");
        std::fs::write(&pdf, &bytes).unwrap();
        let log = LogFile::new(paths.project_log(&project.id)).redacting(key.expose());
        let job = SourceJob { store: &store, pdf_path: pdf, client: &client, key: &key, models: &reader, hooks: &hooks, log: &log };
        let t = std::time::Instant::now();
        job.run(&project.id, |p| eprintln!("[source] {:?} {}/{} {:?}", p.stage, p.chunks_done, p.chunks_total, p.error)).await;
        let p = store.get(&project.id).await.unwrap();
        let s = p.source.as_ref().unwrap();
        println!("\n===== {name}: {} pages, {:.1} s", s.page_count, t.elapsed().as_secs_f64());
        println!("{}", std::fs::read_to_string(paths.project_log(&project.id)).unwrap_or_default());
        assert_eq!(s.stage, SourceStage::Review, "reading did not finish");
        let summary = s.summary.as_ref().unwrap();
        println!("TITLE: {} | {} | {}", summary.title, summary.authors, summary.year);
        for k in &summary.key_points {
            println!("  * {} {:?}", k.text, k.pages);
        }
        for f in &summary.facts {
            println!("  # {} {:?}", f.text, f.pages);
        }
        for t in &summary.terms {
            println!("  ~ {}: {} {:?}", t.term, t.text, t.pages);
        }
        first.get_or_insert(project.id.clone());
    }

    if std::env::var("NITEAN_SCRIPT").is_err() {
        return;
    }
    let id = first.unwrap();
    store
        .update(&id, |p| {
            apply_review(p, &ReviewEdit { key_point_order: vec![], items: vec![] })?;
            confirm(p)
        })
        .await
        .unwrap();
    let log = LogFile::new(paths.project_log(&id)).redacting(key.expose());
    let chain = ModelChain::new(&settings.models.script_chain(true));
    let job = ScriptJob { store: &store, client: &client, key: &key, models: &chain, hooks: &hooks, log: &log };
    job.run(&id, |p| eprintln!("[script] {:?} {}/{} {:?}", p.stage, p.chapters_done, p.chapters_total, p.error)).await;
    let p = store.get(&id).await.unwrap();
    println!("\n===== SCRIPT: {} | credit: {}", p.title, p.source.as_ref().unwrap().credit);
    println!("keep_terms: {:?}", p.keep_terms);
    for s in &p.scenes {
        println!(
            "[{}] p.{:?}{} {}\n      fact_note: {:?}",
            s.id,
            s.source_pages,
            if s.general_knowledge { " (general knowledge)" } else { "" },
            s.narration,
            s.fact_note
        );
    }
    assert!(!p.scenes.is_empty());
}
