//! Client for the Gemini REST API.
//! - Every call goes through [`retry::with_retry`] (one place for all Gemini traffic).
//! - Key goes in the `x-goog-api-key` header (never in the URL, so it can't leak via errors).
//! - One semaphore per endpoint kind limits concurrent requests; a permit is held per
//!   HTTP attempt, not during backoff waits.
//! - Text calls walk a model chain: 3 attempts per model, then the next model.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde::de::DeserializeOwned;
use tokio::sync::Semaphore;

use super::GeminiError;
use super::models::{ErrorEnvelope, GenerateContentRequest, GenerateContentResponse, ModelInfo};
use super::retry::{self, MAX_ATTEMPTS, RetryHooks, RetryNotice, RetryReason, TEXT_CHAIN_ATTEMPTS};
use crate::net::transport::{
    Auth, HttpRequest, HttpResponse, Method, ReqwestTransport, Transport, TransportError,
};
use crate::storage::ApiKey;
use crate::storage::settings::ConcurrencySettings;

const BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";
/// Raw error bodies kept for logs / "Details" are cut to this many characters.
const MAX_ERROR_BODY_CHARS: usize = 2_000;

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // Tts (Phase 2) and Image (Phase 3) are used by later pipeline steps.
pub enum EndpointKind {
    Text,
    Tts,
    Image,
}

/// Ordered text models for one job (`settings.models.text_chain`).
/// Each call starts at the model that last answered, so after a switch the rest of the
/// job doesn't wait on a busy model again. A new job starts at the first model.
pub struct ModelChain {
    models: Vec<String>,
    current: AtomicUsize,
}

impl ModelChain {
    pub fn new<S: AsRef<str>>(models: &[S]) -> Self {
        let mut list: Vec<String> = Vec::new();
        for m in models {
            let m = m.as_ref().trim();
            if !m.is_empty() && !list.iter().any(|x| x == m) {
                list.push(m.to_owned());
            }
        }
        Self {
            models: list,
            current: AtomicUsize::new(0),
        }
    }

