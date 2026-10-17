//! Downloaded stock files of a project and their license records
//! (`projects/<id>/assets/assets.json`, `docs/11-real-world-visuals.md` §3). A file is
//! downloaded once and reused by every scene and every re-render.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

use super::license::{License, credit_line};
use super::{Candidate, Media, Source, StockError, user_agent};
use crate::storage::{StorageError, atomic};

const REGISTRY: &str = "assets.json";
const MAX_PHOTO_BYTES: u64 = 40 * 1024 * 1024;
const MAX_VIDEO_BYTES: u64 = 200 * 1024 * 1024;
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AssetRecord {
    /// `<source>_<source id>`.
    pub id: String,
    pub source: Source,
    pub media: Media,
    /// Relative to the project folder: `assets/<id>.jpg|png|mp4`.
    pub file: String,
    pub title: String,
    pub author: String,
    #[serde(default)]
    pub author_url: Option<String>,
    pub license: License,
    /// Landing page on the source site.
    pub original_url: String,
    /// What was downloaded.
    pub file_url: String,
    /// RFC 3339.
    pub downloaded_at: String,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    /// Ready credit line for the video description.
    pub attribution: String,
    #[serde(default)]
    pub restrictions: Vec<String>,
    pub sha256: String,
    pub bytes: u64,
    /// Videos: preview picture `assets/<id>.poster.jpg` (thumbnails, picker), if it downloaded.
    #[serde(default)]
    pub poster: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AssetRegistry {
    #[serde(default)]
    pub assets: Vec<AssetRecord>,
}

impl AssetRegistry {
    pub fn load(dir: &Path) -> Result<Self, StorageError> {
        Ok(atomic::read_json(&dir.join(REGISTRY))?.unwrap_or_default())
    }

    pub fn save(&self, dir: &Path) -> Result<(), StorageError> {
        atomic::write_json(&dir.join(REGISTRY), self)
    }

    pub fn get(&self, id: &str) -> Option<&AssetRecord> {
        self.assets.iter().find(|a| a.id == id)
    }

    pub fn upsert(&mut self, record: AssetRecord) {
        match self.assets.iter_mut().find(|a| a.id == record.id) {
            Some(a) => *a = record,
            None => self.assets.push(record),
        }
    }
}

pub type DownloadFuture<'a> = Pin<Box<dyn Future<Output = Result<u64, StockError>> + Send + 'a>>;

/// Streams `url` into `dest` (at most `max_bytes`); returns the size. A trait so the asset
/// logic can be tested without the network.
pub trait Downloader: Send + Sync {
    fn fetch<'a>(&'a self, url: &'a str, user_agent: Option<String>, max_bytes: u64, dest: &'a Path, cancel: &'a CancellationToken) -> DownloadFuture<'a>;
}

pub struct ReqwestDownloader {
    http: reqwest::Client,
}

impl ReqwestDownloader {
    pub fn new() -> Result<Self, StockError> {
        let http = reqwest::Client::builder()
            .timeout(DOWNLOAD_TIMEOUT)
            .connect_timeout(Duration::from_secs(20))
            .user_agent(concat!("NiteanStudio/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| StockError::Download(e.to_string()))?;
        Ok(Self { http })
    }
}

impl Downloader for ReqwestDownloader {
    fn fetch<'a>(&'a self, url: &'a str, user_agent: Option<String>, max_bytes: u64, dest: &'a Path, cancel: &'a CancellationToken) -> DownloadFuture<'a> {
        Box::pin(async move {
            let mut req = self.http.get(url);
            if let Some(ua) = user_agent {
                req = req.header(reqwest::header::USER_AGENT, ua);
            }
            let mut resp = req.send().await.map_err(|e| StockError::Download(e.without_url().to_string()))?;
            if !resp.status().is_success() {
                return Err(StockError::Download(format!("HTTP {}", resp.status().as_u16())));
            }
            let mut file = tokio::fs::File::create(dest).await.map_err(|e| StockError::Download(e.to_string()))?;
            let mut total = 0u64;
            loop {
                let chunk = tokio::select! {
                    () = cancel.cancelled() => return Err(StockError::Cancelled),
                    c = resp.chunk() => c.map_err(|e| StockError::Download(e.without_url().to_string()))?,
                };
                let Some(chunk) = chunk else { break };
                total += chunk.len() as u64;
                if total > max_bytes {
                    return Err(StockError::Download(format!("the file is larger than {} MB", max_bytes / 1024 / 1024)));
                }
                file.write_all(&chunk).await.map_err(|e| StockError::Download(e.to_string()))?;
            }
            file.flush().await.map_err(|e| StockError::Download(e.to_string()))?;
            Ok(total)
        })
    }
}

/// File type from the first bytes (servers' Content-Type is not trusted).
fn sniff(head: &[u8]) -> Option<&'static str> {
    if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if head.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("png")
    } else if head.get(4..8) == Some(b"ftyp") {
        Some("mp4")
    } else {
        None
    }
}

fn io(e: std::io::Error) -> StockError {
    StockError::Download(e.to_string())
}

/// The candidate's file in `assets_dir` with its record: reused when already downloaded,
/// else downloaded, checked (type, size) and recorded.
pub async fn ensure(assets_dir: &Path, c: &Candidate, dl: &dyn Downloader, cancel: &CancellationToken) -> Result<AssetRecord, StockError> {
    let id = c.asset_id();
    let project_dir = assets_dir.parent().unwrap_or(assets_dir);
    let mut registry = AssetRegistry::load(assets_dir).map_err(|e| StockError::Download(e.to_string()))?;
    if let Some(r) = registry.get(&id)
        && project_dir.join(&r.file).is_file()
    {
        return Ok(r.clone());
    }
    std::fs::create_dir_all(assets_dir).map_err(io)?;
    let part = assets_dir.join(format!("{id}.part"));
    let max = if c.media == Media::Video { MAX_VIDEO_BYTES } else { MAX_PHOTO_BYTES };
    let ua = matches!(c.source, Source::Wikimedia | Source::Openverse).then(user_agent);
    let fetched = dl.fetch(&c.file_url, ua, max, &part, cancel).await;
    let result = fetched.and_then(|bytes| finish(assets_dir, &part, c, &id, bytes));
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    let mut record = result?;
    if c.media == Media::Video && !c.thumb_url.is_empty() {
        let poster = assets_dir.join(format!("{id}.poster.jpg"));
        let ok = dl.fetch(&c.thumb_url, None, MAX_PHOTO_BYTES, &poster, cancel).await.is_ok()
            && std::fs::read(&poster).is_ok_and(|b| sniff(&b[..b.len().min(16)]).is_some_and(|e| e != "mp4"));
        if ok {
            record.poster = Some(format!("assets/{id}.poster.jpg"));
        } else {
            let _ = std::fs::remove_file(&poster);
        }
    }
    registry.upsert(record.clone());
    registry.save(assets_dir).map_err(|e| StockError::Download(e.to_string()))?;
    Ok(record)
}

fn finish(assets_dir: &Path, part: &Path, c: &Candidate, id: &str, bytes: u64) -> Result<AssetRecord, StockError> {
    let data = std::fs::read(part).map_err(io)?;
    let ext = sniff(&data[..data.len().min(16)]).ok_or_else(|| StockError::Download("the downloaded file is not a JPEG, PNG or MP4".into()))?;
    let ok = match c.media {
        Media::Photo => ext != "mp4",
        Media::Video => ext == "mp4",
    };
    if !ok {
        return Err(StockError::Download(format!("expected a {:?} but got a .{ext} file", c.media)));
    }
    let file: PathBuf = assets_dir.join(format!("{id}.{ext}"));
    std::fs::rename(part, &file).map_err(io)?;
    Ok(AssetRecord {
        id: id.to_owned(),
        source: c.source,
        media: c.media,
        file: format!("assets/{id}.{ext}"),
        title: c.title.clone(),
        author: c.author.clone(),
        author_url: c.author_url.clone(),
        license: c.license.clone(),
        original_url: c.page_url.clone(),
        file_url: c.file_url.clone(),
        downloaded_at: chrono::Utc::now().to_rfc3339(),
        width: c.width,
        height: c.height,
        duration_ms: c.duration_ms,
        attribution: credit_line(c),
        restrictions: c.restrictions.clone(),
        sha256: hex::encode(Sha256::digest(&data)),
        bytes,
        poster: None,
    })
}

/// The user's own screenshot or photo (consent given in the UI): copied into the assets
/// folder as `user_<sha8>.jpg|png`, licence `own` (left out of the credits).
pub fn import_own(assets_dir: &Path, source: &Path) -> Result<AssetRecord, StockError> {
    let meta = std::fs::metadata(source).map_err(io)?;
    if meta.len() > MAX_PHOTO_BYTES {
        return Err(StockError::Download(format!("the file is larger than {} MB", MAX_PHOTO_BYTES / 1024 / 1024)));
    }
    let data = std::fs::read(source).map_err(io)?;
    let ext = match sniff(&data[..data.len().min(16)]) {
        Some(e @ ("jpg" | "png")) => e,
        _ => return Err(StockError::Download("choose a JPEG or PNG image".into())),
    };
    let (width, height) = crate::images::postprocess::dimensions(&data).ok_or_else(|| StockError::Download("could not read the image size".into()))?;
    let sha = hex::encode(Sha256::digest(&data));
    let id = format!("user_{}", &sha[..12]);
    std::fs::create_dir_all(assets_dir).map_err(io)?;
    std::fs::write(assets_dir.join(format!("{id}.{ext}")), &data).map_err(io)?;
    let title = source.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let record = AssetRecord {
        id: id.clone(),
        source: Source::User,
        media: Media::Photo,
        file: format!("assets/{id}.{ext}"),
        title: title.clone(),
        author: String::new(),
        author_url: None,
        license: License::new(super::license::LicenseCode::Own),
        original_url: String::new(),
        file_url: String::new(),
        downloaded_at: chrono::Utc::now().to_rfc3339(),
        width,
        height,
        duration_ms: None,
        attribution: format!("{title} — own material"),
        restrictions: vec![],
        sha256: sha,
        bytes: meta.len(),
        poster: None,
    };
    let mut registry = AssetRegistry::load(assets_dir).map_err(|e| StockError::Download(e.to_string()))?;
    registry.upsert(record.clone());
    registry.save(assets_dir).map_err(|e| StockError::Download(e.to_string()))?;
    Ok(record)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::stock::license::License;
    use crate::stock::tests::candidate;

    struct Fake {
        body: Vec<u8>,
        calls: Mutex<Vec<(String, Option<String>)>>,
    }

    impl Downloader for Fake {
        fn fetch<'a>(&'a self, url: &'a str, ua: Option<String>, _max: u64, dest: &'a Path, _c: &'a CancellationToken) -> DownloadFuture<'a> {
            self.calls.lock().unwrap().push((url.to_owned(), ua));
            Box::pin(async move {
                std::fs::write(dest, &self.body).unwrap();
                Ok(self.body.len() as u64)
            })
        }
    }

    #[tokio::test]
    async fn downloads_once_records_license_and_checks_the_type() {
        let dir = tempfile::tempdir().unwrap();
        let assets = dir.path().join("assets");
        let jpeg = Fake { body: vec![0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3], calls: Mutex::new(vec![]) };
        let c = candidate(Source::Wikimedia, 4000, 3000, License::cc0());
        let r = ensure(&assets, &c, &jpeg, &CancellationToken::new()).await.unwrap();
        assert_eq!(r.file, "assets/wikimedia_1.jpg");
        assert!(dir.path().join(&r.file).is_file());
        assert_eq!((r.license.code, r.width, r.bytes), (License::cc0().code, 4000, 7));
        assert!(r.attribution.contains("via Wikimedia Commons"));
        assert!(jpeg.calls.lock().unwrap()[0].1.as_deref().is_some_and(|ua| ua.contains("niteam-studio")), "Commons gets the contact User-Agent");
        ensure(&assets, &c, &jpeg, &CancellationToken::new()).await.unwrap();
        assert_eq!(jpeg.calls.lock().unwrap().len(), 1, "second use is served from disk");
        assert_eq!(AssetRegistry::load(&assets).unwrap().assets.len(), 1);

        let html = Fake { body: b"<html>not found</html>".to_vec(), calls: Mutex::new(vec![]) };
        let mut other = candidate(Source::Pixabay, 1280, 720, License::cc0());
        other.source_id = "2".into();
        assert!(ensure(&assets, &other, &html, &CancellationToken::new()).await.is_err());
        assert!(!assets.join("pixabay_2.part").exists(), "partial file removed");
        assert!(html.calls.lock().unwrap()[0].1.is_none(), "no contact details sent to Pixabay");
    }

    #[tokio::test]
    async fn videos_keep_their_poster() {
        struct ByUrl;
        impl Downloader for ByUrl {
            fn fetch<'a>(&'a self, url: &'a str, _ua: Option<String>, _m: u64, dest: &'a Path, _c: &'a CancellationToken) -> DownloadFuture<'a> {
                let body: Vec<u8> = if url.ends_with(".mp4") { vec![0, 0, 0, 24, b'f', b't', b'y', b'p', b'm', b'p', b'4', b'2'] } else { vec![0xFF, 0xD8, 0xFF, 0xE0] };
                Box::pin(async move {
                    std::fs::write(dest, &body).unwrap();
                    Ok(body.len() as u64)
                })
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let mut c = candidate(Source::Pixabay, 1920, 1080, License::new(crate::stock::license::LicenseCode::Pixabay));
        c.media = Media::Video;
        c.file_url = "https://cdn.pixabay.com/video/m.mp4".into();
        let r = ensure(&dir.path().join("assets"), &c, &ByUrl, &CancellationToken::new()).await.unwrap();
        assert_eq!(r.file, "assets/pixabay_1.mp4");
        assert_eq!(r.poster.as_deref(), Some("assets/pixabay_1.poster.jpg"));
    }

    #[test]
    fn imports_own_images_only() {
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("my screen.png");
        image::RgbImage::from_pixel(40, 30, image::Rgb([1, 2, 3])).save(&png).unwrap();
        let r = import_own(&dir.path().join("assets"), &png).unwrap();
        assert_eq!((r.source, r.license.code, r.width, r.height), (Source::User, crate::stock::license::LicenseCode::Own, 40, 30));
        assert!(r.file.starts_with("assets/user_") && r.file.ends_with(".png"));
        let txt = dir.path().join("x.txt");
        std::fs::write(&txt, "hello").unwrap();
        assert!(import_own(&dir.path().join("assets"), &txt).is_err());
    }
}
