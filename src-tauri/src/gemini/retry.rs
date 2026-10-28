//! Backoff rules from `docs/03-pipeline.md`: retry 408 / 429 / 5xx / timeouts / network
//! errors with 2 s, 4 s, 8 s, … (+ jitter) between attempts, honor `Retry-After`.
//! The attempt limit is per call: [`MAX_ATTEMPTS`] by default, [`TEXT_CHAIN_ATTEMPTS`] per
//! model of the text fallback chain. Each wait is reported through [`RetryHooks::on_retry`]
//! so the UI can show "Google is busy, retrying… (model X, attempt 2/3)".

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use rand::Rng;
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use super::GeminiError;

/// Default attempts for a single-model call (key test, later TTS / images).
pub const MAX_ATTEMPTS: u32 = 5;
/// Attempts per model in the text fallback chain before moving to the next model.
pub const TEXT_CHAIN_ATTEMPTS: u32 = 3;
const BASE_DELAY: Duration = Duration::from_secs(2);
const MAX_JITTER_MS: u64 = 1_000;
/// Upper bound for a server-provided `Retry-After`, so a bad header can't hang a job.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(120);

/// Why the client is waiting. Sent to the UI inside the `gemini-retry` event.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RetryReason {
    /// 500 / 502 / 503 / 504 — Google is overloaded.
    Busy,
    /// 429 — rate limit.
    RateLimited,
    /// Timeout, connection problem or 408.
    Network,
    /// The previous model of the text chain failed; switching to `model`.
    NextModel,
    /// The model answered, but the JSON failed validation; asking it (or the next model) to fix it.
    InvalidReply,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RetryNotice {
    pub model: String,
    /// For `NextModel`: the model that just gave up.
    pub previous_model: Option<String>,
    pub reason: RetryReason,
    /// The attempt that will run after the wait (2..=MAX_ATTEMPTS).
    pub attempt: u32,
    pub max_attempts: u32,
    pub delay_ms: u64,
    /// HTTP status of the failed attempt, if any.
    pub status: Option<u16>,
    /// Exact error of the failed attempt: HTTP status + raw body (for logs / "Details").
    pub error: String,
}

pub type RetryObserver = Arc<dyn Fn(&RetryNotice) + Send + Sync>;

/// Cancellation + progress reporting for one logical Gemini call.
#[derive(Clone, Default)]
pub struct RetryHooks {
    pub cancel: CancellationToken,
    pub on_retry: Option<RetryObserver>,
}

impl RetryHooks {
    pub fn new(cancel: CancellationToken, on_retry: RetryObserver) -> Self {
        Self {
            cancel,
            on_retry: Some(on_retry),
        }
    }

    pub fn notify(&self, notice: &RetryNotice) {
        if let Some(f) = &self.on_retry {
            f(notice);
        }
    }
}

/// Delay before the next attempt after `failed_attempt` (1-based) failed.
pub fn backoff_delay(
    failed_attempt: u32,
    retry_after: Option<Duration>,
    jitter: Duration,
) -> Duration {
    if let Some(ra) = retry_after {
        return ra.min(MAX_RETRY_AFTER) + jitter;
    }
    let exp = failed_attempt.saturating_sub(1).min(4);
    BASE_DELAY * 2u32.pow(exp) + jitter
}

fn random_jitter() -> Duration {
    Duration::from_millis(rand::thread_rng().gen_range(0..=MAX_JITTER_MS))
}

fn reason_for(err: &GeminiError) -> RetryReason {
    match err {
        GeminiError::Api { status: 429, .. } => RetryReason::RateLimited,
        GeminiError::Api { status: 408, .. } | GeminiError::Network(_) | GeminiError::Timeout => {
            RetryReason::Network
        }
        _ => RetryReason::Busy,
    }
}

