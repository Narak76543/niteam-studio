//! HTTP client for the Gemini REST API.
//! - Key goes in the `x-goog-api-key` header (never in the URL, so it can't leak via errors).
//! - One semaphore per endpoint kind limits concurrent requests.
//! - Every call goes through [`retry::with_retry`].

use std::sync::Arc;
use std::time::Duration;

use reqwest::StatusCode;
use reqwest::header::{HeaderMap, HeaderValue, RETRY_AFTER};
use serde::de::DeserializeOwned;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use super::GeminiError;
use super::models::{ErrorEnvelope, GenerateContentRequest, GenerateContentResponse, ModelInfo};
use super::retry;
use crate::storage::ApiKey;
use crate::storage::settings::ConcurrencySettings;

const BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(180);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // Tts (Phase 2) and Image (Phase 3) are used by later pipeline steps.
pub enum EndpointKind {
    Text,
    Tts,
    Image,
}

#[derive(Clone)]
pub struct GeminiClient {
    http: reqwest::Client,
    base_url: String,
    text_limit: Arc<Semaphore>,
    tts_limit: Arc<Semaphore>,
    image_limit: Arc<Semaphore>,
}

impl GeminiClient {
    pub fn new(concurrency: &ConcurrencySettings) -> Result<Self, GeminiError> {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .user_agent(concat!("NiteanStudio/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| GeminiError::Network(e.to_string()))?;
        Ok(Self {
            http,
            base_url: BASE_URL.to_owned(),
            text_limit: Arc::new(Semaphore::new(concurrency.text)),
            tts_limit: Arc::new(Semaphore::new(concurrency.tts)),
            image_limit: Arc::new(Semaphore::new(concurrency.image)),
        })
    }

    fn limit(&self, kind: EndpointKind) -> &Arc<Semaphore> {
        match kind {
            EndpointKind::Text => &self.text_limit,
            EndpointKind::Tts => &self.tts_limit,
            EndpointKind::Image => &self.image_limit,
        }
    }

    fn auth_headers(key: &ApiKey) -> Result<HeaderMap, GeminiError> {
        let mut value = HeaderValue::from_str(key.expose())
            .map_err(|_| GeminiError::InvalidKey("contains invalid characters".into()))?;
        value.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert("x-goog-api-key", value);
        Ok(headers)
    }

    /// `GET /models/{model}` — costs no tokens; used by the "Test key" button.
    pub async fn get_model(&self, key: &ApiKey, model: &str) -> Result<ModelInfo, GeminiError> {
        let url = format!("{}/models/{}", self.base_url, model);
        let headers = Self::auth_headers(key)?;
        retry::with_retry(&CancellationToken::new(), || async {
            let _permit = self.acquire(EndpointKind::Text).await?;
            let resp = self.http.get(&url).headers(headers.clone()).send().await;
            Self::decode::<ModelInfo>(resp).await
        })
        .await
    }

    /// `POST /models/{model}:generateContent` with retry and concurrency limit.
    pub async fn generate_content(
        &self,
        key: &ApiKey,
        model: &str,
        kind: EndpointKind,
        request: &GenerateContentRequest,
        cancel: &CancellationToken,
    ) -> Result<GenerateContentResponse, GeminiError> {
        let url = format!("{}/models/{}:generateContent", self.base_url, model);
        let headers = Self::auth_headers(key)?;
        retry::with_retry(cancel, || async {
            let _permit = self.acquire(kind).await?;
            let resp = self
                .http
                .post(&url)
                .headers(headers.clone())
                .json(request)
                .send()
                .await;
            Self::decode::<GenerateContentResponse>(resp).await
        })
        .await
    }

    async fn acquire(
        &self,
        kind: EndpointKind,
    ) -> Result<tokio::sync::SemaphorePermit<'_>, GeminiError> {
        self.limit(kind)
            .acquire()
            .await
            .map_err(|_| GeminiError::Cancelled)
    }

    async fn decode<T: DeserializeOwned>(
        resp: Result<reqwest::Response, reqwest::Error>,
    ) -> Result<T, GeminiError> {
        let resp = resp.map_err(|e| GeminiError::Network(e.without_url().to_string()))?;
        let status = resp.status();
        if status.is_success() {
            return resp
                .json::<T>()
                .await
                .map_err(|e| GeminiError::BadResponse(e.without_url().to_string()));
        }
        let retry_after = parse_retry_after(resp.headers());
        let body = resp.text().await.unwrap_or_default();
        Err(map_status(status, &body, retry_after))
    }
}

fn parse_retry_after(headers: &HeaderMap) -> Option<Duration> {
    headers
        .get(RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
}

fn map_status(status: StatusCode, body: &str, retry_after: Option<Duration>) -> GeminiError {
    let message = serde_json::from_str::<ErrorEnvelope>(body)
        .map(|e| e.error.message)
        .ok()
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| {
            status
                .canonical_reason()
                .unwrap_or("unknown error")
                .to_owned()
        });
    let is_key_problem = status == StatusCode::UNAUTHORIZED
        || status == StatusCode::FORBIDDEN
        || (status == StatusCode::BAD_REQUEST && message.to_lowercase().contains("api key"));
    if is_key_problem {
        return GeminiError::InvalidKey(message);
    }
    GeminiError::Api {
        status: status.as_u16(),
        message,
        retry_after,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_key_message_is_mapped() {
        let body = r#"{"error":{"code":400,"message":"API key not valid. Please pass a valid API key.","status":"INVALID_ARGUMENT"}}"#;
        assert!(matches!(
            map_status(StatusCode::BAD_REQUEST, body, None),
            GeminiError::InvalidKey(_)
        ));
        assert!(matches!(
            map_status(StatusCode::FORBIDDEN, "", None),
            GeminiError::InvalidKey(_)
        ));
    }

    #[test]
    fn rate_limit_keeps_retry_after() {
        let e = map_status(
            StatusCode::TOO_MANY_REQUESTS,
            "not json",
            Some(Duration::from_secs(3)),
        );
        assert!(e.is_retryable());
        assert_eq!(e.retry_after(), Some(Duration::from_secs(3)));
    }

    #[test]
    fn parses_retry_after_seconds() {
        let mut h = HeaderMap::new();
        h.insert(RETRY_AFTER, HeaderValue::from_static("12"));
        assert_eq!(parse_retry_after(&h), Some(Duration::from_secs(12)));
    }
}
