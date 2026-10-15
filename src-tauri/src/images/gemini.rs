//! Gemini Nano Banana provider (paid only, 07 §1b): 16:9 native, up to 3 reference images.

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use super::postprocess::dimensions;
use super::provider::{Aspect, GeneratedImage, ImageError, ImageProvider, ImageRequest};
use crate::gemini::image::{ImageAnswer, Reference};
use crate::gemini::retry::RetryHooks;
use crate::gemini::{GeminiClient, GeminiError};
use crate::storage::ApiKey;

pub const MAX_REFERENCES: usize = 3;
pub const NEEDS_BILLING: &str =
    "Gemini image models need billing. Switch to Cloudflare Flux in Settings.";

/// Gemini image models offered in Settings (07 §1 table).
pub const MODELS: &[&str] = &[
    "gemini-3.1-flash-lite-image",
    "gemini-nano-banana-2.1",
    "gemini-3-pro-image",
];

pub struct GeminiNanoBanana {
    client: GeminiClient,
    key: ApiKey,
    model: String,
}

impl GeminiNanoBanana {
    pub fn new(client: GeminiClient, key: ApiKey, model: &str) -> Self {
        Self {
            client,
            key,
            model: model.to_owned(),
        }
    }
}

/// Free-tier keys get a quota/billing error for image models ("Not available" on the free tier).
fn is_billing_error(e: &GeminiError) -> bool {
    let detail = e.detail().to_lowercase();
    let mentions_billing = detail.contains("billing")
        || detail.contains("free_tier")
        || detail.contains("free tier")
        || detail.contains("limit: 0");
    match e {
        GeminiError::Api { status, .. } => matches!(*status, 400 | 403 | 429) && mentions_billing,
        GeminiError::InvalidKey { .. } => mentions_billing,
        _ => false,
    }
}

pub fn map_error(e: GeminiError) -> ImageError {
    if is_billing_error(&e) {
        return ImageError::Auth(NEEDS_BILLING.into());
    }
    match e {
        GeminiError::MissingKey | GeminiError::InvalidKey { .. } => ImageError::Auth(e.to_string()),
        GeminiError::Blocked(m) => ImageError::SafetyBlocked(m),
        GeminiError::Api {
            status: 429,
            retry_after,
            ..
        } => ImageError::RateLimited { retry_after },
        e if e.is_retryable() => ImageError::Transient(e.detail()),
        e => ImageError::Other(e.detail()),
    }
}

fn mime_for(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        _ => "image/jpeg",
    }
}

#[async_trait]
impl ImageProvider for GeminiNanoBanana {
    fn id(&self) -> &'static str {
        "gemini_nano_banana"
    }

    fn supports_reference_images(&self) -> bool {
        true
    }

    fn native_aspect(&self) -> Aspect {
        Aspect::Wide16x9
    }

    fn model(&self) -> &str {
        &self.model
    }

    async fn generate(
        &self,
        req: &ImageRequest,
        cancel: &CancellationToken,
    ) -> Result<GeneratedImage, ImageError> {
        let mut refs = Vec::new();
        for path in req.reference_images.iter().take(MAX_REFERENCES) {
            // A missing reference is not fatal: generate without it.
            if let Ok(bytes) = tokio::fs::read(path).await {
                refs.push(Reference {
                    mime_type: mime_for(path).to_owned(),
                    bytes,
                });
            }
        }
        let hooks = RetryHooks {
            cancel: cancel.clone(),
            on_retry: None,
        };
        // The image job owns retries (one loop for every provider) → 1 attempt here.
        let answer = self
            .client
            .generate_image(&self.key, &self.model, &req.prompt, &refs, &hooks, 1)
            .await
            .map_err(map_error)?;
        match answer {
            ImageAnswer::Image(media) => {
                let (width, height) = dimensions(&media.bytes)
                    .ok_or_else(|| ImageError::Other("the image could not be read".into()))?;
                Ok(GeneratedImage {
                    bytes: media.bytes,
                    mime: media.mime_type.unwrap_or_else(|| "image/png".into()),
                    width,
                    height,
                })
            }
            ImageAnswer::Refused(text) => Err(ImageError::SafetyBlocked(text)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api(status: u16, body: &str) -> GeminiError {
        GeminiError::Api {
            status,
            message: "m".into(),
            body: body.into(),
            retry_after: None,
        }
    }

    #[test]
    fn free_tier_errors_ask_for_billing() {
        let e = map_error(api(
            429,
            r#"Quota exceeded for metric: generate_content_free_tier_requests, limit: 0"#,
        ));
        assert!(matches!(&e, ImageError::Auth(m) if m == NEEDS_BILLING));
        let e = map_error(api(400, "This model requires billing to be enabled"));
        assert!(matches!(&e, ImageError::Auth(m) if m == NEEDS_BILLING));
    }

    #[test]
    fn other_errors_map_by_kind() {
        assert!(matches!(
            map_error(api(429, "slow down")),
            ImageError::RateLimited { .. }
        ));
        assert!(matches!(
            map_error(api(503, "busy")),
            ImageError::Transient(_)
        ));
        assert!(matches!(
            map_error(GeminiError::Blocked("SAFETY".into())),
            ImageError::SafetyBlocked(_)
        ));
        assert!(matches!(map_error(api(400, "bad")), ImageError::Other(_)));
    }
}
