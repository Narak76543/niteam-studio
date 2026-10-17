//! Pixabay photo and video search (`docs/11-real-world-visuals.md` §2). Checked 2026-10-17
//! against pixabay.com/api/docs:
//! - `GET https://pixabay.com/api/?key=&q=(≤100 chars)&image_type=photo&orientation=horizontal
//!   &safesearch=true&per_page=(3–200)&page=` → `hits[]` {`id`, `pageURL`, `tags` (comma list),
//!   `webformatURL` (640 px), `largeImageURL` (≤ 1280 px), `imageWidth`, `imageHeight`,
//!   `user`, `user_id`; `fullHDURL` / `imageURL` only with full API access}.
//! - `GET https://pixabay.com/api/videos/?…` → `hits[]` {`id`, `pageURL`, `tags`, `duration`
//!   (s), `videos.{large,medium,small,tiny}.{url, width, height, size, thumbnail}`, `user`…};
//!   `large` may be empty.
//! - 100 requests / 60 s per key; `X-RateLimit-Reset` = seconds until the window resets; 429
//!   when over. Results must be cached 24 h; files must be downloaded (no hotlinking).

use std::time::Duration;

use serde::Deserialize;

use super::license::{License, LicenseCode};
use super::{Candidate, Media, Source, StockError, bad_json, check_status, looks_ai_generated};
use crate::net::transport::{Auth, HttpRequest, HttpResponse, Method};
use crate::storage::secrets::StockKey;

const API: &str = "https://pixabay.com/api/";
const PER_PAGE: u32 = 20;
const MAX_QUERY_CHARS: usize = 100;
/// Without full API access `largeImageURL` is at most this wide.
const LARGE_MAX: u32 = 1280;
/// Video renditions above Full HD are not needed (and slow to decode on a weak laptop).
const MAX_VIDEO_WIDTH: u32 = 1920;

pub fn request(query: &str, media: Media, page: u32, key: &StockKey) -> Result<HttpRequest, StockError> {
    let q: String = query.trim().chars().take(MAX_QUERY_CHARS).collect();
    let (base, extra): (String, &[(&str, &str)]) = match media {
        Media::Photo => (API.to_owned(), &[("image_type", "photo"), ("orientation", "horizontal")]),
        Media::Video => (format!("{API}videos/"), &[("video_type", "film")]),
    };
    let per_page = PER_PAGE.to_string();
    let page = page.max(1).to_string();
    let mut params: Vec<(&str, &str)> = vec![("q", &q), ("safesearch", "true"), ("per_page", &per_page), ("page", &page)];
    params.extend_from_slice(extra);
    let url = reqwest::Url::parse_with_params(&base, &params)
        .map_err(|e| StockError::BadResponse { site: Source::Pixabay, msg: e.to_string() })?;
    Ok(HttpRequest { method: Method::Get, url: url.into(), auth: Auth::QueryKey("key", key.clone()), body: None, user_agent: None })
}

