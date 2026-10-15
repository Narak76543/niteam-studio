//! Gemini API access (BYOK). Request/response shapes follow the official
//! `generateContent` / `models.get` REST reference (ai.google.dev/api), checked 2026-10-15.

pub mod client;
pub mod models;
pub mod retry;
pub mod script;

pub use client::{EndpointKind, GeminiClient};

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
    #[error("Gemini blocked this request: {0}")]
    Blocked(String),
    #[error("Gemini returned an unexpected answer: {0}")]
    BadResponse(String),
    #[error("Cancelled")]
    Cancelled,
}

impl GeminiError {
    /// 408 / 429 / 5xx and network failures are transient; everything else is final.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Api { status, .. } => matches!(*status, 408 | 429 | 500..=599),
            Self::Network(_) => true,
            _ => false,
        }
    }

    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::Api { retry_after, .. } => *retry_after,
            _ => None,
        }
    }
}
