//! Settings and API key commands. The key itself is never returned to the UI.

use serde::Serialize;
use tauri::{AppHandle, State};
use tokio_util::sync::CancellationToken;

use super::retry_events::retry_hooks;

use crate::error::{CmdResult, to_cmd};
use crate::gemini::GeminiError;
use crate::state::AppState;
use crate::storage::log::LogFile;
use crate::storage::{ApiKey, Settings, secrets, settings};

#[tauri::command]
pub async fn settings_get(state: State<'_, AppState>) -> CmdResult<Settings> {
    Ok(state.settings.read().await.clone())
}

/// Saves only the fields a Settings section changed (`patch`), merged into the current
/// settings, and returns the full result.
#[tauri::command]
pub async fn settings_save(state: State<'_, AppState>, patch: serde_json::Value) -> CmdResult<Settings> {
    let mut current = state.settings.write().await;
    let next = settings::merged(&current, &patch).map_err(to_cmd)?;
    settings::save(&state.paths, &next).map_err(to_cmd)?;
    *current = next.clone();
    Ok(next)
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

/// Result of "Test key". Failures are returned as data (not `Err`) so the UI can show a
/// friendly message for `error_kind` with the raw `error` under "Details".
#[derive(Debug, Serialize)]
pub struct KeyTestOutcome {
    pub ok: bool,
    pub model: Option<String>,
    pub display_name: Option<String>,
    pub error_kind: Option<String>,
    pub error: Option<String>,
}

impl KeyTestOutcome {
    fn failed(e: &GeminiError) -> Self {
        Self {
            ok: false,
            model: None,
            display_name: None,
            error_kind: Some(e.kind().to_owned()),
            error: Some(e.detail()),
        }
    }
}

/// Checks the stored key with `models.get` on the configured text model (no tokens used).
/// Retries like every Gemini call; waits are reported as `gemini-retry` (scope `key_test`).
#[tauri::command]
pub async fn api_key_test(app: AppHandle, state: State<'_, AppState>) -> CmdResult<KeyTestOutcome> {
    let Some(key) = secrets::get_api_key().map_err(to_cmd)? else {
        return Ok(KeyTestOutcome::failed(&GeminiError::MissingKey));
    };
    let first = state
        .settings
        .read()
        .await
        .models
        .text_chain
        .first()
        .cloned();
    let Some(model) = first else {
        return Ok(KeyTestOutcome::failed(&GeminiError::NoTextModel));
    };
    let log = LogFile::new(state.paths.app_log()).redacting(key.expose());
    let hooks = retry_hooks(app, "key_test", None, CancellationToken::new(), log.clone());
    match state.gemini.get_model(&key, &model, &hooks).await {
        Ok(info) => {
            log.info(&format!("API key test ok ({model})"));
            Ok(KeyTestOutcome {
                ok: true,
                model: Some(info.name),
                display_name: info.display_name,
                error_kind: None,
                error: None,
            })
        }
        Err(e) => {
            log.error(&format!(
                "API key test failed [{}]: {}",
                e.kind(),
                e.detail()
            ));
            Ok(KeyTestOutcome::failed(&e))
        }
    }
}
