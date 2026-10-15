//! Project list / create / get / delete.

use tauri::State;

use crate::error::{CmdResult, to_cmd};
use crate::pipeline::editing::{self, NewProjectInput};
use crate::pipeline::presets::{PRESETS, StylePreset};
use crate::pipeline::project::{Project, ProjectSummary};
use crate::state::AppState;

#[tauri::command]
pub fn presets_list() -> Vec<StylePreset> {
    PRESETS.to_vec()
}

#[tauri::command]
pub async fn project_list(state: State<'_, AppState>) -> CmdResult<Vec<ProjectSummary>> {
    state.store.list().map_err(to_cmd)
}

#[tauri::command]
pub async fn project_create(
    state: State<'_, AppState>,
    input: NewProjectInput,
) -> CmdResult<Project> {
    let tts_model = state.settings.read().await.models.tts.clone();
    let project = editing::new_project(&input, &tts_model).map_err(to_cmd)?;
    state.store.insert(&project).await.map_err(to_cmd)?;
    Ok(project)
}

#[tauri::command]
pub async fn project_get(state: State<'_, AppState>, project_id: String) -> CmdResult<Project> {
    state.store.get(&project_id).await.map_err(to_cmd)
}

#[tauri::command]
pub async fn project_delete(state: State<'_, AppState>, project_id: String) -> CmdResult<()> {
    state.script_jobs.cancel(&project_id);
    state.store.delete(&project_id).await.map_err(to_cmd)
}
