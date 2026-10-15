//! Turns Gemini retry notices into `gemini-retry` events (for the UI) and log lines.

use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio_util::sync::CancellationToken;

use crate::gemini::retry::{RetryHooks, RetryNotice, RetryReason};
use crate::storage::log::LogFile;

pub const GEMINI_RETRY_EVENT: &str = "gemini-retry";

/// Payload of `gemini-retry`. `scope` says which UI should show it
/// (`script` + `project_id`, or `key_test`).
#[derive(Debug, Clone, Serialize)]
pub struct GeminiRetryEvent {
    pub scope: &'static str,
    pub project_id: Option<String>,
    #[serde(flatten)]
    pub notice: RetryNotice,
}

pub fn retry_hooks(
    app: AppHandle,
    scope: &'static str,
    project_id: Option<String>,
    cancel: CancellationToken,
    log: LogFile,
) -> RetryHooks {
    RetryHooks::new(
        cancel,
        Arc::new(move |notice: &RetryNotice| {
            let line = match notice.reason {
                RetryReason::Fallback => format!(
                    "primary model still unavailable ({}); switching to fallback model {}",
                    notice.error, notice.model
                ),
                _ => format!(
                    "{} failed ({}); retrying in {} ms (attempt {}/{})",
                    notice.model,
                    notice.error,
                    notice.delay_ms,
                    notice.attempt,
                    notice.max_attempts
                ),
            };
            log.warn(&line);
            // Emitting only fails if the window is gone; nothing to do then.
            let _ = app.emit(
                GEMINI_RETRY_EVENT,
                GeminiRetryEvent {
                    scope,
                    project_id: project_id.clone(),
                    notice: notice.clone(),
                },
            );
        }),
    )
}
