//! Client for the Gemini REST API.
//! - Every call goes through [`retry::with_retry`] (one place for all Gemini traffic).
//! - Key goes in the `x-goog-api-key` header (never in the URL, so it can't leak via errors).
//! - One semaphore per endpoint kind limits concurrent requests; a permit is held per
//!   HTTP attempt, not during backoff waits.
//! - Text calls can fall back to a second model when the first stays overloaded (503).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::de::DeserializeOwned;
use tokio::sync::Semaphore;

use super::GeminiError;
use super::models::{ErrorEnvelope, GenerateContentRequest, GenerateContentResponse, ModelInfo};
use super::retry::{self, MAX_ATTEMPTS, RetryHooks, RetryNotice, RetryReason};
use super::transport::{
    HttpRequest, HttpResponse, Method, ReqwestTransport, Transport, TransportError,
};
use crate::storage::ApiKey;
use crate::storage::settings::ConcurrencySettings;

const BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // Tts (Phase 2) and Image (Phase 3) are used by later pipeline steps.
pub enum EndpointKind {
    Text,
    Tts,
    Image,
}

/// Primary + optional fallback text model for one job. Once the fallback had to be used,
/// the rest of the job stays on it (no point waiting ~30 s on a busy model per chapter).
pub struct TextModels {
    pub primary: String,
    pub fallback: Option<String>,
    switched: AtomicBool,
}

impl TextModels {
    pub fn new(primary: &str, fallback: &str) -> Self {
        let fallback = fallback.trim();
        let fallback = (!fallback.is_empty() && fallback != primary).then(|| fallback.to_owned());
        Self {
            primary: primary.to_owned(),
            fallback,
            switched: AtomicBool::new(false),
        }
    }
}

#[derive(Clone)]
pub struct GeminiClient {
    transport: Arc<dyn Transport>,
    base_url: String,
    text_limit: Arc<Semaphore>,
    tts_limit: Arc<Semaphore>,
    image_limit: Arc<Semaphore>,
}

impl GeminiClient {
    pub fn new(concurrency: &ConcurrencySettings) -> Result<Self, GeminiError> {
        let transport = ReqwestTransport::new().map_err(|e| GeminiError::Network(e.to_string()))?;
        Ok(Self::with_transport(Arc::new(transport), concurrency))
    }

    pub fn with_transport(
        transport: Arc<dyn Transport>,
        concurrency: &ConcurrencySettings,
    ) -> Self {
        Self {
            transport,
            base_url: BASE_URL.to_owned(),
            text_limit: Arc::new(Semaphore::new(concurrency.text)),
            tts_limit: Arc::new(Semaphore::new(concurrency.tts)),
            image_limit: Arc::new(Semaphore::new(concurrency.image)),
        }
    }

    fn limit(&self, kind: EndpointKind) -> &Arc<Semaphore> {
        match kind {
            EndpointKind::Text => &self.text_limit,
            EndpointKind::Tts => &self.tts_limit,
            EndpointKind::Image => &self.image_limit,
        }
    }

    /// `GET /models/{model}` — costs no tokens; used by the "Test key" button.
    pub async fn get_model(
        &self,
        key: &ApiKey,
        model: &str,
        hooks: &RetryHooks,
    ) -> Result<ModelInfo, GeminiError> {
        let request = HttpRequest {
            method: Method::Get,
            url: format!("{}/models/{}", self.base_url, model),
            api_key: key.clone(),
            body: None,
        };
        self.call(EndpointKind::Text, model, request, hooks).await
    }

    /// `POST /models/{model}:generateContent`.
    pub async fn generate_content(
        &self,
        key: &ApiKey,
        model: &str,
        kind: EndpointKind,
        body: &GenerateContentRequest,
        hooks: &RetryHooks,
    ) -> Result<GenerateContentResponse, GeminiError> {
        let body = serde_json::to_vec(body)
            .map_err(|e| GeminiError::BadResponse(format!("could not encode request: {e}")))?;
        let request = HttpRequest {
            method: Method::Post,
            url: format!("{}/models/{}:generateContent", self.base_url, model),
            api_key: key.clone(),
            body: Some(body),
        };
        self.call(kind, model, request, hooks).await
    }

    /// Text generation with the fallback rule: if the primary model is still overloaded
    /// (503) after all retries, the fallback model is tried once (with the same retry rules).
    pub async fn generate_text(
        &self,
        key: &ApiKey,
        models: &TextModels,
        body: &GenerateContentRequest,
        hooks: &RetryHooks,
    ) -> Result<GenerateContentResponse, GeminiError> {
        if let Some(fb) = &models.fallback
            && models.switched.load(Ordering::Relaxed)
        {
            return self
                .generate_content(key, fb, EndpointKind::Text, body, hooks)
                .await;
        }
        match self
            .generate_content(key, &models.primary, EndpointKind::Text, body, hooks)
            .await
        {
            Err(e) if e.is_overloaded() => {
                let Some(fb) = &models.fallback else {
                    return Err(e);
                };
                hooks.notify(&RetryNotice {
                    model: fb.clone(),
                    reason: RetryReason::Fallback,
                    attempt: 1,
                    max_attempts: MAX_ATTEMPTS,
                    delay_ms: 0,
                    status: e.status(),
                    error: e.to_string(),
                });
                models.switched.store(true, Ordering::Relaxed);
                self.generate_content(key, fb, EndpointKind::Text, body, hooks)
                    .await
            }
            other => other,
        }
    }

