//! HTTP layer behind a trait so retry / fallback / quota logic can be tested with a mock.
//! Production uses [`ReqwestTransport`]. Shared by the Gemini and Cloudflare clients.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use reqwest::header::{CONTENT_TYPE, HeaderValue, RETRY_AFTER, USER_AGENT};

use crate::storage::ApiKey;
use crate::storage::secrets::{CloudflareToken, StockKey};

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
    /// Pexels: `Authorization: <key>` (no scheme).
    PlainAuthorization(StockKey),
    /// Pixabay: `?key=<key>`. The only API that wants the key in the URL; it is added when
    /// sending, so `HttpRequest::url` (and every error message) never contains it.
    QueryKey(&'static str, StockKey),
    /// Public APIs (Openverse anonymous, Wikimedia Commons).
    None,
}

impl Auth {
    fn header(&self) -> Option<(&'static str, String)> {
        match self {
            Self::GoogApiKey(k) => Some(("x-goog-api-key", k.expose().to_owned())),
            Self::Bearer(t) => Some(("authorization", format!("Bearer {}", t.expose()))),
            Self::PlainAuthorization(k) => Some(("authorization", k.expose().to_owned())),
            Self::QueryKey(..) | Self::None => None,
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
    /// Replaces the default `NiteanStudio/<version>` (Wikimedia wants contact details).
    pub user_agent: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub retry_after: Option<Duration>,
    /// `x-ratelimit-*` headers, names lower-cased (stock APIs report their quota here).
    pub rate_headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// A rate-limit header by lower-case name.
    pub fn rate_header(&self, name: &str) -> Option<&str> {
        self.rate_headers.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }
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
            let builder = match request.method {
                Method::Get => self.http.get(&request.url),
                Method::Post => self
                    .http
                    .post(&request.url)
                    .header(CONTENT_TYPE, "application/json")
                    .body(request.body.unwrap_or_default()),
            };
            let mut builder = match request.auth.header() {
                Some((name, value)) => {
                    let mut auth = HeaderValue::from_str(&value).map_err(|_| {
                        TransportError::Network("the key or token contains invalid characters".into())
                    })?;
                    auth.set_sensitive(true);
                    builder.header(name, auth)
                }
                None => builder,
            };
            if let Auth::QueryKey(param, key) = &request.auth {
                builder = builder.query(&[(*param, key.expose())]);
            }
            if let Some(ua) = &request.user_agent {
                builder = builder.header(USER_AGENT, ua);
            }
            let resp = builder.send().await.map_err(map_reqwest)?;
            let status = resp.status().as_u16();
            let retry_after = parse_retry_after(resp.headers().get(RETRY_AFTER));
            let rate_headers = resp
                .headers()
                .iter()
                .filter(|(n, _)| n.as_str().starts_with("x-ratelimit"))
                .filter_map(|(n, v)| Some((n.as_str().to_owned(), v.to_str().ok()?.to_owned())))
                .collect();
            let body = resp.bytes().await.map_err(map_reqwest)?.to_vec();
            Ok(HttpResponse {
                status,
                retry_after,
                rate_headers,
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
            rate_headers: vec![],
            body: body.as_bytes().to_vec(),
        })
    }

    pub fn status_retry_after(code: u16, secs: u64) -> Result<HttpResponse, TransportError> {
        Ok(HttpResponse {
            status: code,
            retry_after: Some(Duration::from_secs(secs)),
            rate_headers: vec![],
            body: br#"{"error":{"code":429,"message":"slow down"}}"#.to_vec(),
        })
    }

    impl Transport for MockTransport {
        fn send(&self, request: HttpRequest) -> TransportFuture<'_> {
            self.calls.lock().unwrap().push((
                request.method,
                request.url.clone(),
                request.auth.header().is_some_and(|(_, v)| !v.is_empty()),
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
