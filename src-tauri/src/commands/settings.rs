//! Settings and API key commands. The key itself is never returned to the UI.

use serde::Serialize;
use tauri::State;

use crate::error::{CmdResult, to_cmd};
use crate::gemini::GeminiError;
use crate::state::AppState;
use crate::storage::{ApiKey, Settings, secrets, settings};

#[tauri::command]
pub async fn settings_get(state: State<'_, AppState>) -> CmdResult<Settings> {
    Ok(state.settings.read().await.clone())
}

#[tauri::command]
pub async fn settings_save(state: State<'_, AppState>, settings: Settings) -> CmdResult<Settings> {
    settings::save(&state.paths, &settings).map_err(to_cmd)?;
    *state.settings.write().await = settings.clone();
    Ok(settings)
}

#[tauri::command]
pub async fn api_key_set(key: String) -> CmdResult<()> {
    let key = ApiKey::parse(&key).map_err(to_cmd)?;
    secrets::set_api_key(&key).map_err(to_cmd)
}

#[tauri::command]
pub async fn api_key_exists() -> CmdResult<bool> {
    secrets::get_api_key().map(|k| k.is_some()).map_err(to_cmd)
}

#[tauri::command]
pub async fn api_key_clear() -> CmdResult<()> {
    secrets::clear_api_key().map_err(to_cmd)
}

#[derive(Debug, Serialize)]
pub struct KeyTestResult {
    pub model: String,
    pub display_name: Option<String>,
}

/// Checks the stored key with `models.get` on the configured text model (no tokens used).
#[tauri::command]
pub async fn api_key_test(state: State<'_, AppState>) -> CmdResult<KeyTestResult> {
    let key = secrets::get_api_key()
        .map_err(to_cmd)?
        .ok_or_else(|| to_cmd(GeminiError::MissingKey))?;
    let model = state.settings.read().await.models.text.clone();
    let info = state.gemini.get_model(&key, &model).await.map_err(to_cmd)?;
    Ok(KeyTestResult {
        model: info.name,
        display_name: info.display_name,
    })
}
