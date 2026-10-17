//! One search across the enabled sources (`docs/11-real-world-visuals.md` §4): cache first,
//! one request per source, license filter, local ranking. A source that fails or hit its limit
//! is reported as a notice and skipped until its limit resets; the others still answer.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use serde::Serialize;

use super::cache::{SearchCache, key};
use super::{Candidate, Media, Source, StockError, commons, openverse, pexels, pixabay, rank, transport_error};
use crate::net::transport::Transport;
use crate::storage::secrets::StockKey;
use crate::storage::settings::StockSettings;

#[derive(Clone, Default)]
pub struct Keys {
    pub pexels: Option<StockKey>,
    pub pixabay: Option<StockKey>,
}

#[derive(Debug, Clone)]
pub struct Query {
    /// What is sent to the sources (English).
    pub text: String,
    /// The scene's keywords, for ranking.
    pub keywords: Vec<String>,
    pub media: Media,
    pub page: u32,
    pub frame_width: u32,
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SourceNotice {
    pub source: Source,
    /// `StockError::kind()`.
    pub kind: String,
    pub message: String,
    /// When a rate-limited source is tried again (RFC 3339).
    pub retry_at_utc: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SearchOutcome {
    pub candidates: Vec<Candidate>,
    pub notices: Vec<SourceNotice>,
}

pub struct Searcher {
    transport: Arc<dyn Transport>,
    cache: SearchCache,
    keys: Keys,
    settings: StockSettings,
    /// Sources waiting for their limit to reset.
    paused: Mutex<HashMap<Source, DateTime<Utc>>>,
}

impl Searcher {
    pub fn new(transport: Arc<dyn Transport>, cache: SearchCache, keys: Keys, settings: StockSettings) -> Self {
        Self { transport, cache, keys, settings, paused: Mutex::new(HashMap::new()) }
    }

    /// Enabled sources for this kind of media. Video: Pexels and Pixabay only.
    pub fn sources(&self, media: Media) -> Vec<Source> {
        let s = &self.settings;
        let mut out = Vec::new();
        if s.pixabay {
            out.push(Source::Pixabay);
        }
        if s.pexels && self.keys.pexels.is_some() {
            out.push(Source::Pexels);
        }
        if media == Media::Photo {
            if s.commons {
                out.push(Source::Wikimedia);
            }
            if s.openverse {
                out.push(Source::Openverse);
            }
        }
        out
    }

    fn paused_until(&self, source: Source, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let mut paused = self.paused.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        match paused.get(&source) {
            Some(t) if *t > now => Some(*t),
            Some(_) => {
                paused.remove(&source);
                None
            }
            None => None,
        }
    }

    async fn one(&self, source: Source, q: &Query, now: DateTime<Utc>) -> Result<Vec<Candidate>, StockError> {
        let by_sa = self.settings.allow_by_sa;
        let variant = if source == Source::Openverse && by_sa { "by-sa" } else { "" };
        let cache_key = key(source, q.media, &q.text, q.page, variant);
        if let Some(hit) = self.cache.get(source, &cache_key, now) {
            return Ok(hit);
        }
        let request = match source {
            Source::Pixabay => pixabay::request(&q.text, q.media, q.page, self.keys.pixabay.as_ref().ok_or(StockError::NoKey(source))?)?,
            Source::Pexels => pexels::request(&q.text, q.media, q.page, self.keys.pexels.as_ref().ok_or(StockError::NoKey(source))?)?,
            Source::Wikimedia => commons::request(&q.text, q.page)?,
            Source::Openverse => openverse::request(&q.text, q.page, by_sa)?,
            Source::User => return Ok(vec![]),
        };
        let resp = self.transport.send(request).await.map_err(|e| transport_error(source, e))?;
        let found = match source {
            Source::Pixabay => pixabay::check(&resp).and_then(|()| pixabay::parse(&resp.body, q.media)),
            Source::Pexels => pexels::check(&resp).and_then(|()| pexels::parse(&resp.body, q.media)),
            Source::Wikimedia => commons::check(&resp).and_then(|()| commons::parse(&resp.body)),
            Source::Openverse => openverse::check(&resp).and_then(|()| openverse::parse(&resp.body)),
            Source::User => Ok(vec![]),
        }?;
        // A full cache disk must not fail the search.
        let _ = self.cache.put(source, &cache_key, &found, now);
        Ok(found)
    }

    pub async fn search(&self, q: &Query) -> SearchOutcome {
        let now = Utc::now();
        let mut all = Vec::new();
        let mut notices = Vec::new();
        if q.media == Media::Video && self.sources(Media::Video).is_empty() {
            notices.push(SourceNotice {
                source: Source::Pixabay,
                kind: "stock_no_key".into(),
                message: "Stock videos need a Pixabay (or Pexels) key in Settings.".into(),
                retry_at_utc: None,
            });
        }
        for source in self.sources(q.media) {
            if let Some(until) = self.paused_until(source, now) {
                notices.push(SourceNotice { source, kind: "stock_rate_limited".into(), message: format!("{} limit reached", source.display()), retry_at_utc: Some(until.to_rfc3339()) });
                continue;
            }
            match self.one(source, q, now).await {
                Ok(found) => all.extend(found.into_iter().filter(|c| c.license.allowed(self.settings.allow_by_sa))),
                Err(e) => {
                    let retry_at = match &e {
                        StockError::RateLimited { retry_after, .. } => {
                            let until = now + chrono::Duration::from_std(*retry_after).unwrap_or(chrono::Duration::hours(1));
                            self.paused.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(source, until);
                            Some(until.to_rfc3339())
                        }
                        _ => None,
                    };
                    notices.push(SourceNotice { source, kind: e.kind().into(), message: e.to_string(), retry_at_utc: retry_at });
                }
            }
        }
        SearchOutcome { candidates: rank::rank(all, &q.keywords, q.frame_width, q.limit), notices }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::transport::mock::{self, MockTransport};

    const COMMONS: &str = r#"{"query":{"pages":[{"pageid":1,"title":"File:Submarine cable.jpg","index":1,"imageinfo":[{"size":10,"width":4000,"height":2250,
      "url":"https://u/1.jpg","descriptionurl":"https://c/1","mime":"image/jpeg","extmetadata":{"LicenseShortName":{"value":"CC BY-SA 4.0"}}}]},
      {"pageid":2,"title":"File:Cable ship.jpg","index":2,"imageinfo":[{"size":10,"width":3000,"height":2000,
      "url":"https://u/2.jpg","descriptionurl":"https://c/2","mime":"image/jpeg","extmetadata":{"LicenseShortName":{"value":"CC BY 2.0"}}}]}]}}"#;
    const OPENVERSE: &str = r#"{"results":[{"id":"a","title":"submarine cable landing","url":"https://x/a.jpg","license":"cc0","foreign_landing_url":"https://x/a","width":2560,"height":1440}]}"#;

    fn searcher(responses: Vec<Result<crate::net::transport::HttpResponse, crate::net::transport::TransportError>>, dir: &std::path::Path) -> (Searcher, Arc<MockTransport>) {
        let t = Arc::new(MockTransport::new(responses));
        let s = Searcher::new(t.clone(), SearchCache::new(dir.to_path_buf()), Keys::default(), StockSettings::default());
        (s, t)
    }

    fn query() -> Query {
        Query { text: "submarine cable".into(), keywords: vec!["submarine cable".into()], media: Media::Photo, page: 1, frame_width: 1920, limit: 6 }
    }

    #[tokio::test]
    async fn works_without_keys_filters_by_sa_and_caches() {
        let dir = tempfile::tempdir().unwrap();
        let (s, t) = searcher(vec![mock::status(200, COMMONS), mock::status(200, OPENVERSE)], dir.path());
        assert_eq!(s.sources(Media::Photo), vec![Source::Pixabay, Source::Wikimedia, Source::Openverse]);
        let out = s.search(&query()).await;
        assert_eq!(out.candidates.len(), 2, "BY-SA file dropped: {:#?}", out.candidates);
        assert!(out.candidates.iter().all(|c| c.license.code != crate::stock::license::LicenseCode::BySa));
        assert_eq!(out.notices.len(), 1, "Pixabay without key: {:?}", out.notices);
        assert_eq!(out.notices[0].kind, "stock_no_key");
        assert_eq!(t.urls().len(), 2);
        let again = s.search(&query()).await;
        assert_eq!(again.candidates, out.candidates);
        assert_eq!(t.urls().len(), 2, "second search served from the cache");
    }

    #[tokio::test]
    async fn rate_limited_source_is_paused_others_still_answer() {
        let dir = tempfile::tempdir().unwrap();
        let mut limited = mock::status_retry_after(429, 120).unwrap();
        limited.rate_headers = vec![];
        let (mut s, t) = searcher(vec![Ok(limited), mock::status(200, OPENVERSE)], dir.path());
        s.settings.pixabay = false;
        let out = s.search(&query()).await;
        assert_eq!(out.candidates.len(), 1);
        assert_eq!(out.notices[0].kind, "stock_rate_limited");
        assert!(out.notices[0].retry_at_utc.is_some());
        let mut q = query();
        q.text = "cable ship".into();
        let second = s.search(&q).await;
        assert_eq!(second.notices[0].kind, "stock_rate_limited");
        let commons = t.urls().iter().filter(|u| u.contains("commons.wikimedia.org")).count();
        assert_eq!(commons, 1, "paused source not asked again");
    }

    /// Live: Commons + Openverse without keys. Run with `--ignored --nocapture`.
    #[tokio::test]
    #[ignore = "network"]
    async fn live_search_without_keys() {
        let dir = tempfile::tempdir().unwrap();
        let t = Arc::new(crate::net::transport::ReqwestTransport::new().unwrap());
        let s = Searcher::new(t, SearchCache::new(dir.path().to_path_buf()), Keys::default(), StockSettings::default());
        let mut q = query();
        q.keywords = vec!["submarine cable".into(), "undersea cable".into()];
        let out = s.search(&q).await;
        for c in &out.candidates {
            println!("{:>5.1} {:<9} {:>5}x{:<5} low_res={} {:<14} restr={:?} {}", c.score, c.source.as_str(), c.width, c.height, c.low_res, c.license.label(), c.restrictions, c.title);
        }
        println!("notices: {:?}", out.notices);
        assert!(!out.candidates.is_empty());
    }

    #[tokio::test]
    async fn video_without_keys_says_what_to_do() {
        let dir = tempfile::tempdir().unwrap();
        let (s, _) = searcher(vec![], dir.path());
        let mut q = query();
        q.media = Media::Video;
        let out = s.search(&q).await;
        assert!(out.candidates.is_empty());
        assert_eq!(out.notices[0].kind, "stock_no_key");
    }
}
