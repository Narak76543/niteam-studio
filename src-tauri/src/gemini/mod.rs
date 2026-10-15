//! Gemini API access (BYOK). Request/response shapes follow the official
//! `generateContent` / `models.get` REST reference (ai.google.dev/api), checked 2026-10-15.

pub mod client;
pub mod models;
pub mod retry;
pub mod script;
pub mod transport;

pub use client::GeminiClient;

use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum GeminiError {
    #[error("No Gemini API key is set. Add your key in Settings.")]
    MissingKey,
    #[error("Gemini did not accept the API key ({message}). Check the key in Settings.")]
    InvalidKey {
        status: u16,
        message: String,
        body: String,
    },
    #[error("Gemini error {status}: {message}")]
    Api {
        status: u16,
        message: String,
        /// Raw response body (truncated) — for logs and "Details" only.
        body: String,
        retry_after: Option<Duration>,
    },
    #[error("Network problem talking to Gemini: {0}")]
    Network(String),
    #[error("Gemini did not answer in time (request timed out)")]
    Timeout,
    #[error("Gemini blocked this request: {0}")]
    Blocked(String),
    #[error("Gemini returned an unexpected answer: {0}")]
    BadResponse(String),
    #[error("No text model is configured. Add one in Settings → Models.")]
    NoTextModel,
    /// Every model of the text chain failed; `last` is the final model's error.
    #[error("All text models failed (tried {}). Last error: {last}", tried.join(", "))]
    ChainExhausted {
        tried: Vec<String>,
        last: Box<GeminiError>,
    },
    #[error("Cancelled")]
    Cancelled,
}

impl GeminiError {
    /// 408 / 429 / 5xx, timeouts and network failures are transient; everything else is final.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Api { status, .. } => matches!(*status, 408 | 429 | 500..=599),
            Self::Network(_) | Self::Timeout => true,
            _ => false,
        }
    }

    /// After a model's retries are used up: is another model in the chain worth trying?
    /// Yes for transient failures and 404 (model renamed/retired); no for key, quota-free
    /// request errors or safety blocks, which another model would hit too.
    pub fn try_next_model(&self) -> bool {
        self.is_retryable() || matches!(self, Self::Api { status: 404, .. })
    }

    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::Api { retry_after, .. } => *retry_after,
            _ => None,
        }
    }

    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Api { status, .. } | Self::InvalidKey { status, .. } => Some(*status),
            Self::ChainExhausted { last, .. } => last.status(),
            _ => None,
        }
    }

    /// Full technical description for logs and the UI "Details" section:
    /// exact HTTP status and raw response body where there is one.
    pub fn detail(&self) -> String {
        match self {
            Self::Api { status, body, .. } | Self::InvalidKey { status, body, .. } => {
                format!("HTTP {status} — {body}")
            }
            Self::ChainExhausted { tried, last } => {
                format!(
                    "Tried: {}. Last error: {}",
                    tried.join(" → "),
                    last.detail()
                )
            }
            other => other.to_string(),
        }
    }

    /// Stable code the UI maps to a friendly, translated message (`error.<kind>.*`).
    pub fn kind(&self) -> &'static str {
        match self {
            Self::MissingKey => "missing_key",
            Self::InvalidKey { .. } => "invalid_key",
            Self::Api { status: 429, .. } => "rate_limited",
            Self::Api { status: 402, .. } => "payment",
            Self::Api { status: 404, .. } | Self::NoTextModel => "not_found",
            Self::Api {
                status: 500..=599, ..
            } => "busy",
            Self::Api { status: 408, .. } | Self::Network(_) | Self::Timeout => "network",
            Self::Blocked(_) => "blocked",
            Self::BadResponse(_) => "bad_response",
            Self::ChainExhausted { last, .. } => last.kind(),
            Self::Cancelled => "cancelled",
            Self::Api { .. } => "other",
        }
    }
}
