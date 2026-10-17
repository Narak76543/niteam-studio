//! Openverse image search, anonymous (`docs/11-real-world-visuals.md` §2). Checked 2026-10-17
//! against api.openverse.org/v1/schema and live replies:
//! `GET /v1/images/?q=&license=cc0,pdm,by[,by-sa]&excluded_source=wikimedia&page_size=20&page=`
//! → `results[]` {`id`, `title`, `url`, `thumbnail`, `creator`, `creator_url`, `license`,
//! `license_version`, `license_url`, `foreign_landing_url`, `width`, `height`, `source`,
//! `filetype`, `tags[].name`}. Anonymous limits (headers): 20 / min burst
//! (`x-ratelimit-*-anon_burst`), 200 / day sustained (`…-anon_sustained`); thumbnails have their
//! own 1000 / day. Wikimedia files come from Commons directly (`commons.rs`), not from here.

use std::time::Duration;

use serde::Deserialize;

use super::license::from_openverse;
use super::{Candidate, Media, Source, StockError, bad_json, check_status, looks_ai_generated, user_agent};
use crate::net::transport::{Auth, HttpRequest, HttpResponse, Method};

const API: &str = "https://api.openverse.org/v1/images/";
const PAGE_SIZE: u32 = 20;
const SUSTAINED_LEFT: &str = "x-ratelimit-available-anon_sustained";

pub fn request(query: &str, page: u32, allow_by_sa: bool) -> Result<HttpRequest, StockError> {
    let licenses = if allow_by_sa { "cc0,pdm,by,by-sa" } else { "cc0,pdm,by" };
    let url = reqwest::Url::parse_with_params(
        API,
        &[
            ("q", query.trim()),
            ("license", licenses),
            ("excluded_source", "wikimedia"),
            ("mature", "false"),
            ("page_size", &PAGE_SIZE.to_string()),
            ("page", &page.max(1).to_string()),
        ],
    )
    .map_err(|e| StockError::BadResponse { site: Source::Openverse, msg: e.to_string() })?;
    Ok(HttpRequest { method: Method::Get, url: url.into(), auth: Auth::None, body: None, user_agent: Some(user_agent()) })
}

#[derive(Deserialize)]
struct Reply {
    #[serde(default)]
    results: Vec<Item>,
}

#[derive(Deserialize)]
struct Item {
    id: String,
    #[serde(default)]
    title: Option<String>,
    url: String,
    #[serde(default)]
    thumbnail: Option<String>,
    #[serde(default)]
    creator: Option<String>,
    #[serde(default)]
    creator_url: Option<String>,
    license: String,
    #[serde(default)]
    license_version: Option<String>,
    #[serde(default)]
    license_url: Option<String>,
    foreign_landing_url: String,
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
    #[serde(default)]
    filetype: Option<String>,
    #[serde(default)]
    tags: Vec<Tag>,
}

#[derive(Deserialize)]
struct Tag {
    name: String,
}

/// Parses results; unknown sizes, non-JPEG/PNG files and other licenses are left out.
pub fn parse(body: &[u8]) -> Result<Vec<Candidate>, StockError> {
    let reply: Reply = serde_json::from_slice(body).map_err(|e| bad_json(Source::Openverse, e))?;
    let mut out = Vec::new();
    for r in reply.results {
        let (Some(width), Some(height)) = (r.width, r.height) else { continue };
        if !matches!(r.filetype.as_deref().map(str::to_lowercase).as_deref(), None | Some("jpg" | "jpeg" | "png")) {
            continue;
        }
        let Some(license) = from_openverse(&r.license, r.license_version.as_deref(), r.license_url.as_deref()) else {
            continue;
        };
        let title = r.title.unwrap_or_default();
        let tags: Vec<String> = r.tags.into_iter().map(|t| t.name).collect();
        let tag_refs: Vec<&str> = tags.iter().map(String::as_str).chain([title.as_str()]).collect();
        out.push(Candidate {
            source: Source::Openverse,
            ai_generated: looks_ai_generated(&tag_refs),
            source_id: r.id,
            media: Media::Photo,
            title,
            tags,
            author: r.creator.unwrap_or_default(),
            author_url: r.creator_url,
            license,
            page_url: r.foreign_landing_url,
            thumb_url: r.thumbnail.unwrap_or_else(|| r.url.clone()),
            file_url: r.url,
            width,
            height,
            duration_ms: None,
            restrictions: vec![],
            score: 0.0,
            low_res: false,
        });
    }
    Ok(out)
}

