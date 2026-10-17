//! Stock search cache: `%LOCALAPPDATA%\NiteanStudio\cache\stock\<source>\<sha256>.json`, kept
//! 24 h. Pixabay requires caching requests for 24 h; for Openverse it keeps us under the
//! anonymous 200 searches / day. Parsed candidates are stored (license filter applied on read).

use std::path::PathBuf;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{Candidate, Media, Source};
use crate::storage::{StorageError, atomic};

const TTL_HOURS: i64 = 24;

#[derive(Serialize, Deserialize)]
struct Entry {
    /// RFC 3339.
    fetched_at: String,
    candidates: Vec<Candidate>,
}

fn fresh(e: &Entry, now: DateTime<Utc>) -> bool {
    DateTime::parse_from_rfc3339(&e.fetched_at).is_ok_and(|t| now - t.with_timezone(&Utc) < Duration::hours(TTL_HOURS))
}

pub struct SearchCache {
    dir: PathBuf,
}

/// Cache key: source, media, normalised query, page and a request variant (e.g. license set).
pub fn key(source: Source, media: Media, query: &str, page: u32, variant: &str) -> String {
    let query = query.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    let mut h = Sha256::new();
    h.update(format!("{}|{media:?}|{query}|{page}|{variant}", source.as_str()));
    hex::encode(h.finalize())
}

impl SearchCache {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn file(&self, source: Source, key: &str) -> PathBuf {
        self.dir.join(source.as_str()).join(format!("{key}.json"))
    }

    /// Fresh results, or `None` (missing, unreadable or older than 24 h).
    pub fn get(&self, source: Source, key: &str, now: DateTime<Utc>) -> Option<Vec<Candidate>> {
        let entry: Entry = atomic::read_json(&self.file(source, key)).ok().flatten()?;
        fresh(&entry, now).then_some(entry.candidates)
    }

    pub fn put(&self, source: Source, key: &str, candidates: &[Candidate], now: DateTime<Utc>) -> Result<(), StorageError> {
        atomic::write_json(&self.file(source, key), &Entry { fetched_at: now.to_rfc3339(), candidates: candidates.to_vec() })
    }

    /// Deletes expired entries (best effort).
    pub fn prune(&self, now: DateTime<Utc>) {
        let Ok(sources) = std::fs::read_dir(&self.dir) else { return };
        for dir in sources.flatten() {
            let Ok(files) = std::fs::read_dir(dir.path()) else { continue };
            for f in files.flatten() {
                let old = atomic::read_json::<Entry>(&f.path())
                    .ok()
                    .flatten()
                    .is_none_or(|e| !fresh(&e, now));
                if old {
                    let _ = std::fs::remove_file(f.path());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stock::license::License;
    use crate::stock::tests::candidate;

    #[test]
    fn keeps_results_for_24_hours() {
        let dir = tempfile::tempdir().unwrap();
        let cache = SearchCache::new(dir.path().to_path_buf());
        let k = key(Source::Openverse, Media::Photo, "  Server   RACK ", 1, "cc0,pdm,by");
        assert_eq!(k, key(Source::Openverse, Media::Photo, "server rack", 1, "cc0,pdm,by"));
        assert_ne!(k, key(Source::Openverse, Media::Photo, "server rack", 1, "cc0,pdm,by,by-sa"));
        let t0 = Utc::now();
        let c = vec![candidate(Source::Openverse, 3000, 2000, License::cc0())];
        cache.put(Source::Openverse, &k, &c, t0).unwrap();
        assert_eq!(cache.get(Source::Openverse, &k, t0 + Duration::hours(23)), Some(c));
        assert_eq!(cache.get(Source::Openverse, &k, t0 + Duration::hours(25)), None);
        cache.prune(t0 + Duration::hours(25));
        assert!(!cache.file(Source::Openverse, &k).exists());
    }
}
