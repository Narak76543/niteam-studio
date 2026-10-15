//! Image provider trait and types (`docs/07-image-generation.md` §1).

use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ProviderId {
    #[default]
    CloudflareFlux,
    GeminiNanoBanana,
    /// stable-diffusion.cpp on the user's GPU (`docs/08-local-gpu.md`).
    LocalSdcpp,
}

impl ProviderId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CloudflareFlux => "cloudflare_flux",
            Self::GeminiNanoBanana => "gemini_nano_banana",
            Self::LocalSdcpp => "local_sdcpp",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aspect {
    Square,
    Wide16x9,
}

pub struct ImageRequest {
    /// Fully built prompt (`images::prompt`).
    pub prompt: String,
    /// Used by the local engine (reproducible images). Cloudflare Flux rejects a `seed`
    /// property (07 §1a) and the Gemini image request has none.
    pub seed: u32,
    pub steps: Option<u8>,
    /// Ignored by providers without reference-image support.
    pub reference_images: Vec<PathBuf>,
}

pub struct GeneratedImage {
    pub bytes: Vec<u8>,
    pub mime: String,
    pub width: u32,
    pub height: u32,
}

impl std::fmt::Debug for GeneratedImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "GeneratedImage({} {}×{}, {} bytes)",
            self.mime,
            self.width,
            self.height,
            self.bytes.len()
        )
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ImageError {
    /// Daily free allowance used up → pause the queue until `resets_at_utc`.
    #[error("Daily free image limit reached (resets {resets_at_utc})")]
    QuotaExhaustedDaily { resets_at_utc: DateTime<Utc> },
    /// Back off and retry.
    #[error("Too many requests to the image service")]
    RateLimited { retry_after: Option<Duration> },
    /// The user edits the prompt.
    #[error("The image service refused this prompt: {0}")]
    SafetyBlocked(String),
    /// Bad token/key or missing billing: stop the job.
    #[error("{0}")]
    Auth(String),
    /// The local engine is missing or crashed: stop the job (switch to Cloudflare).
    #[error("{0}")]
    Engine(String),
    /// 5xx / timeout / network: retry.
    #[error("The image service had a temporary problem: {0}")]
    Transient(String),
    #[error("Image generation failed: {0}")]
    Other(String),
}

impl ImageError {
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::RateLimited { .. } | Self::Transient(_))
    }

    /// Stable code for the UI's friendly message.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::QuotaExhaustedDaily { .. } => "quota_daily",
            Self::RateLimited { .. } => "rate_limited",
            Self::SafetyBlocked(_) => "blocked",
            Self::Auth(_) => "image_auth",
            Self::Engine(_) => "local_engine",
            Self::Transient(_) => "busy",
            Self::Other(_) => "other",
        }
    }
}

#[async_trait]
pub trait ImageProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn supports_reference_images(&self) -> bool;
    fn native_aspect(&self) -> Aspect;
    /// Model ID used (part of the cache hash).
    fn model(&self) -> &str;
    async fn generate(
        &self,
        req: &ImageRequest,
        cancel: &CancellationToken,
    ) -> Result<GeneratedImage, ImageError>;
}

/// Next 00:00 UTC after `now` (Cloudflare's daily reset).
pub fn next_utc_midnight(now: DateTime<Utc>) -> DateTime<Utc> {
    let tomorrow = now.date_naive().succ_opt().unwrap_or(now.date_naive());
    tomorrow
        .and_hms_opt(0, 0, 0)
        .map(|t| t.and_utc())
        .unwrap_or(now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn reset_is_next_utc_midnight() {
        let now = Utc.with_ymd_and_hms(2026, 10, 15, 23, 59, 0).unwrap();
        assert_eq!(
            next_utc_midnight(now),
            Utc.with_ymd_and_hms(2026, 10, 16, 0, 0, 0).unwrap()
        );
        let now = Utc.with_ymd_and_hms(2026, 12, 31, 0, 0, 1).unwrap();
        assert_eq!(
            next_utc_midnight(now),
            Utc.with_ymd_and_hms(2027, 1, 1, 0, 0, 0).unwrap()
        );
    }
}
