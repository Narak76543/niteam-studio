//! Cloudflare Workers AI — `@cf/black-forest-labs/flux-1-schnell` (07 §1a).
//! Checked against developers.cloudflare.com/workers-ai/models/flux-1-schnell (2026-10-15):
//! - `POST /client/v4/accounts/{account}/ai/run/@cf/black-forest-labs/flux-1-schnell`
//! - `Authorization: Bearer <token>`; body `{prompt (1–2048), steps (≤ 8, default 4), seed}`
//! - response `{ result: { image: <base64 JPEG> }, success, errors[], messages[] }`
//! - errors: HTTP 429 + code 3036 = daily free allocation used up; 3040 = out of capacity.

use std::sync::Arc;

use async_trait::async_trait;
use base64::Engine as _;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use super::postprocess::dimensions;
use super::provider::{
    Aspect, GeneratedImage, ImageError, ImageProvider, ImageRequest, next_utc_midnight,
};
use crate::net::transport::{Auth, HttpRequest, HttpResponse, Method, Transport, TransportError};
use crate::storage::secrets::CloudflareToken;

pub const MODEL: &str = "@cf/black-forest-labs/flux-1-schnell";
const API_BASE: &str = "https://api.cloudflare.com/client/v4/accounts";
const DEFAULT_STEPS: u8 = 4;
const MAX_STEPS: u8 = 8;
/// Cloudflare error codes (developers.cloudflare.com/workers-ai/platform/errors).
const CODE_DAILY_LIMIT: i64 = 3036;
const CODE_OUT_OF_CAPACITY: i64 = 3040;

pub struct CloudflareFlux {
    transport: Arc<dyn Transport>,
    account_id: String,
    token: CloudflareToken,
}

impl CloudflareFlux {
    pub fn new(transport: Arc<dyn Transport>, account_id: &str, token: CloudflareToken) -> Self {
        Self {
            transport,
            account_id: account_id.trim().to_owned(),
            token,
        }
    }

    fn url(&self) -> String {
        format!("{API_BASE}/{}/ai/run/{MODEL}", self.account_id)
    }
}

#[derive(Serialize)]
struct RunRequest<'a> {
    prompt: &'a str,
    steps: u8,
    seed: u32,
}

#[derive(Deserialize, Default)]
struct Envelope {
    #[serde(default)]
    result: Option<RunResult>,
    #[serde(default)]
    success: Option<bool>,
    #[serde(default)]
    errors: Vec<ApiError>,
}

#[derive(Deserialize)]
struct RunResult {
    #[serde(default)]
    image: Option<String>,
}

#[derive(Deserialize)]
struct ApiError {
    #[serde(default)]
    code: Option<i64>,
    #[serde(default)]
    message: String,
}

