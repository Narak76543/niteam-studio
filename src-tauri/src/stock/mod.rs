//! Stock photos and videos for the "Real-world Tech Explainer" preset
//! (`docs/11-real-world-visuals.md`, CLAUDE.md exception 5): Pexels, Pixabay, Openverse and
//! Wikimedia Commons only. Searches are cached for 24 h (`cache`), results are filtered by
//! license (`license`) and ranked locally (`rank`); a picked file is downloaded once per
//! project with its license record (`assets`).

pub mod assets;
pub mod cache;
pub mod commons;
pub mod license;
pub mod openverse;
pub mod pexels;
pub mod pixabay;
pub mod rank;
pub mod search;

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::net::transport::{HttpResponse, TransportError};
use license::License;

/// Who is calling, for APIs that ask for contact details (Wikimedia's User-Agent policy).
/// The one place to change the contact.
pub const CONTACT: &str = "https://github.com/Narak76543/niteam-studio; saratnarak85@gmail.com";

/// `NiteanStudio/<app version> (<contact>)`.
pub fn user_agent() -> String {
    format!("NiteanStudio/{} ({CONTACT})", env!("CARGO_PKG_VERSION"))
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Pexels,
    Pixabay,
    Openverse,
    Wikimedia,
    /// The user's own screenshot or photo.
    User,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pexels => "pexels",
            Self::Pixabay => "pixabay",
            Self::Openverse => "openverse",
            Self::Wikimedia => "wikimedia",
            Self::User => "user",
        }
    }

    pub fn display(self) -> &'static str {
        match self {
            Self::Pexels => "Pexels",
            Self::Pixabay => "Pixabay",
            Self::Openverse => "Openverse",
            Self::Wikimedia => "Wikimedia Commons",
            Self::User => "Own",
        }
    }

    /// Keyring user of the source's API key (`None` = no key needed).
    pub fn key_user(self) -> Option<&'static str> {
        match self {
            Self::Pexels => Some("pexels_api_key"),
            Self::Pixabay => Some("pixabay_api_key"),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Media {
    Photo,
    Video,
}

/// One search result, as shown in the picker and stored on the scene.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Candidate {
    pub source: Source,
    pub source_id: String,
    pub media: Media,
    pub title: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub author: String,
    #[serde(default)]
    pub author_url: Option<String>,
    pub license: License,
    /// Landing page on the source site (credits, disputes).
    pub page_url: String,
    /// The file that gets downloaded.
    pub file_url: String,
    /// Small preview for the picker (temporary display only).
    pub thumb_url: String,
    /// Pixel size of `file_url`.
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    /// Commons reuse restrictions (trademarked, personality rights…), shown as a warning.
    #[serde(default)]
    pub restrictions: Vec<String>,
    /// Looks AI-generated (category / title / tags); ranked so low it never shows.
    #[serde(default)]
    pub ai_generated: bool,
    /// Set by `rank`.
    #[serde(default)]
    pub score: f32,
    /// Smaller than the project frame after the 16:9 crop (set by `rank`).
    #[serde(default)]
    pub low_res: bool,
}

impl Candidate {
    /// Asset id: `<source>_<id>` with only safe file-name characters.
    pub fn asset_id(&self) -> String {
        let id: String = self
            .source_id
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
            .take(64)
            .collect();
        format!("{}_{id}", self.source.as_str())
    }
}

/// Words that mark a file as AI-generated in titles, tags or categories.
const AI_MARKERS: [&str; 7] =
    ["ai-generated", "ai generated", "dall-e", "dall·e", "midjourney", "stable diffusion", "generative ai"];

pub fn looks_ai_generated(texts: &[&str]) -> bool {
    texts.iter().any(|t| {
        let t = t.to_lowercase();
        AI_MARKERS.iter().any(|m| t.contains(m))
    })
}

