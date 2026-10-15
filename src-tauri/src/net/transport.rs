//! HTTP layer behind a trait so retry / fallback / quota logic can be tested with a mock.
//! Production uses [`ReqwestTransport`]. Shared by the Gemini and Cloudflare clients.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use reqwest::header::{CONTENT_TYPE, HeaderValue, RETRY_AFTER};

use crate::storage::ApiKey;
use crate::storage::secrets::CloudflareToken;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(180);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

/// Credentials, always sent as a header (never in the URL, so they can't leak via errors).
#[derive(Clone)]
pub enum Auth {
    /// Gemini: `x-goog-api-key: <key>`
    GoogApiKey(ApiKey),
    /// Cloudflare: `Authorization: Bearer <token>`
    Bearer(CloudflareToken),
}

impl Auth {
    fn header(&self) -> (&'static str, String) {
        match self {
            Self::GoogApiKey(k) => ("x-goog-api-key", k.expose().to_owned()),
            Self::Bearer(t) => ("authorization", format!("Bearer {}", t.expose())),
        }
    }
}

/// One HTTP request. No `Debug` derive: it holds credentials.
#[derive(Clone)]
pub struct HttpRequest {
    pub method: Method,
    pub url: String,
    pub auth: Auth,
    /// JSON body for POST.
    pub body: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub retry_after: Option<Duration>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum TransportError {
    #[error("the request timed out")]
    Timeout,
    #[error("{0}")]
    Network(String),
}

pub type TransportFuture<'a> =
    Pin<Box<dyn Future<Output = Result<HttpResponse, TransportError>> + Send + 'a>>;

pub trait Transport: Send + Sync {
    fn send(&self, request: HttpRequest) -> TransportFuture<'_>;
}

pub struct ReqwestTransport {
    http: reqwest::Client,
}

impl ReqwestTransport {
    pub fn new() -> Result<Self, TransportError> {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .user_agent(concat!("NiteanStudio/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| TransportError::Network(e.to_string()))?;
        Ok(Self { http })
    }
}

fn map_reqwest(e: reqwest::Error) -> TransportError {
    if e.is_timeout() {
        TransportError::Timeout
    } else {
        TransportError::Network(e.without_url().to_string())
    }
}

/// `Retry-After` in seconds (the HTTP-date form is not used by Google APIs).
pub fn parse_retry_after(value: Option<&HeaderValue>) -> Option<Duration> {
    value
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
}

impl Transport for ReqwestTransport {
    fn send(&self, request: HttpRequest) -> TransportFuture<'_> {
        Box::pin(async move {
            let (name, value) = request.auth.header();
            let mut auth = HeaderValue::from_str(&value).map_err(|_| {
                TransportError::Network("the key or token contains invalid characters".into())
            })?;
            auth.set_sensitive(true);
            let builder = match request.method {
                Method::Get => self.http.get(&request.url),
                Method::Post => self
                    .http
                    .post(&request.url)
                    .header(CONTENT_TYPE, "application/json")
                    .body(request.body.unwrap_or_default()),
            };
            let resp = builder
                .header(name, auth)
                .send()
                .await
                .map_err(map_reqwest)?;
            let status = resp.status().as_u16();
            let retry_after = parse_retry_after(resp.headers().get(RETRY_AFTER));
            let body = resp.bytes().await.map_err(map_reqwest)?.to_vec();
            Ok(HttpResponse {
                status,
                retry_after,
                body,
            })
        })
    }
}

#[cfg(test)]
pub mod mock {
    //! Scripted transport: returns queued responses in order and records each request.

    use std::collections::VecDeque;
    use std::sync::Mutex;

    use super::*;

    pub struct MockTransport {
        responses: Mutex<VecDeque<Result<HttpResponse, TransportError>>>,
        /// (method, url, had_auth_header) of every request.
        pub calls: Mutex<Vec<(Method, String, bool)>>,
    }

    impl MockTransport {
        pub fn new(responses: Vec<Result<HttpResponse, TransportError>>) -> Self {
            Self {
                responses: Mutex::new(responses.into()),
                calls: Mutex::new(vec![]),
            }
        }

        pub fn urls(&self) -> Vec<String> {
            self.calls
                .lock()
                .unwrap()
                .iter()
                .map(|c| c.1.clone())
                .collect()
        }
    }

    pub fn status(code: u16, body: &str) -> Result<HttpResponse, TransportError> {
        Ok(HttpResponse {
            status: code,
            retry_after: None,
            body: body.as_bytes().to_vec(),
        })
    }

    pub fn status_retry_after(code: u16, secs: u64) -> Result<HttpResponse, TransportError> {
        Ok(HttpResponse {
            status: code,
            retry_after: Some(Duration::from_secs(secs)),
            body: br#"{"error":{"code":429,"message":"slow down"}}"#.to_vec(),
        })
    }

    impl Transport for MockTransport {
        fn send(&self, request: HttpRequest) -> TransportFuture<'_> {
            self.calls.lock().unwrap().push((
                request.method,
                request.url.clone(),
                !request.auth.header().1.is_empty(),
            ));
            let next = self
                .responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err(TransportError::Network("mock: no more responses".into())));
            Box::pin(async move { next })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_retry_after_seconds() {
        let v = HeaderValue::from_static("12");
        assert_eq!(parse_retry_after(Some(&v)), Some(Duration::from_secs(12)));
        let v = HeaderValue::from_static("Wed, 21 Oct 2026 07:28:00 GMT");
        assert_eq!(parse_retry_after(Some(&v)), None);
        assert_eq!(parse_retry_after(None), None);
    }
}
