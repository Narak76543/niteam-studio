//! Backoff rules from `docs/03-pipeline.md`: 2 s, 4 s, 8 s, 16 s, 32 s (+ jitter),
//! honor `Retry-After`, at most 5 attempts, no retry on 4xx other than 408/429.

use std::future::Future;
use std::time::Duration;

use rand::Rng;
use tokio_util::sync::CancellationToken;

use super::GeminiError;

pub const MAX_ATTEMPTS: u32 = 5;
const BASE_DELAY: Duration = Duration::from_secs(2);
const MAX_JITTER_MS: u64 = 1_000;
/// Upper bound for a server-provided `Retry-After`, so a bad header can't hang a job.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(120);

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

/// Runs `op` until it succeeds, fails with a non-retryable error, runs out of
/// attempts, or `cancel` fires.
pub async fn with_retry<T, F, Fut>(cancel: &CancellationToken, mut op: F) -> Result<T, GeminiError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, GeminiError>>,
{
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
            Err(e) if e.is_retryable() && attempt < MAX_ATTEMPTS => {
                let delay = backoff_delay(attempt, e.retry_after(), random_jitter());
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
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn api(status: u16) -> GeminiError {
        GeminiError::Api {
            status,
            message: "x".into(),
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
        for s in [408, 429, 500, 503, 504] {
            assert!(api(s).is_retryable(), "{s}");
        }
        for s in [400, 401, 402, 403, 404] {
            assert!(!api(s).is_retryable(), "{s}");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn gives_up_after_five_attempts() {
        let calls = Arc::new(AtomicU32::new(0));
        let c = calls.clone();
        let r: Result<(), _> = with_retry(&CancellationToken::new(), || {
            c.fetch_add(1, Ordering::SeqCst);
            async { Err(api(429)) }
        })
        .await;
        assert!(r.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), MAX_ATTEMPTS);
    }

    #[tokio::test(start_paused = true)]
    async fn does_not_retry_client_errors() {
        let calls = Arc::new(AtomicU32::new(0));
        let c = calls.clone();
        let r: Result<(), _> = with_retry(&CancellationToken::new(), || {
            c.fetch_add(1, Ordering::SeqCst);
            async { Err(api(400)) }
        })
        .await;
        assert!(r.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn succeeds_after_transient_failure() {
        let calls = Arc::new(AtomicU32::new(0));
        let c = calls.clone();
        let r = with_retry(&CancellationToken::new(), || {
            let n = c.fetch_add(1, Ordering::SeqCst);
            async move { if n < 2 { Err(api(503)) } else { Ok(n) } }
        })
        .await;
        assert_eq!(r.unwrap(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn cancel_stops_waiting() {
        let token = CancellationToken::new();
        token.cancel();
        let r: Result<(), _> = with_retry(&token, || async { Err(api(429)) }).await;
        assert!(matches!(r, Err(GeminiError::Cancelled)));
    }
}