#[derive(Debug, Clone, thiserror::Error, PartialEq)]
pub enum StockError {
    #[error("no {} API key is stored", .0.display())]
    NoKey(Source),
    #[error("{} rejected the API key", .0.display())]
    BadKey(Source),
    #[error("{} limit reached", .site.display())]
    RateLimited { site: Source, retry_after: Duration, daily: bool },
    #[error("{} is not reachable: {msg}", .site.display())]
    Unavailable { site: Source, msg: String },
    #[error("unexpected answer from {}: {msg}", .site.display())]
    BadResponse { site: Source, msg: String },
    #[error("download failed: {0}")]
    Download(String),
    #[error("cancelled")]
    Cancelled,
}

impl StockError {
    /// Stable kind for the UI (`error.<kind>` strings).
    pub fn kind(&self) -> &'static str {
        match self {
            Self::NoKey(_) => "stock_no_key",
            Self::BadKey(_) => "stock_bad_key",
            Self::RateLimited { .. } => "stock_rate_limited",
            Self::Unavailable { .. } => "stock_unavailable",
            Self::BadResponse { .. } => "stock_bad_response",
            Self::Download(_) => "stock_download",
            Self::Cancelled => "cancelled",
        }
    }
}

pub fn transport_error(source: Source, e: TransportError) -> StockError {
    StockError::Unavailable { site: source, msg: e.to_string() }
}

/// Common status handling; 429 is left to the source (each reports its limit differently).
pub fn check_status(source: Source, resp: &HttpResponse) -> Result<(), StockError> {
    match resp.status {
        200..=299 => Ok(()),
        401 | 403 if source.key_user().is_some() => Err(StockError::BadKey(source)),
        500..=599 | 403 => Err(StockError::Unavailable { site: source, msg: format!("HTTP {}", resp.status) }),
        s => Err(StockError::BadResponse {
            site: source,
            msg: format!("HTTP {s} — {}", String::from_utf8_lossy(&resp.body).chars().take(300).collect::<String>()),
        }),
    }
}

pub fn bad_json(source: Source, e: serde_json::Error) -> StockError {
    StockError::BadResponse { site: source, msg: format!("could not read the search results: {e}") }
}

/// Title from a landing-page slug: `…/photo/server-room-in-a-data-center-1234/` →
/// "server room in a data center".
pub fn title_from_slug(url: &str) -> String {
    let slug = url.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    let words: Vec<&str> = slug.split('-').filter(|w| !w.chars().all(|c| c.is_ascii_digit())).collect();
    words.join(" ")
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn candidate(source: Source, w: u32, h: u32, license: License) -> Candidate {
        Candidate {
            source,
            source_id: "1".into(),
            media: Media::Photo,
            title: "server rack in a data center".into(),
            tags: vec![],
            author: "Jane".into(),
            author_url: None,
            license,
            page_url: "https://example.org/p".into(),
            file_url: "https://example.org/f.jpg".into(),
            thumb_url: "https://example.org/t.jpg".into(),
            width: w,
            height: h,
            duration_ms: None,
            restrictions: vec![],
            ai_generated: false,
            score: 0.0,
            low_res: false,
        }
    }

    #[test]
    fn user_agent_has_version_and_contact() {
        let ua = user_agent();
        assert!(ua.starts_with(&format!("NiteanStudio/{} (", env!("CARGO_PKG_VERSION"))));
        assert!(ua.contains("github.com/Narak76543/niteam-studio") && ua.contains('@'));
    }

    #[test]
    fn app_and_crate_versions_match() {
        // The User-Agent uses the crate version; the installer uses tauri.conf.json.
        let conf: serde_json::Value = serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        assert_eq!(conf["version"], env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn asset_ids_are_file_safe() {
        let mut c = candidate(Source::Openverse, 1, 1, License::cc0());
        c.source_id = "c025bb1a-e95b/../x".into();
        assert_eq!(c.asset_id(), "openverse_c025bb1a-e95b____x");
    }

    #[test]
    fn spots_ai_generated_files() {
        assert!(looks_ai_generated(&["DALL·E 2025-01-31 21.58.22 - A realistic data center"]));
        assert!(looks_ai_generated(&["Category|AI-generated images"]));
        assert!(!looks_ai_generated(&["Racks Amravati Data Center.jpg"]));
    }

    #[test]
    fn slug_titles() {
        assert_eq!(title_from_slug("https://www.pexels.com/photo/server-room-in-a-data-center-1234/"), "server room in a data center");
    }
}
