//! Wikimedia Commons file search (`docs/11-real-world-visuals.md` §2). Checked 2026-10-17:
//! `GET https://commons.wikimedia.org/w/api.php?action=query&generator=search&gsrnamespace=6
//! &prop=imageinfo&iiprop=url|size|mime|extmetadata&formatversion=2` → `query.pages[]` with
//! `index`, `title`, `imageinfo[0]` {`url`, `descriptionurl`, `thumburl`, `width`, `height`,
//! `size`, `mime`, `extmetadata.{LicenseShortName, LicenseUrl, Artist (HTML), ObjectName,
//! Restrictions, Categories}.value`}. A descriptive User-Agent with contact is required
//! (Wikimedia User-Agent policy, else HTTP 403). `maxlag=5`: busy servers answer with error
//! `maxlag` + `Retry-After`. No key, no fixed quota; requests are made one at a time.

use std::time::Duration;

use serde::Deserialize;

use super::license::from_commons;
use super::{Candidate, Media, Source, StockError, bad_json, check_status, looks_ai_generated, user_agent};
use crate::net::transport::{Auth, HttpRequest, HttpResponse, Method};

const API: &str = "https://commons.wikimedia.org/w/api.php";
const PER_PAGE: u32 = 20;
/// Bigger originals are skipped (slow download, huge decode on an 8 GB laptop).
const MAX_FILE_BYTES: u64 = 40 * 1024 * 1024;
const METADATA: &str = "LicenseShortName|LicenseUrl|Artist|ObjectName|Restrictions|Categories";

pub fn request(query: &str, page: u32) -> Result<HttpRequest, StockError> {
    let offset = (page.max(1) - 1) * PER_PAGE;
    let url = reqwest::Url::parse_with_params(
        API,
        &[
            ("action", "query"),
            ("format", "json"),
            ("formatversion", "2"),
            ("generator", "search"),
            ("gsrsearch", &format!("{} filetype:bitmap", query.trim())),
            ("gsrnamespace", "6"),
            ("gsrlimit", &PER_PAGE.to_string()),
            ("gsroffset", &offset.to_string()),
            ("prop", "imageinfo"),
            ("iiprop", "url|size|mime|extmetadata"),
            ("iiurlwidth", "320"),
            ("iiextmetadatafilter", METADATA),
            ("maxlag", "5"),
        ],
    )
    .map_err(|e| StockError::BadResponse { site: Source::Wikimedia, msg: e.to_string() })?;
    Ok(HttpRequest { method: Method::Get, url: url.into(), auth: Auth::None, body: None, user_agent: Some(user_agent()) })
}

#[derive(Deserialize)]
struct Reply {
    #[serde(default)]
    query: Option<Query>,
    #[serde(default)]
    error: Option<ApiError>,
}

#[derive(Deserialize)]
struct ApiError {
    #[serde(default)]
    code: String,
    #[serde(default)]
    info: String,
}

#[derive(Deserialize)]
struct Query {
    #[serde(default)]
    pages: Vec<Page>,
}

#[derive(Deserialize)]
struct Page {
    pageid: u64,
    title: String,
    #[serde(default)]
    index: u32,
    #[serde(default)]
    imageinfo: Vec<Info>,
}

#[derive(Deserialize)]
struct Info {
    url: String,
    descriptionurl: String,
    #[serde(default)]
    thumburl: Option<String>,
    width: u32,
    height: u32,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    mime: String,
    #[serde(default)]
    extmetadata: std::collections::HashMap<String, Meta>,
}

#[derive(Deserialize)]
struct Meta {
    #[serde(default)]
    value: serde_json::Value,
}

/// Plain text of an extmetadata value (strings may hold HTML).
fn meta(info: &Info, key: &str) -> String {
    match info.extmetadata.get(key).map(|m| &m.value) {
        Some(serde_json::Value::String(s)) => strip_html(s),
        Some(v) if !v.is_null() => v.to_string(),
        _ => String::new(),
    }
}

fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&").replace("&nbsp;", " ").replace("&quot;", "\"").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// "File:Racks Amravati Data Center.jpg" → "Racks Amravati Data Center".
fn title_from_file(title: &str) -> String {
    let name = title.strip_prefix("File:").unwrap_or(title);
    name.rsplit_once('.').map_or(name, |(n, _)| n).to_owned()
}

/// Parses search results; files with an unusable license or format are left out.
pub fn parse(body: &[u8]) -> Result<Vec<Candidate>, StockError> {
    let reply: Reply = serde_json::from_slice(body).map_err(|e| bad_json(Source::Wikimedia, e))?;
    if let Some(e) = reply.error {
        return Err(StockError::BadResponse { site: Source::Wikimedia, msg: format!("{}: {}", e.code, e.info) });
    }
    let mut pages = reply.query.map(|q| q.pages).unwrap_or_default();
    pages.sort_by_key(|p| p.index);
    let mut out = Vec::new();
    for p in pages {
        let Some(info) = p.imageinfo.first() else { continue };
        if !matches!(info.mime.as_str(), "image/jpeg" | "image/png") || info.size > MAX_FILE_BYTES {
            continue;
        }
        let url = meta(info, "LicenseUrl");
        let Some(license) = from_commons(&meta(info, "LicenseShortName"), (!url.is_empty()).then_some(url.as_str())) else {
            continue;
        };
        let object = meta(info, "ObjectName");
        let title = if object.is_empty() { title_from_file(&p.title) } else { object };
        let categories = meta(info, "Categories");
        let restrictions: Vec<String> =
            meta(info, "Restrictions").split('|').map(str::trim).filter(|r| !r.is_empty()).map(str::to_owned).collect();
        out.push(Candidate {
            source: Source::Wikimedia,
            source_id: p.pageid.to_string(),
            media: Media::Photo,
            ai_generated: looks_ai_generated(&[&p.title, &categories]),
            title,
            tags: categories.split('|').map(str::trim).filter(|c| !c.is_empty()).map(str::to_owned).collect(),
            author: meta(info, "Artist"),
            author_url: None,
            license,
            page_url: info.descriptionurl.clone(),
            file_url: info.url.clone(),
            thumb_url: info.thumburl.clone().unwrap_or_else(|| info.url.clone()),
            width: info.width,
            height: info.height,
            duration_ms: None,
            restrictions,
            score: 0.0,
            low_res: false,
        });
    }
    Ok(out)
}