fn truncate(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// Maps an HTTP response to the image or an [`ImageError`].
fn decode_response(resp: HttpResponse) -> Result<GeneratedImage, ImageError> {
    let raw = String::from_utf8_lossy(&resp.body);
    let envelope: Envelope = serde_json::from_slice(&resp.body).unwrap_or_default();
    let codes: Vec<i64> = envelope.errors.iter().filter_map(|e| e.code).collect();
    let message = envelope
        .errors
        .iter()
        .map(|e| e.message.trim())
        .filter(|m| !m.is_empty())
        .collect::<Vec<_>>()
        .join("; ");
    let detail = if message.is_empty() {
        format!("HTTP {} — {}", resp.status, truncate(raw.trim(), 500))
    } else {
        format!("HTTP {} — {message}", resp.status)
    };
    let lower = message.to_lowercase();

    if codes.contains(&CODE_DAILY_LIMIT) || lower.contains("daily free allocation") || lower.contains("neurons") {
        return Err(ImageError::QuotaExhaustedDaily {
            resets_at_utc: next_utc_midnight(Utc::now()),
        });
    }
    if resp.status == 429 || codes.contains(&CODE_OUT_OF_CAPACITY) {
        return Err(ImageError::RateLimited {
            retry_after: resp.retry_after,
        });
    }
    match resp.status {
        200..=299 => {}
        401 | 403 => {
            return Err(ImageError::Auth(format!(
                "Cloudflare rejected the account ID or API token — check them in Settings → Images. ({detail})"
            )));
        }
        404 => {
            return Err(ImageError::Auth(format!(
                "Cloudflare could not find this account or model — check the account ID in Settings → Images. ({detail})"
            )));
        }
        400 if lower.contains("nsfw") || lower.contains("safety") || lower.contains("flagged") => {
            return Err(ImageError::SafetyBlocked(detail));
        }
        408 | 500..=599 => return Err(ImageError::Transient(detail)),
        _ => return Err(ImageError::Other(detail)),
    }
    if envelope.success == Some(false) {
        return Err(ImageError::Other(detail));
    }
    let b64 = envelope
        .result
        .and_then(|r| r.image)
        .ok_or_else(|| ImageError::Other(format!("no image in the answer ({detail})")))?;
    let b64 = b64
        .split_once("base64,")
        .map_or(b64.as_str(), |(_, data)| data)
        .trim();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| ImageError::Other(format!("image is not valid base64: {e}")))?;
    let (width, height) =
        dimensions(&bytes).ok_or_else(|| ImageError::Other("the image could not be read".into()))?;
    Ok(GeneratedImage {
        bytes,
        mime: "image/jpeg".into(),
        width,
        height,
    })
}

