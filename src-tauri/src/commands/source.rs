//! "From document" commands (`docs/10-document-source.md`): PDF info, create the project,
//! read the document (background job, `source-progress` events), review, confirm.

use std::path::{Path, PathBuf};

use tauri::{AppHandle, Emitter, State};

use super::retry_events::retry_hooks;
use crate::error::{CmdResult, to_cmd};
use crate::gemini::GeminiError;
use crate::gemini::client::ModelChain;
use crate::pipeline::ProjectError;
use crate::pipeline::project::Project;
use crate::pipeline::source_edit::{self, NewDocumentInput, PdfInfo, ReviewEdit};
use crate::pipeline::source_job::SourceJob;
use crate::state::AppState;
use crate::storage::log::LogFile;
use crate::storage::{AppPaths, secrets};

pub const SOURCE_PROGRESS_EVENT: &str = "source-progress";

/// `source/document.pdf` inside the project folder.
pub fn pdf_path(paths: &AppPaths, project_id: &str) -> PathBuf {
    paths.project_dir(project_id).join("source").join("document.pdf")
}

fn file_name(path: &str) -> String {
    Path::new(path).file_name().map_or_else(|| "document.pdf".into(), |n| n.to_string_lossy().into_owned())
}

async fn read_pdf(path: String) -> CmdResult<Vec<u8>> {
    tauri::async_runtime::spawn_blocking(move || {
        let meta = std::fs::metadata(&path).map_err(|e| format!("Could not open the file: {e}"))?;
        if meta.len() > crate::source::pdf::MAX_BYTES {
            return Err(to_cmd(ProjectError::Pdf(crate::source::pdf::PdfError::TooLarge)));
        }
        std::fs::read(&path).map_err(|e| format!("Could not read the file: {e}"))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn source_pdf_info(path: String) -> CmdResult<PdfInfo> {
    let name = file_name(&path);
    let bytes = read_pdf(path).await?;
    tauri::async_runtime::spawn_blocking(move || source_edit::pdf_info(&name, &bytes).map_err(to_cmd))
        .await
        .map_err(|e| e.to_string())?
}

/// Creates the project and copies the PDF into it. Reading starts with [`source_read`].
#[tauri::command]
pub async fn project_create_from_document(
    state: State<'_, AppState>,
    input: NewDocumentInput,
) -> CmdResult<Project> {
    let tts_model = state.settings.read().await.models.tts.clone();
    let name = file_name(&input.path);
    let bytes = read_pdf(input.path.clone()).await?;
    let (project, bytes) = tauri::async_runtime::spawn_blocking(move || {
        source_edit::new_document_project(&input, &name, &bytes, &tts_model).map(|p| (p, bytes))
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(to_cmd)?;
    state.store.insert(&project).await.map_err(to_cmd)?;
    let target = pdf_path(&state.paths, &project.id);
    let copied = target
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&target, &bytes));
    if let Err(e) = copied {
        let _ = state.store.delete(&project.id).await;
        return Err(format!("Could not copy the PDF into the project: {e}"));
    }
    Ok(project)
}

/// Starts (or resumes after a pause) reading the document. Progress: `source-progress`.
#[tauri::command]
pub async fn source_read(app: AppHandle, state: State<'_, AppState>, project_id: String) -> CmdResult<()> {
    let key = secrets::get_api_key().map_err(to_cmd)?.ok_or_else(|| to_cmd(GeminiError::MissingKey))?;
    let cancel = state.source_jobs.start(&project_id).map_err(to_cmd)?;
    let store = state.store.clone();
    let jobs = state.source_jobs.clone();
    let client = state.gemini.clone();
    let models = ModelChain::new(&state.settings.read().await.models.document);
    let pdf = pdf_path(&state.paths, &project_id);
    let log = LogFile::new(state.paths.project_log(&project_id)).redacting(key.expose());
    let hooks = retry_hooks(app.clone(), "source", Some(project_id.clone()), cancel, log.clone());
    tauri::async_runtime::spawn(async move {
        let job = SourceJob { store: &store, pdf_path: pdf, client: &client, key: &key, models: &models, hooks: &hooks, log: &log };
        job.run(&project_id, |p| {
            // Emitting only fails if the window is gone; nothing to do then.
            let _ = app.emit(SOURCE_PROGRESS_EVENT, p);
        })
        .await;
        jobs.finish(&project_id);
    });
    Ok(())
}

#[tauri::command]
pub fn source_read_cancel(state: State<'_, AppState>, project_id: String) {
    state.source_jobs.cancel(&project_id);
}

#[tauri::command]
pub fn source_read_running(state: State<'_, AppState>, project_id: String) -> bool {
    state.source_jobs.is_running(&project_id)
}

#[tauri::command]
pub async fn source_review_save(state: State<'_, AppState>, project_id: String, edit: ReviewEdit) -> CmdResult<Project> {
    state.store.update(&project_id, |p| source_edit::apply_review(p, &edit)).await.map(|(p, ())| p).map_err(to_cmd)
}

/// Saves the review and confirms it; the Script page then starts the script.
#[tauri::command]
pub async fn source_confirm(state: State<'_, AppState>, project_id: String, edit: ReviewEdit) -> CmdResult<Project> {
    state
        .store
        .update(&project_id, |p| {
            source_edit::apply_review(p, &edit)?;
            source_edit::confirm(p)
        })
        .await
        .map(|(p, ())| p)
        .map_err(to_cmd)
}

/// Back to the review (the script written from the old selection is discarded).
#[tauri::command]
pub async fn source_reopen(state: State<'_, AppState>, project_id: String) -> CmdResult<Project> {
    if state.script_jobs.is_running(&project_id) {
        return Err(to_cmd(ProjectError::Busy));
    }
    state.store.update(&project_id, source_edit::reopen).await.map(|(p, ())| p).map_err(to_cmd)
}

/// Forget the notes and summary and read the document again.
#[tauri::command]
pub async fn source_reread(state: State<'_, AppState>, project_id: String) -> CmdResult<Project> {
    if state.source_jobs.is_running(&project_id) {
        return Err(to_cmd(ProjectError::Busy));
    }
    state.store.update(&project_id, source_edit::reread).await.map(|(p, ())| p).map_err(to_cmd)
}