/// Status → error; `maxlag` and 429 become a short pause.
pub fn check(resp: &HttpResponse) -> Result<(), StockError> {
    let maxlag = resp.status == 200 && String::from_utf8_lossy(&resp.body).contains("\"code\":\"maxlag\"");
    if resp.status == 429 || maxlag {
        return Err(StockError::RateLimited {
            site: Source::Wikimedia,
            retry_after: resp.retry_after.unwrap_or(Duration::from_secs(30)),
            daily: false,
        });
    }
    check_status(Source::Wikimedia, resp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::transport::mock;
    use crate::stock::license::LicenseCode;

    /// Trimmed live reply (2026-10-17) plus a trademarked, an NC and an AI-generated file.
    const SAMPLE: &str = r#"{"batchcomplete":true,"query":{"pages":[
      {"pageid":113324689,"ns":6,"title":"File:Lebanon submarine telecommunications cables.png","index":2,"imageinfo":[{"size":267633,"width":1244,"height":912,
        "thumburl":"https://thumb.wikimedia.org/a/330px-L.png","url":"https://upload.wikimedia.org/d/d1/L.png","descriptionurl":"https://commons.wikimedia.org/wiki/File:Lebanon_submarine_telecommunications_cables.png","mime":"image/png",
        "extmetadata":{"ObjectName":{"value":"Lebanon submarine telecommunications cables"},"Categories":{"value":"Maps of submarine communication cables|Internet in Lebanon"},
        "Artist":{"value":"<a href=\"//x\">Submarine Cable Map</a>"},"LicenseShortName":{"value":"CC BY 3.0"},"LicenseUrl":{"value":"https://creativecommons.org/licenses/by/3.0"},"Restrictions":{"value":""}}}]},
      {"pageid":5,"title":"File:Coca-ColaHQ.jpg","index":1,"imageinfo":[{"size":1000,"width":4000,"height":3000,"url":"https://u/c.jpg","descriptionurl":"https://c/wiki/File:Coca-ColaHQ.jpg","mime":"image/jpeg",
        "extmetadata":{"LicenseShortName":{"value":"Public domain"},"Restrictions":{"value":"trademarked|personality"}}}]},
      {"pageid":6,"title":"File:Nc.jpg","index":3,"imageinfo":[{"size":1000,"width":4000,"height":3000,"url":"https://u/n.jpg","descriptionurl":"https://c/n","mime":"image/jpeg",
        "extmetadata":{"LicenseShortName":{"value":"CC BY-NC 2.0"}}}]},
      {"pageid":7,"title":"File:DALL·E 2025 data center.png","index":4,"imageinfo":[{"size":1000,"width":1024,"height":1024,"url":"https://u/d.png","descriptionurl":"https://c/d","mime":"image/png",
        "extmetadata":{"LicenseShortName":{"value":"CC0"}}}]},
      {"pageid":8,"title":"File:Scan.tif","index":5,"imageinfo":[{"size":1000,"width":4000,"height":3000,"url":"https://u/s.tif","descriptionurl":"https://c/s","mime":"image/tiff",
        "extmetadata":{"LicenseShortName":{"value":"CC0"}}}]}
    ]}}"#;

    #[test]
    fn parses_licenses_restrictions_and_order() {
        let c = parse(SAMPLE.as_bytes()).unwrap();
        assert_eq!(c.len(), 3, "NC and TIFF left out: {c:#?}");
        assert_eq!(c[0].title, "Coca-ColaHQ");
        assert_eq!(c[0].restrictions, vec!["trademarked", "personality"]);
        assert_eq!(c[0].license.code, LicenseCode::Pdm);
        let cable = &c[1];
        assert_eq!(cable.author, "Submarine Cable Map");
        assert_eq!(cable.license.label(), "CC BY 3.0");
        assert_eq!(cable.license.url.as_deref(), Some("https://creativecommons.org/licenses/by/3.0"));
        assert_eq!((cable.width, cable.height), (1244, 912));
        assert_eq!(cable.page_url, "https://commons.wikimedia.org/wiki/File:Lebanon_submarine_telecommunications_cables.png");
        assert!(c[2].ai_generated);
    }

    #[test]
    fn request_has_user_agent_and_no_key() {
        let r = request("submarine cable", 2).unwrap();
        assert!(r.url.contains("gsrsearch=submarine+cable+filetype%3Abitmap"), "{}", r.url);
        assert!(r.url.contains("gsroffset=20"));
        assert!(r.user_agent.as_deref().is_some_and(|ua| ua.contains("niteam-studio")));
        assert!(matches!(r.auth, Auth::None));
    }

    #[test]
    fn maxlag_and_429_pause() {
        let lag = mock::status(200, r#"{"error":{"code":"maxlag","info":"Waiting for a database server"}}"#).unwrap();
        assert!(matches!(check(&lag), Err(StockError::RateLimited { daily: false, .. })));
        assert!(matches!(check(&mock::status(429, "").unwrap()), Err(StockError::RateLimited { .. })));
        assert!(matches!(check(&mock::status(403, "").unwrap()), Err(StockError::Unavailable { .. })));
        assert!(check(&mock::status(200, "{}").unwrap()).is_ok());
    }
}
