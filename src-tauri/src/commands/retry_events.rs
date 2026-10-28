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
                RetryReason::NextModel => format!(
                    "{} gave up after {} attempts ({}); trying next model {}",
                    notice.previous_model.as_deref().unwrap_or("?"),
                    notice.max_attempts,
                    notice.error,
                    notice.model
                ),
                RetryReason::InvalidReply => format!(
                    "{} returned an invalid reply ({}); repair step {}/{}",
                    notice.model, notice.error, notice.attempt, notice.max_attempts
                ),
                _ => format!(
                    "{} attempt {}/{} failed: {}; retrying in {} ms",
                    notice.model,
                    notice.attempt - 1,
                    notice.max_attempts,
                    notice.error,
                    notice.delay_ms
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