#[async_trait]
impl ImageProvider for CloudflareFlux {
    fn id(&self) -> &'static str {
        "cloudflare_flux"
    }

    fn supports_reference_images(&self) -> bool {
        false
    }

    fn native_aspect(&self) -> Aspect {
        Aspect::Square
    }

    fn model(&self) -> &str {
        MODEL
    }

    async fn generate(
        &self,
        req: &ImageRequest,
        cancel: &CancellationToken,
    ) -> Result<GeneratedImage, ImageError> {
        if self.account_id.is_empty() {
            return Err(ImageError::Auth(
                "Add your Cloudflare account ID in Settings → Images.".into(),
            ));
        }
        let prompt: String = req.prompt.chars().take(super::prompt::MAX_PROMPT_CHARS).collect();
        let body = RunRequest {
            prompt: &prompt,
            steps: req.steps.unwrap_or(DEFAULT_STEPS).clamp(1, MAX_STEPS),
            seed: req.seed,
        };
        let body = serde_json::to_vec(&body).map_err(|e| ImageError::Other(e.to_string()))?;
        let request = HttpRequest {
            method: Method::Post,
            url: self.url(),
            auth: Auth::Bearer(self.token.clone()),
            body: Some(body),
        };
        let resp = tokio::select! {
            r = self.transport.send(request) => r,
            _ = cancel.cancelled() => return Err(ImageError::Other("cancelled".into())),
        };
        let resp = resp.map_err(|e| match e {
            TransportError::Timeout => ImageError::Transient("the request timed out".into()),
            TransportError::Network(m) => ImageError::Transient(m),
        })?;
        decode_response(resp)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use image::{DynamicImage, ImageFormat, RgbImage};

    use super::*;
    use crate::gemini::retry::RetryHooks;
    use crate::images::generate_with_retry;
    use crate::net::transport::mock::{MockTransport, status};

    fn token() -> CloudflareToken {
        CloudflareToken::parse("abcdefghijklmnopqrstuvwxyzABCDEFGHIJ_-12").unwrap()
    }

    fn jpeg_b64() -> String {
        let mut out = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(RgbImage::new(16, 16))
            .write_to(&mut out, ImageFormat::Jpeg)
            .unwrap();
        base64::engine::general_purpose::STANDARD.encode(out.get_ref())
    }

    fn ok_body() -> String {
        format!(r#"{{"result":{{"image":"{}"}},"success":true,"errors":[],"messages":[]}}"#, jpeg_b64())
    }

    fn provider(responses: Vec<Result<HttpResponse, TransportError>>) -> (CloudflareFlux, Arc<MockTransport>) {
        let mock = Arc::new(MockTransport::new(responses));
        (CloudflareFlux::new(mock.clone(), "0123456789abcdef0123456789abcdef", token()), mock)
    }

    fn req() -> ImageRequest {
        ImageRequest {
            prompt: "a small green leaf".into(),
            seed: 7,
            steps: Some(1),
            reference_images: vec![],
        }
    }

    #[tokio::test]
    async fn success_decodes_base64_jpeg() {
        let (p, mock) = provider(vec![status(200, &ok_body())]);
        let img = p.generate(&req(), &CancellationToken::new()).await.unwrap();
        assert_eq!((img.width, img.height, img.mime.as_str()), (16, 16, "image/jpeg"));
        let calls = mock.calls.lock().unwrap();
        assert!(calls[0].1.ends_with("/0123456789abcdef0123456789abcdef/ai/run/@cf/black-forest-labs/flux-1-schnell"));
        assert!(calls[0].2, "bearer header sent");
        assert!(!calls[0].1.contains("abcdefghij"), "token never in the URL");
    }

    #[tokio::test]
    async fn auth_error_stops() {
        let body = r#"{"result":null,"success":false,"errors":[{"code":10000,"message":"Authentication error"}],"messages":[]}"#;
        let (p, _) = provider(vec![status(401, body)]);
        let err = p.generate(&req(), &CancellationToken::new()).await.unwrap_err();
        assert!(matches!(err, ImageError::Auth(_)), "{err:?}");
        assert!(!err.is_retryable());
    }

    #[tokio::test]
    async fn daily_quota_maps_to_pause_until_utc_midnight() {
        let body = r#"{"result":null,"success":false,"errors":[{"code":3036,"message":"Account limited: you have used up your daily free allocation of 10,000 neurons, please upgrade to Cloudflare's Workers Paid plan if you would like to continue usage."}],"messages":[]}"#;
        let (p, _) = provider(vec![status(429, body)]);
        let err = p.generate(&req(), &CancellationToken::new()).await.unwrap_err();
        let ImageError::QuotaExhaustedDaily { resets_at_utc } = err else {
            panic!("expected quota error, got {err:?}");
        };
        assert!(resets_at_utc > Utc::now());
        assert_eq!(resets_at_utc.format("%H:%M:%S").to_string(), "00:00:00");
    }

    #[tokio::test(start_paused = true)]
    async fn server_errors_are_retried_then_succeed() {
        let (p, mock) = provider(vec![
            status(500, "Internal Server Error"),
            status(429, r#"{"success":false,"errors":[{"code":3040,"message":"Capacity temporarily exceeded, please try again."}]}"#),
            status(200, &ok_body()),
        ]);
        let img = generate_with_retry(&p, &req(), &RetryHooks::default()).await.unwrap();
        assert_eq!(img.width, 16);
        assert_eq!(mock.urls().len(), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn quota_is_not_retried() {
        let body = r#"{"success":false,"errors":[{"code":3036,"message":"daily free allocation"}]}"#;
        let (p, mock) = provider(vec![status(429, body), status(200, &ok_body())]);
        let err = generate_with_retry(&p, &req(), &RetryHooks::default()).await.unwrap_err();
        assert!(matches!(err, ImageError::QuotaExhaustedDaily { .. }));
        assert_eq!(mock.urls().len(), 1);
    }

    #[tokio::test]
    async fn missing_account_id_is_an_auth_error() {
        let mock = Arc::new(MockTransport::new(vec![]));
        let p = CloudflareFlux::new(mock, "  ", token());
        assert!(matches!(
            p.generate(&req(), &CancellationToken::new()).await,
            Err(ImageError::Auth(_))
        ));
    }
}