    async fn call<T: DeserializeOwned>(
        &self,
        kind: EndpointKind,
        model: &str,
        request: HttpRequest,
        hooks: &RetryHooks,
    ) -> Result<T, GeminiError> {
        retry::with_retry(hooks, model, || {
            let request = request.clone();
            async move {
                let _permit = self
                    .limit(kind)
                    .acquire()
                    .await
                    .map_err(|_| GeminiError::Cancelled)?;
                let response = self.transport.send(request).await.map_err(|e| match e {
                    TransportError::Timeout => GeminiError::Timeout,
                    TransportError::Network(m) => GeminiError::Network(m),
                })?;
                decode(response)
            }
        })
        .await
    }
}

fn decode<T: DeserializeOwned>(response: HttpResponse) -> Result<T, GeminiError> {
    if (200..300).contains(&response.status) {
        return serde_json::from_slice(&response.body)
            .map_err(|e| GeminiError::BadResponse(e.to_string()));
    }
    let body = String::from_utf8_lossy(&response.body);
    Err(map_status(response.status, &body, response.retry_after))
}

fn map_status(status: u16, body: &str, retry_after: Option<std::time::Duration>) -> GeminiError {
    let message = serde_json::from_str::<ErrorEnvelope>(body)
        .map(|e| e.error.message)
        .ok()
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| format!("HTTP {status}"));
    let is_key_problem = status == 401
        || status == 403
        || (status == 400 && message.to_lowercase().contains("api key"));
    if is_key_problem {
        return GeminiError::InvalidKey(message);
    }
    GeminiError::Api {
        status,
        message,
        retry_after,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Duration;

    use super::*;
    use crate::gemini::models::Content;
    use crate::gemini::transport::mock::{MockTransport, status, status_retry_after};

    const OK_BODY: &str = r#"{"candidates":[{"content":{"role":"model","parts":[{"text":"hi"}]},"finishReason":"STOP"}]}"#;
    const BUSY_BODY: &str = r#"{"error":{"code":503,"message":"The model is overloaded. Please try again later.","status":"UNAVAILABLE"}}"#;

    fn key() -> ApiKey {
        ApiKey::parse("AIzaSyTESTTESTTESTTESTTESTTEST123").unwrap()
    }

    fn client(
        responses: Vec<Result<HttpResponse, TransportError>>,
    ) -> (GeminiClient, Arc<MockTransport>) {
        let mock = Arc::new(MockTransport::new(responses));
        let c = GeminiClient::with_transport(mock.clone(), &ConcurrencySettings::default());
        (c, mock)
    }

    fn recording_hooks() -> (RetryHooks, Arc<Mutex<Vec<RetryNotice>>>) {
        let notices = Arc::new(Mutex::new(vec![]));
        let n = notices.clone();
        let hooks = RetryHooks::new(
            Default::default(),
            Arc::new(move |r: &RetryNotice| n.lock().unwrap().push(r.clone())),
        );
        (hooks, notices)
    }

    fn body() -> GenerateContentRequest {
        GenerateContentRequest {
            contents: vec![Content::user("hello")],
            ..Default::default()
        }
    }

    fn busy() -> Result<HttpResponse, TransportError> {
        status(503, BUSY_BODY)
    }

    #[tokio::test(start_paused = true)]
    async fn retries_503_then_succeeds_with_growing_delays() {
        let (c, mock) = client(vec![busy(), status(502, ""), status(200, OK_BODY)]);
        let (hooks, notices) = recording_hooks();
        let r = c
            .generate_content(&key(), "m1", EndpointKind::Text, &body(), &hooks)
            .await
            .unwrap();
        assert_eq!(r.text().unwrap(), "hi");
        assert_eq!(mock.urls().len(), 3);
        let n = notices.lock().unwrap();
        assert_eq!(n.len(), 2);
        assert_eq!((n[0].attempt, n[0].status), (2, Some(503)));
        assert!(
            (2_000..=3_000).contains(&n[0].delay_ms),
            "{}",
            n[0].delay_ms
        );
        assert!(
            (4_000..=5_000).contains(&n[1].delay_ms),
            "{}",
            n[1].delay_ms
        );
        assert_eq!(n[0].reason, RetryReason::Busy);
    }

    #[tokio::test(start_paused = true)]
    async fn honors_retry_after_on_429() {
        let (c, _) = client(vec![status_retry_after(429, 7), status(200, OK_BODY)]);
        let (hooks, notices) = recording_hooks();
        c.generate_content(&key(), "m1", EndpointKind::Text, &body(), &hooks)
            .await
            .unwrap();
        let n = notices.lock().unwrap();
        assert_eq!(n[0].reason, RetryReason::RateLimited);
        assert!(
            (7_000..=8_000).contains(&n[0].delay_ms),
            "{}",
            n[0].delay_ms
        );
    }

    #[tokio::test(start_paused = true)]
    async fn retries_timeouts_and_network_errors() {
        let (c, mock) = client(vec![
            Err(TransportError::Timeout),
            Err(TransportError::Network("connection reset".into())),
            status(200, OK_BODY),
        ]);
        let (hooks, notices) = recording_hooks();
        c.generate_content(&key(), "m1", EndpointKind::Text, &body(), &hooks)
            .await
            .unwrap();
        assert_eq!(mock.urls().len(), 3);
        assert!(
            notices
                .lock()
                .unwrap()
                .iter()
                .all(|n| n.reason == RetryReason::Network)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn gives_up_after_five_503s() {
        let (c, mock) = client((0..6).map(|_| busy()).collect());
        let (hooks, notices) = recording_hooks();
        let err = c
            .generate_content(&key(), "m1", EndpointKind::Text, &body(), &hooks)
            .await
            .unwrap_err();
        assert!(err.is_overloaded());
        assert_eq!(err.kind(), "busy");
        assert_eq!(mock.urls().len(), MAX_ATTEMPTS as usize);
        assert_eq!(notices.lock().unwrap().len(), MAX_ATTEMPTS as usize - 1);
    }

    #[tokio::test(start_paused = true)]
    async fn bad_request_is_not_retried() {
        let (c, mock) = client(vec![status(
            400,
            r#"{"error":{"code":400,"message":"bad field"}}"#,
        )]);
        let err = c
            .generate_content(
                &key(),
                "m1",
                EndpointKind::Text,
                &body(),
                &RetryHooks::default(),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, GeminiError::Api { status: 400, .. }));
        assert_eq!(mock.urls().len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn falls_back_after_primary_stays_busy_and_sticks_to_it() {
        let mut responses: Vec<_> = (0..5).map(|_| busy()).collect();
        responses.push(status(200, OK_BODY)); // fallback answers
        responses.push(status(200, OK_BODY)); // next call goes straight to fallback
        let (c, mock) = client(responses);
        let (hooks, notices) = recording_hooks();
        let models = TextModels::new("primary-model", "fallback-model");

        c.generate_text(&key(), &models, &body(), &hooks)
            .await
            .unwrap();
        c.generate_text(&key(), &models, &body(), &hooks)
            .await
            .unwrap();

        let urls = mock.urls();
        assert_eq!(urls.len(), 7);
        assert!(
            urls[..5]
                .iter()
                .all(|u| u.contains("/models/primary-model:"))
        );
        assert!(
            urls[5..]
                .iter()
                .all(|u| u.contains("/models/fallback-model:"))
        );
        let n = notices.lock().unwrap();
        let fallback: Vec<_> = n
            .iter()
            .filter(|x| x.reason == RetryReason::Fallback)
            .collect();
        assert_eq!(fallback.len(), 1);
        assert_eq!(fallback[0].model, "fallback-model");
    }

    #[tokio::test(start_paused = true)]
    async fn no_fallback_for_other_errors_or_when_unset() {
        // 429 exhausted → no fallback (fallback is only for overload).
        let (c, mock) = client((0..5).map(|_| status_retry_after(429, 1)).collect());
        let models = TextModels::new("p", "f");
        let err = c
            .generate_text(&key(), &models, &body(), &RetryHooks::default())
            .await
            .unwrap_err();
        assert_eq!(err.kind(), "rate_limited");
        assert!(mock.urls().iter().all(|u| u.contains("/models/p:")));

        // Fallback disabled (empty) → the 503 is returned.
        let (c, mock) = client((0..5).map(|_| busy()).collect());
        let models = TextModels::new("p", "  ");
        assert!(
            c.generate_text(&key(), &models, &body(), &RetryHooks::default())
                .await
                .unwrap_err()
                .is_overloaded()
        );
        assert_eq!(mock.urls().len(), 5);
    }

    #[tokio::test(start_paused = true)]
    async fn key_is_sent_as_header_not_in_url() {
        let (c, mock) = client(vec![status(200, r#"{"name":"models/m1"}"#)]);
        c.get_model(&key(), "m1", &RetryHooks::default())
            .await
            .unwrap();
        let calls = mock.calls.lock().unwrap();
        assert_eq!(calls[0].0, Method::Get);
        assert!(calls[0].2, "key header missing");
        assert!(!calls[0].1.contains("AIza"));
    }

    #[test]
    fn invalid_key_message_is_mapped() {
        let body = r#"{"error":{"code":400,"message":"API key not valid. Please pass a valid API key.","status":"INVALID_ARGUMENT"}}"#;
        assert!(matches!(
            map_status(400, body, None),
            GeminiError::InvalidKey(_)
        ));
        assert!(matches!(
            map_status(403, "", None),
            GeminiError::InvalidKey(_)
        ));
    }

    #[test]
    fn rate_limit_keeps_retry_after() {
        let e = map_status(429, "not json", Some(Duration::from_secs(3)));
        assert!(e.is_retryable());
        assert_eq!(e.retry_after(), Some(Duration::from_secs(3)));
    }
}