    pub fn models(&self) -> &[String] {
        &self.models
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
            auth: Auth::GoogApiKey(key.clone()),
            body: None,
            user_agent: None,
        };
        self.call(EndpointKind::Text, model, request, hooks, MAX_ATTEMPTS)
            .await
    }

    /// `POST /models/{model}:generateContent` with up to `max_attempts` tries.
    pub async fn generate_content(
        &self,
        key: &ApiKey,
        model: &str,
        kind: EndpointKind,
        body: &GenerateContentRequest,
        hooks: &RetryHooks,
        max_attempts: u32,
    ) -> Result<GenerateContentResponse, GeminiError> {
        let body = serde_json::to_vec(body)
            .map_err(|e| GeminiError::BadResponse(format!("could not encode request: {e}")))?;
        let request = HttpRequest {
            method: Method::Post,
            url: format!("{}/models/{}:generateContent", self.base_url, model),
            auth: Auth::GoogApiKey(key.clone()),
            body: Some(body),
            user_agent: None,
        };
        self.call(kind, model, request, hooks, max_attempts).await
    }

    /// `POST /interactions` (Interactions API; used for TTS, see `gemini/tts.rs`).
    pub async fn create_interaction<T: DeserializeOwned, B: serde::Serialize>(
        &self,
        key: &ApiKey,
        model: &str,
        kind: EndpointKind,
        body: &B,
        hooks: &RetryHooks,
        max_attempts: u32,
    ) -> Result<T, GeminiError> {
        let body = serde_json::to_vec(body)
            .map_err(|e| GeminiError::BadResponse(format!("could not encode request: {e}")))?;
        let request = HttpRequest {
            method: Method::Post,
            url: format!("{}/interactions", self.base_url),
            auth: Auth::GoogApiKey(key.clone()),
            body: Some(body),
            user_agent: None,
        };
        self.call(kind, model, request, hooks, max_attempts).await
    }

    /// Text generation over the model chain: each model gets [`TEXT_CHAIN_ATTEMPTS`] tries
    /// (429 / 5xx / timeouts are retried with backoff); if it still fails with a transient
    /// error or 404, the next model is tried. Returns the response and the model that answered.
    pub async fn generate_text(
        &self,
        key: &ApiKey,
        chain: &ModelChain,
        body: &GenerateContentRequest,
        hooks: &RetryHooks,
    ) -> Result<(GenerateContentResponse, String), GeminiError> {
        let start = chain.current.load(Ordering::Relaxed);
        let mut tried: Vec<String> = Vec::new();
        let mut last: Option<GeminiError> = None;
        for (i, model) in chain.models.iter().enumerate().skip(start) {
            if let Some(prev_err) = &last {
                hooks.notify(&RetryNotice {
                    model: model.clone(),
                    previous_model: tried.last().cloned(),
                    reason: RetryReason::NextModel,
                    attempt: 1,
                    max_attempts: TEXT_CHAIN_ATTEMPTS,
                    delay_ms: 0,
                    status: prev_err.status(),
                    error: prev_err.detail(),
                });
            }
            match self
                .generate_content(
                    key,
                    model,
                    EndpointKind::Text,
                    body,
                    hooks,
                    TEXT_CHAIN_ATTEMPTS,
                )
                .await
            {
                Ok(response) => {
                    chain.current.store(i, Ordering::Relaxed);
                    return Ok((response, model.clone()));
                }
                Err(e) if e.try_next_model() => {
                    tried.push(model.clone());
                    last = Some(e);
                }
                Err(e) => return Err(e),
            }
        }
        match last {
            Some(last) => Err(GeminiError::ChainExhausted {
                tried,
                last: Box::new(last),
            }),
            None => Err(GeminiError::NoTextModel),
        }
    }

    async fn call<T: DeserializeOwned>(
        &self,
        kind: EndpointKind,
        model: &str,
        request: HttpRequest,
        hooks: &RetryHooks,
        max_attempts: u32,
    ) -> Result<T, GeminiError> {
        retry::with_retry(hooks, model, max_attempts, || {
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

fn truncate(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let cut: String = s.chars().take(max).collect();
    format!("{cut}… (truncated)")
}

fn map_status(status: u16, body: &str, retry_after: Option<std::time::Duration>) -> GeminiError {
    let message = serde_json::from_str::<ErrorEnvelope>(body)
        .map(|e| e.error.message)
        .ok()
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| format!("HTTP {status}"));
    let raw = if body.trim().is_empty() {
        "(empty body)".to_owned()
    } else {
        truncate(body, MAX_ERROR_BODY_CHARS)
    };
    let is_key_problem = status == 401
        || status == 403
        || (status == 400 && message.to_lowercase().contains("api key"));
    if is_key_problem {
        return GeminiError::InvalidKey {
            status,
            message,
            body: raw,
        };
    }
    GeminiError::Api {
        status,
        message,
        body: raw,
        retry_after,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Duration;

    use super::*;
    use crate::gemini::models::Content;
    use crate::net::transport::mock::{MockTransport, status, status_retry_after};

    const OK_BODY: &str = r#"{"candidates":[{"content":{"role":"model","parts":[{"text":"hi"}]},"finishReason":"STOP"}]}"#;
    const BUSY_BODY: &str = r#"{"error":{"code":503,"message":"This model is currently experiencing high demand.","status":"UNAVAILABLE"}}"#;

    fn key() -> ApiKey {
        ApiKey::parse("AIzaSyTESTTESTTESTTESTTESTTEST123").unwrap()
    }

    type Responses = Vec<Result<HttpResponse, TransportError>>;

    fn client(responses: Responses) -> (GeminiClient, Arc<MockTransport>) {
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

    fn busy_n(n: usize) -> Responses {
        (0..n).map(|_| busy()).collect()
    }

    fn models_called(mock: &MockTransport) -> Vec<String> {
        mock.urls()
            .iter()
            .map(|u| {
                u.split("/models/")
                    .nth(1)
                    .and_then(|r| r.split(':').next())
                    .unwrap_or_default()
                    .to_owned()
            })
            .collect()
    }

    // ---- single-model retry ----

    #[tokio::test(start_paused = true)]
    async fn retries_503_then_succeeds_with_growing_delays() {
        let (c, mock) = client(vec![busy(), status(502, ""), status(200, OK_BODY)]);
        let (hooks, notices) = recording_hooks();
        let r = c
            .generate_content(
                &key(),
                "m1",
                EndpointKind::Text,
                &body(),
                &hooks,
                MAX_ATTEMPTS,
            )
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
        // The exact status + raw body is carried for the log.
        assert!(n[0].error.starts_with("HTTP 503"), "{}", n[0].error);
        assert!(n[0].error.contains("high demand"));
        assert!(
            n[1].error.contains("HTTP 502 — (empty body)"),
            "{}",
            n[1].error
        );
    }

    #[tokio::test(start_paused = true)]
    async fn honors_retry_after_on_429() {
        let (c, _) = client(vec![status_retry_after(429, 7), status(200, OK_BODY)]);
        let (hooks, notices) = recording_hooks();
        c.generate_content(
            &key(),
            "m1",
            EndpointKind::Text,
            &body(),
            &hooks,
            MAX_ATTEMPTS,
        )
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
        c.generate_content(
            &key(),
            "m1",
            EndpointKind::Text,
            &body(),
            &hooks,
            MAX_ATTEMPTS,
        )
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
    async fn gives_up_after_max_attempts() {
        let (c, mock) = client(busy_n(6));
        let err = c
            .generate_content(
                &key(),
                "m1",
                EndpointKind::Text,
                &body(),
                &RetryHooks::default(),
                MAX_ATTEMPTS,
            )
            .await
            .unwrap_err();
        assert_eq!(err.kind(), "busy");
        assert_eq!(mock.urls().len(), MAX_ATTEMPTS as usize);
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
                MAX_ATTEMPTS,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, GeminiError::Api { status: 400, .. }));
        assert_eq!(mock.urls().len(), 1);
    }

    // ---- text model chain ----

    #[tokio::test(start_paused = true)]
    async fn chain_uses_three_attempts_per_model_then_moves_on_and_sticks() {
        // a: 3× 503 → b answers; next call starts at b.
        let mut r = busy_n(3);
        r.push(status(200, OK_BODY));
        r.push(status(200, OK_BODY));
        let (c, mock) = client(r);
        let (hooks, notices) = recording_hooks();
        let chain = ModelChain::new(&["a", "b", "c"]);

        let (_, used) = c
            .generate_text(&key(), &chain, &body(), &hooks)
            .await
            .unwrap();
        assert_eq!(used, "b");
        let (_, used) = c
            .generate_text(&key(), &chain, &body(), &hooks)
            .await
            .unwrap();
        assert_eq!(used, "b");

        assert_eq!(models_called(&mock), vec!["a", "a", "a", "b", "b"]);
        let n = notices.lock().unwrap();
        let busy: Vec<_> = n.iter().filter(|x| x.reason == RetryReason::Busy).collect();
        assert_eq!(busy.len(), 2, "3 attempts = 2 waits");
        assert!(
            busy.iter()
                .all(|x| x.max_attempts == TEXT_CHAIN_ATTEMPTS && x.model == "a")
        );
        assert!((2_000..=3_000).contains(&busy[0].delay_ms));
        assert!((4_000..=5_000).contains(&busy[1].delay_ms));
        let next: Vec<_> = n
            .iter()
            .filter(|x| x.reason == RetryReason::NextModel)
            .collect();
        assert_eq!(next.len(), 1);
        assert_eq!(next[0].model, "b");
        assert_eq!(next[0].previous_model.as_deref(), Some("a"));
    }

    #[tokio::test(start_paused = true)]
    async fn chain_exhausted_reports_every_model_and_last_error() {
        let (c, mock) = client(busy_n(9));
        let chain = ModelChain::new(&["a", "b", "c"]);
        let err = c
            .generate_text(&key(), &chain, &body(), &RetryHooks::default())
            .await
            .unwrap_err();
        assert_eq!(mock.urls().len(), 9);
        assert_eq!(err.kind(), "busy");
        let detail = err.detail();
        assert!(detail.contains("a → b → c"), "{detail}");
        assert!(detail.contains("HTTP 503"), "{detail}");
    }

    #[tokio::test(start_paused = true)]
    async fn chain_moves_on_after_404_and_timeouts_but_not_after_400() {
        // a: unknown model (404, no retry) → b: timeouts ×3 → c answers.
        let mut r = vec![status(
            404,
            r#"{"error":{"code":404,"message":"models/a is not found"}}"#,
        )];
        r.extend((0..3).map(|_| Err(TransportError::Timeout)));
        r.push(status(200, OK_BODY));
        let (c, mock) = client(r);
        let chain = ModelChain::new(&["a", "b", "c"]);
        let (_, used) = c
            .generate_text(&key(), &chain, &body(), &RetryHooks::default())
            .await
            .unwrap();
        assert_eq!(used, "c");
        assert_eq!(models_called(&mock), vec!["a", "b", "b", "b", "c"]);

        // 400 (bad request) is final: another model would get the same request.
        let (c, mock) = client(vec![status(
            400,
            r#"{"error":{"code":400,"message":"bad"}}"#,
        )]);
        let err = c
            .generate_text(
                &key(),
                &ModelChain::new(&["a", "b"]),
                &body(),
                &RetryHooks::default(),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, GeminiError::Api { status: 400, .. }));
        assert_eq!(mock.urls().len(), 1);
    }

    #[test]
    fn chain_drops_blanks_and_duplicates() {
        let c = ModelChain::new(&[" a ", "", "b", "a"]);
        assert_eq!(c.models(), ["a", "b"]);
    }

    #[tokio::test]
    async fn empty_chain_is_an_error() {
        let (c, _) = client(vec![]);
        let empty: [&str; 0] = [];
        let err = c
            .generate_text(
                &key(),
                &ModelChain::new(&empty),
                &body(),
                &RetryHooks::default(),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, GeminiError::NoTextModel));
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
            GeminiError::InvalidKey { status: 400, .. }
        ));
        assert!(matches!(
            map_status(403, "", None),
            GeminiError::InvalidKey { .. }
        ));
    }

    #[test]
    fn rate_limit_keeps_retry_after_and_raw_body() {
        let e = map_status(429, "not json", Some(Duration::from_secs(3)));
        assert!(e.is_retryable());
        assert_eq!(e.retry_after(), Some(Duration::from_secs(3)));
        assert_eq!(e.detail(), "HTTP 429 — not json");
        let long = "x".repeat(MAX_ERROR_BODY_CHARS + 50);
        assert!(
            map_status(500, &long, None)
                .detail()
                .ends_with("(truncated)")
        );
    }
}
