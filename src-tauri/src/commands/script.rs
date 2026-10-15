//! Script generation and editing commands.

use tauri::{AppHandle, Emitter, State};

use super::retry_events::retry_hooks;
use crate::error::{CmdResult, to_cmd};
use crate::gemini::GeminiError;
use crate::gemini::client::TextModels;
use crate::pipeline::ProjectError;
use crate::pipeline::editing::{self, SceneEdit};
use crate::pipeline::project::{Project, ProjectStatus};
use crate::pipeline::script_job::ScriptJob;
use crate::state::AppState;
use crate::storage::log::LogFile;
use crate::storage::secrets;

pub const SCRIPT_PROGRESS_EVENT: &str = "script-progress";

/// Starts (or resumes) script generation in the background. With `restart`, the
/// existing outline and scenes are discarded first. Progress arrives as `script-progress`.
#[tauri::command]
pub async fn script_generate(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
    restart: bool,
) -> CmdResult<()> {
    let key = secrets::get_api_key()
        .map_err(to_cmd)?
        .ok_or_else(|| to_cmd(GeminiError::MissingKey))?;
    let project = state.store.get(&project_id).await.map_err(to_cmd)?;
    if project.status != ProjectStatus::ScriptDraft {
        return Err(to_cmd(ProjectError::WrongState(
            editing::status_name(project.status).into(),
        )));
    }
    let cancel = state.script_jobs.start(&project_id).map_err(to_cmd)?;
    if restart {
        let cleared = state
            .store
            .update(&project_id, |p| {
                p.outline = None;
                p.scenes.clear();
                Ok(())
            })
            .await;
        if let Err(e) = cleared {
            state.script_jobs.finish(&project_id);
            return Err(to_cmd(e));
        }
    }

    let store = state.store.clone();
    let jobs = state.script_jobs.clone();
    let client = state.gemini.clone();
    let models = {
        let settings = state.settings.read().await;
        TextModels::new(&settings.models.text, &settings.models.text_fallback)
    };
    let log = LogFile::new(state.paths.project_log(&project_id)).redacting(key.expose());
    let hooks = retry_hooks(
        app.clone(),
        "script",
        Some(project_id.clone()),
        cancel,
        log.clone(),
    );
    tauri::async_runtime::spawn(async move {
        let job = ScriptJob {
            store: &store,
            client: &client,
            key: &key,
            models: &models,
            hooks: &hooks,
            log: &log,
        };
        job.run(&project_id, |p| {
            // Emitting only fails if the window is gone; nothing to do then.
            let _ = app.emit(SCRIPT_PROGRESS_EVENT, p);
        })
        .await;
        jobs.finish(&project_id);
    });
    Ok(())
}

#[tauri::command]
pub fn script_cancel(state: State<'_, AppState>, project_id: String) {
    state.script_jobs.cancel(&project_id);
}

#[tauri::command]
pub fn script_running(state: State<'_, AppState>, project_id: String) -> bool {
    state.script_jobs.is_running(&project_id)
}

#[tauri::command]
pub async fn scene_update(
    state: State<'_, AppState>,
    project_id: String,
    scene: SceneEdit,
) -> CmdResult<Project> {
    state
        .store
        .update(&project_id, |p| editing::apply_scene_edit(p, &scene))
        .await
        .map(|(p, ())| p)
        .map_err(to_cmd)
}

#[tauri::command]
pub async fn scene_add(
    state: State<'_, AppState>,
    project_id: String,
    after_scene_id: Option<String>,
) -> CmdResult<Project> {
    state
        .store
        .update(&project_id, |p| {
            editing::add_scene(p, after_scene_id.as_deref())
        })
        .await
        .map(|(p, _)| p)
        .map_err(to_cmd)
}

#[tauri::command]
pub async fn scene_delete(
    state: State<'_, AppState>,
    project_id: String,
    scene_id: String,
) -> CmdResult<Project> {
    state
        .store
        .update(&project_id, |p| editing::delete_scene(p, &scene_id))
        .await
        .map(|(p, ())| p)
        .map_err(to_cmd)
}

#[tauri::command]
pub async fn scenes_reorder(
    state: State<'_, AppState>,
    project_id: String,
    scene_ids: Vec<String>,
) -> CmdResult<Project> {
    state
        .store
        .update(&project_id, |p| editing::reorder(p, &scene_ids))
        .await
        .map(|(p, ())| p)
        .map_err(to_cmd)
}

/// script_draft → generating. (Phase 1: the voice/image scheduler is not wired yet.)
#[tauri::command]
pub async fn script_approve(state: State<'_, AppState>, project_id: String) -> CmdResult<Project> {
    if state.script_jobs.is_running(&project_id) {
        return Err(to_cmd(ProjectError::Busy));
    }
    state
        .store
        .update(&project_id, editing::approve)
        .await
        .map(|(p, ())| p)
        .map_err(to_cmd)
}

/// "Edit script": back to script_draft.
#[tauri::command]
pub async fn script_reopen(state: State<'_, AppState>, project_id: String) -> CmdResult<Project> {
    state
        .store
        .update(&project_id, editing::reopen)
        .await
        .map(|(p, ())| p)
        .map_err(to_cmd)
}
