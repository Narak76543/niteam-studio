//! Gemini API access (BYOK). Request/response shapes follow the official
//! `generateContent` / `models.get` REST reference (ai.google.dev/api), checked 2026-10-15.

pub mod client;
pub mod diagram_fix;
pub mod document;
pub mod image;
pub mod image_prompts;
pub mod interactions;
pub mod models;
pub mod proofread;
pub mod retry;
pub mod script;
pub mod tts;

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
            // A used-up daily quota stays used up until midnight Pacific: retrying only burns time.
            Self::Api { .. } if self.is_daily_quota() => false,
            Self::Api { status, .. } => matches!(*status, 408 | 429 | 500..=599),
            Self::Network(_) | Self::Timeout => true,
            _ => false,
        }
    }

    /// After a model's retries are used up: is another model in the chain worth trying?
    /// Yes for transient failures and 404 (model renamed/retired); no for key, quota-free
    /// request errors or safety blocks, which another model would hit too.
    pub fn try_next_model(&self) -> bool {
        // Free-tier daily quotas are per model: the next one still has its own.
        self.is_retryable() || self.is_daily_quota() || matches!(self, Self::Api { status: 404, .. })
    }

    /// The server's `RetryInfo.retryDelay` (e.g. `"76787s"`) from a 429 body.
    pub fn retry_delay(&self) -> Option<Duration> {
        match self {
            Self::Api { body, .. } => {
                let start = body.find("retryDelay")?;
                let rest = &body[start..];
                let digits: String = rest
                    .chars()
                    .skip_while(|c| !c.is_ascii_digit())
                    .take_while(char::is_ascii_digit)
                    .collect();
                digits.parse().ok().map(Duration::from_secs)
            }
            Self::ChainExhausted { last, .. } => last.retry_delay(),
            _ => None,
        }
    }

    /// 429 for the per-day quota (free tier: requests per day), not the per-minute limit.
    /// The body names the quota, e.g. `GenerateRequestsPerDayPerProjectPerModel-FreeTier`.
    pub fn is_daily_quota(&self) -> bool {
        match self {
            Self::Api { status: 429, body, message, .. } => {
                body.contains("PerDay") || message.to_lowercase().contains("per day")
            }
            Self::ChainExhausted { last, .. } => last.is_daily_quota(),
            _ => false,
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn api(status: u16, body: &str) -> GeminiError {
        GeminiError::Api { status, message: "Resource exhausted".into(), body: body.into(), retry_after: None }
    }

    #[test]
    fn daily_quota_is_not_retried_but_the_minute_limit_is() {
        let daily = api(429, r#"{"error":{"details":[{"violations":[{"quotaId":"GenerateRequestsPerDayPerProjectPerModel-FreeTier"}]}]}}"#);
        assert!(daily.is_daily_quota());
        assert!(!daily.is_retryable());
        let minute = api(429, r#"{"error":{"details":[{"violations":[{"quotaId":"GenerateRequestsPerMinutePerProjectPerModel-FreeTier"}]}]}}"#);
        assert!(!minute.is_daily_quota());
        assert!(minute.is_retryable());
        assert!(!api(503, "PerDay").is_daily_quota());
        assert!(daily.try_next_model(), "another model has its own daily quota");
    }

    #[test]
    fn retry_delay_is_read_from_the_body() {
        // Shape of the real free-tier 429 (2026-10-17).
        let e = api(429, "{
 \"@type\": \"type.googleapis.com/google.rpc.RetryInfo\",
 \"retryDelay\": \"76787s\"
}");
        assert_eq!(e.retry_delay(), Some(Duration::from_secs(76_787)));
        assert_eq!(api(429, "no delay").retry_delay(), None);
    }
}