/// Runs `op` until it succeeds, fails with a non-retryable error, runs out of
/// attempts, or `hooks.cancel` fires. `model` is only used for notices.
pub async fn with_retry<T, F, Fut>(
    hooks: &RetryHooks,
    model: &str,
    max_attempts: u32,
    mut op: F,
) -> Result<T, GeminiError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, GeminiError>>,
{
    let cancel = &hooks.cancel;
    let mut attempt = 1;
    loop {
        if cancel.is_cancelled() {
            return Err(GeminiError::Cancelled);
        }
        let result = tokio::select! {
            r = op() => r,
            _ = cancel.cancelled() => return Err(GeminiError::Cancelled),
        };
        match result {
            Ok(v) => return Ok(v),
            Err(e) if e.is_retryable() && attempt < max_attempts => {
                let delay = backoff_delay(attempt, e.retry_after(), random_jitter());
                hooks.notify(&RetryNotice {
                    model: model.to_owned(),
                    previous_model: None,
                    reason: reason_for(&e),
                    attempt: attempt + 1,
                    max_attempts,
                    delay_ms: u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
                    status: e.status(),
                    error: e.detail(),
                });
                tokio::select! {
                    _ = tokio::time::sleep(delay) => {}
                    _ = cancel.cancelled() => return Err(GeminiError::Cancelled),
                }
                attempt += 1;
            }
            Err(e) => return Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn api(status: u16) -> GeminiError {
        GeminiError::Api {
            status,
            message: "x".into(),
            body: String::new(),
            retry_after: None,
        }
    }

    #[test]
    fn delays_double_from_two_seconds() {
        let z = Duration::ZERO;
        let secs: Vec<u64> = (1..=5)
            .map(|a| backoff_delay(a, None, z).as_secs())
            .collect();
        assert_eq!(secs, vec![2, 4, 8, 16, 32]);
    }

    #[test]
    fn retry_after_wins_but_is_capped() {
        let z = Duration::ZERO;
        assert_eq!(
            backoff_delay(1, Some(Duration::from_secs(7)), z),
            Duration::from_secs(7)
        );
        assert_eq!(
            backoff_delay(1, Some(Duration::from_secs(9999)), z),
            MAX_RETRY_AFTER
        );
    }

    #[test]
    fn classifies_status_codes() {
        for s in [408, 429, 500, 502, 503, 504] {
            assert!(api(s).is_retryable(), "{s}");
        }
        assert!(GeminiError::Timeout.is_retryable());
        for s in [400, 401, 402, 403, 404] {
            assert!(!api(s).is_retryable(), "{s}");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn gives_up_after_five_attempts_and_reports_each_wait() {
        let calls = Arc::new(AtomicU32::new(0));
        let notices = Arc::new(Mutex::new(Vec::<RetryNotice>::new()));
        let n = notices.clone();
        let hooks = RetryHooks::new(
            CancellationToken::new(),
            Arc::new(move |r: &RetryNotice| n.lock().unwrap().push(r.clone())),
        );
        let c = calls.clone();
        let r: Result<(), _> = with_retry(&hooks, "m", MAX_ATTEMPTS, || {
            c.fetch_add(1, Ordering::SeqCst);
            async { Err(api(503)) }
        })
        .await;
        assert!(matches!(r, Err(GeminiError::Api { status: 503, .. })));
        assert_eq!(calls.load(Ordering::SeqCst), MAX_ATTEMPTS);
        let notices = notices.lock().unwrap();
        let attempts: Vec<u32> = notices.iter().map(|n| n.attempt).collect();
        assert_eq!(attempts, vec![2, 3, 4, 5]);
        assert!(notices.iter().all(|n| n.reason == RetryReason::Busy));
    }

    #[tokio::test(start_paused = true)]
    async fn does_not_retry_client_errors() {
        let calls = Arc::new(AtomicU32::new(0));
        let c = calls.clone();
        let r: Result<(), _> = with_retry(&RetryHooks::default(), "m", MAX_ATTEMPTS, || {
            c.fetch_add(1, Ordering::SeqCst);
            async { Err(api(400)) }
        })
        .await;
        assert!(r.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn cancel_stops_waiting() {
        let hooks = RetryHooks::default();
        hooks.cancel.cancel();
        let r: Result<(), _> =
            with_retry(&hooks, "m", MAX_ATTEMPTS, || async { Err(api(429)) }).await;
        assert!(matches!(r, Err(GeminiError::Cancelled)));
    }
}
