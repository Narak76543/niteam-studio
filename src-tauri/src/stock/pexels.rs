//! Pexels photo and video search, optional (`docs/11-real-world-visuals.md` §2; new keys were
//! paused when checked). Checked 2026-10-17 against pexels.com/api/documentation:
//! - `GET https://api.pexels.com/v1/search?query=&orientation=landscape&per_page=(≤80)&page=`,
//!   header `Authorization: <key>` → `photos[]` {`id`, `width`, `height`, `url` (page),
//!   `photographer`, `photographer_url`, `alt`, `src.{original, large2x, large, medium, …}`}.
//! - `GET https://api.pexels.com/v1/videos/search?…` → `videos[]` {`id`, `width`, `height`,
//!   `url`, `image`, `duration` (s), `user.{name, url}`, `video_files[]` {`quality`,
//!   `file_type`, `width`, `height`, `link`}}.
//! - 200 / hour, 20 000 / month; `X-Ratelimit-*` only on 2xx, so a 429 waits an hour.

use std::time::Duration;

use serde::Deserialize;

use super::license::{License, LicenseCode};
use super::{Candidate, Media, Source, StockError, bad_json, check_status, title_from_slug};
use crate::net::transport::{Auth, HttpRequest, HttpResponse, Method};
use crate::storage::secrets::StockKey;

const API: &str = "https://api.pexels.com/v1/";
const PER_PAGE: u32 = 20;
const MAX_VIDEO_WIDTH: u32 = 1920;

pub fn request(query: &str, media: Media, page: u32, key: &StockKey) -> Result<HttpRequest, StockError> {
    let path = match media {
        Media::Photo => "search",
        Media::Video => "videos/search",
    };
    let url = reqwest::Url::parse_with_params(
        &format!("{API}{path}"),
        &[("query", query.trim()), ("orientation", "landscape"), ("per_page", &PER_PAGE.to_string()), ("page", &page.max(1).to_string())],
    )
    .map_err(|e| StockError::BadResponse { site: Source::Pexels, msg: e.to_string() })?;
    Ok(HttpRequest { method: Method::Get, url: url.into(), auth: Auth::PlainAuthorization(key.clone()), body: None, user_agent: None })
}

#[derive(Deserialize)]
struct Photos {
    #[serde(default)]
    photos: Vec<Photo>,
}

#[derive(Deserialize)]
struct Photo {
    id: u64,
    width: u32,
    height: u32,
    url: String,
    #[serde(default)]
    photographer: String,
    #[serde(default)]
    photographer_url: Option<String>,
    #[serde(default)]
    alt: Option<String>,
    src: Src,
}

#[derive(Deserialize)]
struct Src {
    original: String,
    medium: String,
}

#[derive(Deserialize)]
struct Videos {
    #[serde(default)]
    videos: Vec<Video>,
}

#[derive(Deserialize)]
struct Video {
    id: u64,
    url: String,
    #[serde(default)]
    image: String,
    #[serde(default)]
    duration: u64,
    user: User,
    #[serde(default)]
    video_files: Vec<VideoFile>,
}

#[derive(Deserialize)]
struct User {
    #[serde(default)]
    name: String,
    #[serde(default)]
    url: Option<String>,
}

#[derive(Deserialize)]
struct VideoFile {
    #[serde(default)]
    file_type: String,
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
    link: String,
}

pub fn parse(body: &[u8], media: Media) -> Result<Vec<Candidate>, StockError> {
    let license = License::new(LicenseCode::Pexels);
    let base = |id: u64, page_url: String, title: String, author: String, author_url: Option<String>| Candidate {
        source: Source::Pexels,
        source_id: id.to_string(),
        media,
        title,
        tags: vec![],
        author,
        author_url,
        license: license.clone(),
        page_url,
        file_url: String::new(),
        thumb_url: String::new(),
        width: 0,
        height: 0,
        duration_ms: None,
        restrictions: vec![],
        ai_generated: false,
        score: 0.0,
        low_res: false,
    };
    match media {
        Media::Photo => {
            let reply: Photos = serde_json::from_slice(body).map_err(|e| bad_json(Source::Pexels, e))?;
            Ok(reply
                .photos
                .into_iter()
                .map(|p| {
                    let title = p.alt.filter(|a| !a.trim().is_empty()).unwrap_or_else(|| title_from_slug(&p.url));
                    Candidate {
                        file_url: p.src.original,
                        thumb_url: p.src.medium,
                        width: p.width,
                        height: p.height,
                        ..base(p.id, p.url, title, p.photographer, p.photographer_url)
                    }
                })
                .collect())
        }
        Media::Video => {
            let reply: Videos = serde_json::from_slice(body).map_err(|e| bad_json(Source::Pexels, e))?;
            Ok(reply
                .videos
                .into_iter()
                .filter_map(|v| {
                    let file = v
                        .video_files
                        .iter()
                        .filter(|f| f.file_type == "video/mp4" && f.width.is_some_and(|w| w > 0 && w <= MAX_VIDEO_WIDTH))
                        .max_by_key(|f| f.width)?;
                    let title = title_from_slug(&v.url);
                    Some(Candidate {
                        file_url: file.link.clone(),
                        thumb_url: v.image.clone(),
                        width: file.width.unwrap_or(0),
                        height: file.height.unwrap_or(0),
                        duration_ms: Some(v.duration * 1000),
                        ..base(v.id, v.url.clone(), title, v.user.name.clone(), v.user.url.clone())
                    })
                })
                .collect())
        }
    }
}