#[derive(Deserialize)]
struct Reply<T> {
    #[serde(default = "Vec::new")]
    hits: Vec<T>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Photo {
    id: u64,
    #[serde(rename = "pageURL")]
    page_url: String,
    #[serde(default)]
    tags: String,
    #[serde(rename = "webformatURL")]
    webformat_url: String,
    #[serde(rename = "largeImageURL")]
    large_image_url: String,
    #[serde(default, rename = "fullHDURL")]
    full_hd_url: Option<String>,
    #[serde(default, rename = "imageURL")]
    image_url: Option<String>,
    image_width: u32,
    image_height: u32,
    #[serde(default)]
    user: String,
    #[serde(default, rename = "user_id")]
    user_id: u64,
}

#[derive(Deserialize)]
struct Video {
    id: u64,
    #[serde(rename = "pageURL")]
    page_url: String,
    #[serde(default)]
    tags: String,
    #[serde(default)]
    duration: u64,
    videos: std::collections::HashMap<String, Rendition>,
    #[serde(default)]
    user: String,
    #[serde(default)]
    user_id: u64,
}

#[derive(Deserialize)]
struct Rendition {
    #[serde(default)]
    url: String,
    #[serde(default)]
    width: u32,
    #[serde(default)]
    height: u32,
    #[serde(default)]
    thumbnail: String,
}

fn tags(s: &str) -> Vec<String> {
    s.split(',').map(str::trim).filter(|t| !t.is_empty()).map(str::to_owned).collect()
}

fn user_url(user: &str, id: u64) -> Option<String> {
    (!user.is_empty()).then(|| format!("https://pixabay.com/users/{user}-{id}/"))
}

/// The biggest file we may download, with its real size: original (full access) → Full HD
/// (full access) → `largeImageURL` (≤ 1280 px wide).
fn best_photo(p: &Photo) -> (String, u32, u32) {
    if let Some(u) = p.image_url.as_deref().filter(|u| !u.is_empty()) {
        return (u.to_owned(), p.image_width, p.image_height);
    }
    let scaled = |w: u32| (w, (u64::from(p.image_height) * u64::from(w) / u64::from(p.image_width.max(1))) as u32);
    if let Some(u) = p.full_hd_url.as_deref().filter(|u| !u.is_empty()) {
        let (w, h) = scaled(p.image_width.min(1920));
        return (u.to_owned(), w, h);
    }
    let (w, h) = scaled(p.image_width.min(LARGE_MAX));
    (p.large_image_url.clone(), w, h)
}

pub fn parse(body: &[u8], media: Media) -> Result<Vec<Candidate>, StockError> {
    let license = License::new(LicenseCode::Pixabay);
    match media {
        Media::Photo => {
            let reply: Reply<Photo> = serde_json::from_slice(body).map_err(|e| bad_json(Source::Pixabay, e))?;
            Ok(reply
                .hits
                .into_iter()
                .map(|p| {
                    let (file_url, width, height) = best_photo(&p);
                    let tags = tags(&p.tags);
                    Candidate {
                        source: Source::Pixabay,
                        source_id: p.id.to_string(),
                        media,
                        title: tags.join(", "),
                        ai_generated: looks_ai_generated(&tags.iter().map(String::as_str).collect::<Vec<_>>()),
                        tags,
                        author_url: user_url(&p.user, p.user_id),
                        author: p.user,
                        license: license.clone(),
                        page_url: p.page_url,
                        file_url,
                        thumb_url: p.webformat_url,
                        width,
                        height,
                        duration_ms: None,
                        restrictions: vec![],
                        score: 0.0,
                        low_res: false,
                    }
                })
                .collect())
        }
        Media::Video => {
            let reply: Reply<Video> = serde_json::from_slice(body).map_err(|e| bad_json(Source::Pixabay, e))?;
            Ok(reply
                .hits
                .into_iter()
                .filter_map(|v| {
                    let best = v
                        .videos
                        .values()
                        .filter(|r| !r.url.is_empty() && r.width > 0 && r.width <= MAX_VIDEO_WIDTH)
                        .max_by_key(|r| r.width)?;
                    let thumb = if best.thumbnail.is_empty() { v.videos.get("tiny").map(|t| t.thumbnail.clone()).unwrap_or_default() } else { best.thumbnail.clone() };
                    let tags = tags(&v.tags);
                    Some(Candidate {
                        source: Source::Pixabay,
                        source_id: v.id.to_string(),
                        media,
                        title: tags.join(", "),
                        ai_generated: looks_ai_generated(&tags.iter().map(String::as_str).collect::<Vec<_>>()),
                        tags,
                        author_url: user_url(&v.user, v.user_id),
                        author: v.user.clone(),
                        license: license.clone(),
                        page_url: v.page_url.clone(),
                        file_url: best.url.clone(),
                        thumb_url: thumb,
                        width: best.width,
                        height: best.height,
                        duration_ms: Some(v.duration * 1000),
                        restrictions: vec![],
                        score: 0.0,
                        low_res: false,
                    })
                })
                .collect())
        }
    }
}

/// Status → error. 429: wait for the 60 s window (`X-RateLimit-Reset`, seconds).
pub fn check(resp: &HttpResponse) -> Result<(), StockError> {
    if resp.status == 429 {
        let secs = resp.rate_header("x-ratelimit-reset").and_then(|v| v.trim().parse::<u64>().ok()).unwrap_or(60);
        return Err(StockError::RateLimited { site: Source::Pixabay, retry_after: Duration::from_secs(secs.max(1)), daily: false });
    }
    // Pixabay answers a wrong key with HTTP 400 "[ERROR 400] Invalid or missing API key".
    if resp.status == 400 && String::from_utf8_lossy(&resp.body).to_lowercase().contains("api key") {
        return Err(StockError::BadKey(Source::Pixabay));
    }
    check_status(Source::Pixabay, resp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::transport::mock;

    /// Shapes from the API docs example (2026-10-17).
    const PHOTOS: &str = r#"{"total":4692,"totalHits":500,"hits":[{"id":195893,"pageURL":"https://pixabay.com/en/blossom-bloom-flower-195893/","type":"photo",
      "tags":"blossom, bloom, flower","previewURL":"https://cdn.pixabay.com/photo/2013/10/15/09/12/flower-195893_150.jpg","previewWidth":150,"previewHeight":84,
      "webformatURL":"https://pixabay.com/get/35bbf209e13e39d2_640.jpg","webformatWidth":640,"webformatHeight":360,"largeImageURL":"https://pixabay.com/get/ed6a99fd0a76647_1280.jpg",
      "imageWidth":4000,"imageHeight":2250,"imageSize":4731420,"views":7671,"downloads":6439,"likes":5,"comments":2,"user_id":48777,"user":"Josch13","userImageURL":"https://cdn.pixabay.com/user/x.jpg"}]}"#;
    const VIDEOS: &str = r#"{"total":42,"totalHits":42,"hits":[{"id":125,"pageURL":"https://pixabay.com/videos/id-125/","type":"film","tags":"flowers, yellow, blossom","duration":12,
      "videos":{"large":{"url":"https://cdn.pixabay.com/video/l.mp4","width":3840,"height":2160,"size":6615235,"thumbnail":"https://cdn.pixabay.com/video/l.jpg"},
        "medium":{"url":"https://cdn.pixabay.com/video/m.mp4","width":1920,"height":1080,"size":3562083,"thumbnail":"https://cdn.pixabay.com/video/m.jpg"},
        "small":{"url":"https://cdn.pixabay.com/video/s.mp4","width":1280,"height":720,"size":2569598,"thumbnail":"https://cdn.pixabay.com/video/s.jpg"},
        "tiny":{"url":"https://cdn.pixabay.com/video/t.mp4","width":960,"height":540,"size":1030736,"thumbnail":"https://cdn.pixabay.com/video/t.jpg"}},
      "views":4462,"downloads":1464,"likes":18,"comments":0,"user_id":1281706,"user":"Coverr-Free-Footage","userImageURL":""}]}"#;

    #[test]
    fn photos_use_the_largest_allowed_file_with_its_real_size() {
        let c = parse(PHOTOS.as_bytes(), Media::Photo).unwrap();
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].file_url, "https://pixabay.com/get/ed6a99fd0a76647_1280.jpg");
        assert_eq!((c[0].width, c[0].height), (1280, 720), "largeImageURL is ≤ 1280 px");
        assert_eq!(c[0].author_url.as_deref(), Some("https://pixabay.com/users/Josch13-48777/"));
        assert_eq!(c[0].tags, vec!["blossom", "bloom", "flower"]);
    }

    #[test]
    fn full_access_original_when_present() {
        let full = PHOTOS.replace("\"imageWidth\"", "\"imageURL\":\"https://pixabay.com/get/orig.jpg\",\"imageWidth\"");
        let c = parse(full.as_bytes(), Media::Photo).unwrap();
        assert_eq!((c[0].file_url.as_str(), c[0].width), ("https://pixabay.com/get/orig.jpg", 4000));
    }

    #[test]
    fn videos_pick_full_hd_at_most() {
        let c = parse(VIDEOS.as_bytes(), Media::Video).unwrap();
        assert_eq!(c[0].file_url, "https://cdn.pixabay.com/video/m.mp4");
        assert_eq!((c[0].width, c[0].duration_ms), (1920, Some(12_000)));
        assert_eq!(c[0].thumb_url, "https://cdn.pixabay.com/video/m.jpg");
    }

    #[test]
    fn key_never_in_the_url_and_limits_are_mapped() {
        let key = StockKey::parse("abcdefgh12345678-abcdefgh").unwrap();
        let r = request("submarine cable", Media::Video, 1, &key).unwrap();
        assert!(r.url.starts_with("https://pixabay.com/api/videos/?q=submarine+cable"));
        assert!(!r.url.contains("abcdefgh"));
        let mut limited = mock::status(429, "API rate limit exceeded").unwrap();
        limited.rate_headers = vec![("x-ratelimit-reset".into(), "17".into())];
        assert!(matches!(check(&limited), Err(StockError::RateLimited { retry_after, .. }) if retry_after.as_secs() == 17));
        assert_eq!(check(&mock::status(400, "[ERROR 400] Invalid or missing API key").unwrap()), Err(StockError::BadKey(Source::Pixabay)));
    }
}
