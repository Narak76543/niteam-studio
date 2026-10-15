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
    #[error("Gemini did not accept the API key ({0}). Check the key in Settings.")]
    InvalidKey(String),
    #[error("Gemini error {status}: {message}")]
    Api {
        status: u16,
        message: String,
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

    /// 503 after all retries: the model is overloaded → the text fallback model may help.
    pub fn is_overloaded(&self) -> bool {
        matches!(self, Self::Api { status: 503, .. })
    }

    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::Api { retry_after, .. } => *retry_after,
            _ => None,
        }
    }

    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Api { status, .. } => Some(*status),
            _ => None,
        }
    }

    /// Stable code the UI maps to a friendly, translated message (`error.<kind>.*`).
    pub fn kind(&self) -> &'static str {
        match self {
            Self::MissingKey => "missing_key",
            Self::InvalidKey(_) => "invalid_key",
            Self::Api { status: 429, .. } => "rate_limited",
            Self::Api { status: 402, .. } => "payment",
            Self::Api { status: 404, .. } => "not_found",
            Self::Api {
                status: 500..=599, ..
            } => "busy",
            Self::Api { status: 408, .. } | Self::Network(_) | Self::Timeout => "network",
            Self::Blocked(_) => "blocked",
            Self::BadResponse(_) => "bad_response",
            Self::Cancelled => "cancelled",
            Self::Api { .. } => "other",
        }
    }
}