/// Status → error; 429 carries no rate headers, so wait an hour.
pub fn check(resp: &HttpResponse) -> Result<(), StockError> {
    if resp.status == 429 {
        return Err(StockError::RateLimited { site: Source::Pexels, retry_after: Duration::from_secs(3600), daily: false });
    }
    check_status(Source::Pexels, resp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::transport::mock;

    /// Shapes from the API docs examples (2026-10-17).
    const PHOTOS: &str = r##"{"total_results":10000,"page":1,"per_page":1,"photos":[{"id":3573351,"width":3066,"height":3968,
      "url":"https://www.pexels.com/photo/trees-during-day-3573351/","photographer":"Lukas Rodriguez","photographer_url":"https://www.pexels.com/@lukas-rodriguez-1845331",
      "photographer_id":1845331,"avg_color":"#374824","src":{"original":"https://images.pexels.com/photos/3573351/pexels-photo-3573351.png",
      "large2x":"https://images.pexels.com/photos/3573351/pexels-photo-3573351.png?auto=compress&cs=tinysrgb&dpr=2&h=650&w=940",
      "large":"https://images.pexels.com/photos/3573351/pexels-photo-3573351.png?auto=compress&cs=tinysrgb&h=650&w=940",
      "medium":"https://images.pexels.com/photos/3573351/pexels-photo-3573351.png?auto=compress&cs=tinysrgb&h=350",
      "small":"x","portrait":"x","landscape":"x","tiny":"x"},"liked":false,"alt":"Brown Rocks During Golden Hour"}],"next_page":"x"}"##;
    const VIDEOS: &str = r#"{"page":1,"per_page":1,"total_results":20475,"url":"https://www.pexels.com/videos/","videos":[{"id":1448735,"width":4096,"height":2160,
      "url":"https://www.pexels.com/video/video-of-forest-1448735/","image":"https://images.pexels.com/videos/1448735/free-video-1448735.jpg","duration":32,
      "user":{"id":574687,"name":"Ruvim Miksanskiy","url":"https://www.pexels.com/@digitech"},
      "video_files":[{"id":58649,"quality":"sd","file_type":"video/mp4","width":640,"height":338,"link":"https://player.vimeo.com/sd.mp4"},
        {"id":58650,"quality":"hd","file_type":"video/mp4","width":2048,"height":1080,"link":"https://player.vimeo.com/2k.mp4"},
        {"id":58651,"quality":"hd","file_type":"video/mp4","width":1920,"height":1014,"link":"https://player.vimeo.com/fhd.mp4"},
        {"id":58652,"quality":"hls","file_type":"hls","width":null,"height":null,"link":"https://player.vimeo.com/x.m3u8"}],
      "video_pictures":[{"id":133236,"picture":"https://static-videos.pexels.com/videos/1448735/pictures/preview-0.jpg","nr":0}]}]}"#;

    #[test]
    fn photos_and_videos() {
        let p = parse(PHOTOS.as_bytes(), Media::Photo).unwrap();
        assert_eq!(p[0].title, "Brown Rocks During Golden Hour");
        assert_eq!((p[0].width, p[0].height), (3066, 3968));
        assert!(p[0].file_url.ends_with("pexels-photo-3573351.png"));
        let v = parse(VIDEOS.as_bytes(), Media::Video).unwrap();
        assert_eq!(v[0].file_url, "https://player.vimeo.com/fhd.mp4", "largest mp4 up to 1920 wide");
        assert_eq!((v[0].title.as_str(), v[0].duration_ms), ("video of forest", Some(32_000)));
        assert_eq!(v[0].author, "Ruvim Miksanskiy");
    }

    #[test]
    fn request_and_limits() {
        let key = StockKey::parse("abcdefgh12345678abcdefgh").unwrap();
        let r = request("data center", Media::Video, 1, &key).unwrap();
        assert!(r.url.starts_with("https://api.pexels.com/v1/videos/search?query=data+center"));
        assert!(matches!(check(&mock::status(429, "").unwrap()), Err(StockError::RateLimited { .. })));
        assert_eq!(check(&mock::status(401, "").unwrap()), Err(StockError::BadKey(Source::Pexels)));
    }
}
