//! Grammar / phrasing check (Script page) and the "names stay as written" list.

use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};

use super::retry_events::retry_hooks;
use crate::error::{CmdResult, to_cmd};
use crate::gemini::GeminiError;
use crate::gemini::client::ModelChain;
use crate::pipeline::ProjectError;
use crate::pipeline::editing;
use crate::pipeline::project::{Project, ProjectStatus};
use crate::pipeline::proofread::{self, ProofEvents, ProofJob, ProofProgress};
use crate::state::AppState;
use crate::storage::log::LogFile;
use crate::storage::secrets;

const PROOFREAD_PROGRESS_EVENT: &str = "proofread-progress";

struct TauriProofEvents(AppHandle);

impl ProofEvents for TauriProofEvents {
    fn progress(&self, p: ProofProgress) {
        // Emitting only fails if the window is gone; nothing to do then.
        let _ = self.0.emit(PROOFREAD_PROGRESS_EVENT, p);
    }
}

/// Checks every scene whose text changed since the last check (in batches).
#[tauri::command]
pub async fn proofread_start(app: AppHandle, state: State<'_, AppState>, project_id: String) -> CmdResult<()> {
    let key = secrets::get_api_key()
        .map_err(to_cmd)?
        .ok_or_else(|| to_cmd(GeminiError::MissingKey))?;
    let project = state.store.get(&project_id).await.map_err(to_cmd)?;
    if project.status != ProjectStatus::ScriptDraft {
        return Err(to_cmd(ProjectError::WrongState(editing::status_name(project.status).into())));
    }
    if state.script_jobs.is_running(&project_id) {
        return Err(to_cmd(ProjectError::Busy));
    }
    let cancel = state.proofread_jobs.start(&project_id).map_err(to_cmd)?;
    let log = LogFile::new(state.paths.project_log(&project_id)).redacting(key.expose());
    let hooks = retry_hooks(app.clone(), "script", Some(project_id.clone()), cancel.clone(), log.clone());
    let job = ProofJob {
        store: state.store.clone(),
        client: state.gemini.clone(),
        key,
        chain: ModelChain::new(&state.settings.read().await.models.text_chain),
        hooks,
        log,
    };
    let jobs = state.proofread_jobs.clone();
    let events: Arc<dyn ProofEvents> = Arc::new(TauriProofEvents(app));
    tauri::async_runtime::spawn(async move {
        job.run(&project_id, cancel, events).await;
        jobs.finish(&project_id);
    });
    Ok(())
}

#[tauri::command]
pub fn proofread_cancel(state: State<'_, AppState>, project_id: String) {
    state.proofread_jobs.cancel(&project_id);
}

#[tauri::command]
pub fn proofread_running(state: State<'_, AppState>, project_id: String) -> bool {
    state.proofread_jobs.is_running(&project_id)
}

/// Applies the suggestions of `scene_ids` to the script.
#[tauri::command]
pub async fn proofread_accept(state: State<'_, AppState>, project_id: String, scene_ids: Vec<String>) -> CmdResult<Project> {
    state
        .store
        .update(&project_id, |p| proofread::accept(p, &scene_ids).map(|_| ()))
        .await
        .map(|(p, ())| p)
        .map_err(to_cmd)
}

/// Drops the suggestions of `scene_ids` (the text stays as it is).
#[tauri::command]
pub async fn proofread_dismiss(state: State<'_, AppState>, project_id: String, scene_ids: Vec<String>) -> CmdResult<Project> {
    state
        .store
        .update(&project_id, |p| {
            proofread::dismiss(p, &scene_ids);
            Ok(())
        })
        .await
        .map(|(p, ())| p)
        .map_err(to_cmd)
}

/// Names that must stay exactly as written (React Native, Android, …).
#[tauri::command]
pub async fn project_set_keep_terms(state: State<'_, AppState>, project_id: String, terms: Vec<String>) -> CmdResult<Project> {
    state
        .store
        .update(&project_id, |p| proofread::set_keep_terms(p, &terms))
        .await
        .map(|(p, ())| p)
        .map_err(to_cmd)
}
