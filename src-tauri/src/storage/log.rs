//! Plain-text logs: `projects/<id>/logs/pipeline.log` per project and `logs/app.log`
//! for app-level events (e.g. key tests). Best effort: a failed log write never fails a job.
//! Secrets registered with [`LogFile::redacting`] are replaced before anything is written.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

const MAX_BYTES: u64 = 1024 * 1024;
const REDACTED: &str = "<redacted>";

#[derive(Debug, Clone, Copy)]
pub enum Level {
    Info,
    Warn,
    Error,
}

impl Level {
    fn as_str(self) -> &'static str {
        match self {
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
        }
    }
}

#[derive(Clone)]
pub struct LogFile {
    path: PathBuf,
    secrets: Vec<String>,
}

impl LogFile {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            secrets: vec![],
        }
    }

    /// Never write `secret` (e.g. the API key) even if an error message echoes it.
    pub fn redacting(mut self, secret: &str) -> Self {
        if !secret.is_empty() {
            self.secrets.push(secret.to_owned());
        }
        self
    }

    pub fn info(&self, msg: &str) {
        self.write(Level::Info, msg);
    }

    pub fn warn(&self, msg: &str) {
        self.write(Level::Warn, msg);
    }

    pub fn error(&self, msg: &str) {
        self.write(Level::Error, msg);
    }

    pub fn write(&self, level: Level, msg: &str) {
        let mut clean = msg.replace(['\r', '\n'], " ");
        for s in &self.secrets {
            clean = clean.replace(s.as_str(), REDACTED);
        }
        let ts = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let line = format!("{ts} [{}] {clean}\n", level.as_str());
        // Logging must never break the pipeline: ignore I/O errors.
        let _ = self.append(&line);
    }

    fn append(&self, line: &str) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        if fs::metadata(&self.path).is_ok_and(|m| m.len() > MAX_BYTES) {
            let mut old = self.path.clone().into_os_string();
            old.push(".1");
            fs::rename(&self.path, old)?;
        }
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        f.write_all(line.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_lines_and_redacts_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("logs").join("pipeline.log");
        let log = LogFile::new(path.clone()).redacting("AIzaSECRET123");
        log.warn("retry: key AIzaSECRET123 rejected\nsecond line");
        log.info("done");
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains("AIzaSECRET123"));
        assert!(text.contains("[WARN] retry: key <redacted> rejected second line"));
        assert_eq!(text.lines().count(), 2);
    }

    #[test]
    fn rotates_large_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.log");
        fs::write(&path, vec![b'x'; (MAX_BYTES + 1) as usize]).unwrap();
        LogFile::new(path.clone()).info("fresh");
        assert!(dir.path().join("app.log.1").exists());
        assert!(fs::read_to_string(&path).unwrap().contains("fresh"));
    }
}