/// Status → error. 429: `Retry-After` when given; the daily (sustained) limit pauses for a day.
pub fn check(resp: &HttpResponse) -> Result<(), StockError> {
    if resp.status == 429 {
        let daily = resp.rate_header(SUSTAINED_LEFT) == Some("0");
        let fallback = Duration::from_secs(if daily { 24 * 3600 } else { 60 });
        return Err(StockError::RateLimited { site: Source::Openverse, retry_after: resp.retry_after.unwrap_or(fallback), daily });
    }
    check_status(Source::Openverse, resp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::transport::mock;
    use crate::stock::license::LicenseCode;

    /// Trimmed live reply (2026-10-17) plus an NC item and an item without a size.
    const SAMPLE: &str = r#"{"result_count":3,"page_count":1,"page_size":20,"page":1,"results":[
      {"id":"c025bb1a-e95b-435f-bd74-750ded7ff096","title":"Cable racks at grid computing","url":"https://images.rawpixel.com/x.jpg",
       "creator":"U.S. Department of Energy","creator_url":null,"tags":[{"name":"server"},{"name":"data center"}],"source":"rawpixel","provider":"rawpixel",
       "license":"cc0","license_version":"1.0","license_url":"https://creativecommons.org/publicdomain/zero/1.0/","foreign_landing_url":"https://www.rawpixel.com/image/1",
       "filetype":"jpg","width":3000,"height":2251,"thumbnail":"https://api.openverse.org/v1/images/c025bb1a-e95b-435f-bd74-750ded7ff096/thumb/"},
      {"id":"b","title":"28TB","url":"https://live.staticflickr.com/b.jpg","creator":"jared","creator_url":"https://www.flickr.com/photos/x","tags":[],
       "license":"by","license_version":"2.0","license_url":"https://creativecommons.org/licenses/by/2.0/","foreign_landing_url":"https://www.flickr.com/photos/x/1","filetype":null,"width":1024,"height":768,"thumbnail":"https://api.openverse.org/v1/images/b/thumb/"},
      {"id":"c","title":"nc","url":"https://x/c.jpg","license":"by-nc","license_version":"2.0","foreign_landing_url":"https://x/c","width":1000,"height":800},
      {"id":"d","title":"no size","url":"https://x/d.jpg","license":"cc0","foreign_landing_url":"https://x/d","width":null,"height":null}
    ]}"#;

    #[test]
    fn parses_fields_and_filters_licenses() {
        let c = parse(SAMPLE.as_bytes()).unwrap();
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].source_id, "c025bb1a-e95b-435f-bd74-750ded7ff096");
        assert_eq!(c[0].license.code, LicenseCode::Cc0);
        assert_eq!(c[0].tags, vec!["server", "data center"]);
        assert_eq!((c[0].width, c[0].height), (3000, 2251));
        assert!(c[0].thumb_url.ends_with("/thumb/"));
        assert_eq!(c[1].license.label(), "CC BY 2.0");
        assert_eq!(c[1].author_url.as_deref(), Some("https://www.flickr.com/photos/x"));
    }

    #[test]
    fn request_filters_licenses_and_skips_wikimedia() {
        let r = request("server rack", 1, false).unwrap();
        assert!(r.url.contains("license=cc0%2Cpdm%2Cby&"), "{}", r.url);
        assert!(r.url.contains("excluded_source=wikimedia"));
        assert!(request("x", 1, true).unwrap().url.contains("by-sa"));
    }

    #[test]
    fn daily_limit_pauses_for_a_day() {
        let mut r = mock::status(429, "").unwrap();
        r.rate_headers = vec![(SUSTAINED_LEFT.into(), "0".into())];
        assert!(matches!(check(&r), Err(StockError::RateLimited { daily: true, retry_after, .. }) if retry_after.as_secs() == 86_400));
        let burst = mock::status_retry_after(429, 42).unwrap();
        assert!(matches!(check(&burst), Err(StockError::RateLimited { daily: false, retry_after, .. }) if retry_after.as_secs() == 42));
    }
}
