//! Image generation (`docs/07-image-generation.md`).
//! - [`provider`]: `ImageProvider` trait and types
//! - [`cloudflare`]: Workers AI Flux Schnell (default, free daily allowance)
//! - [`gemini`]: Gemini Nano Banana (paid)
//! - [`prompt`] / [`plan`]: prompt, seed and cache hash per scene (pure)
//! - [`postprocess`]: 16:9 crop, pan-size upscale, thumbnail
//! - [`budget`]: estimate before generating

pub mod budget;
pub mod cloudflare;
pub mod gemini;
pub mod plan;
pub mod postprocess;
pub mod prompt;
pub mod provider;

use std::time::Duration;

use rand::Rng;

use crate::gemini::retry::{RetryHooks, RetryNotice, RetryReason, backoff_delay};
use provider::{GeneratedImage, ImageError, ImageProvider, ImageRequest};

/// Attempts for transient errors / rate limits (2 s, 4 s, 8 s, 16 s between them).
pub const MAX_ATTEMPTS: u32 = 5;

/// Calls the provider, retrying rate limits and transient errors with backoff; quota,
/// auth, safety and other errors return immediately.
pub async fn generate_with_retry(
    provider: &dyn ImageProvider,
    req: &ImageRequest,
    hooks: &RetryHooks,
) -> Result<GeneratedImage, ImageError> {
    let mut attempt = 1;
    loop {
        let err = match provider.generate(req, &hooks.cancel).await {
            Ok(img) => return Ok(img),
            Err(e) => e,
        };
        if !err.is_retryable() || attempt >= MAX_ATTEMPTS || hooks.cancel.is_cancelled() {
            return Err(err);
        }
        let retry_after = match &err {
            ImageError::RateLimited { retry_after } => *retry_after,
            _ => None,
        };
        let jitter = Duration::from_millis(rand::thread_rng().gen_range(0..=1_000));
        let delay = backoff_delay(attempt, retry_after, jitter);
        hooks.notify(&RetryNotice {
            model: provider.model().to_owned(),
            previous_model: None,
            reason: match err {
                ImageError::RateLimited { .. } => RetryReason::RateLimited,
                _ => RetryReason::Busy,
            },
            attempt: attempt + 1,
            max_attempts: MAX_ATTEMPTS,
            delay_ms: u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
            status: None,
            error: err.to_string(),
        });
        tokio::select! {
            _ = tokio::time::sleep(delay) => {}
            _ = hooks.cancel.cancelled() => return Err(err),
        }
        attempt += 1;
    }
}
