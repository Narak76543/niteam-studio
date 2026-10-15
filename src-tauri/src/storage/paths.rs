//! All on-disk locations. Root is `%APPDATA%\NiteanStudio\` resolved through Tauri's
//! path API (never hard-coded).

use std::path::PathBuf;

use tauri::{AppHandle, Manager, Runtime};

use super::StorageError;

const APP_DIR_NAME: &str = "NiteanStudio";

#[derive(Debug, Clone)]
pub struct AppPaths {
    root: PathBuf,
}

impl AppPaths {
    pub fn from_app<R: Runtime>(app: &AppHandle<R>) -> Result<Self, StorageError> {
        let data_dir = app
            .path()
            .data_dir()
            .map_err(|e| StorageError::NoAppDir(e.to_string()))?;
        Ok(Self::new(data_dir.join(APP_DIR_NAME)))
    }

    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn settings_file(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    pub fn projects_dir(&self) -> PathBuf {
        self.root.join("projects")
    }

    /// Folder of one project. `id` must already be validated with [`is_valid_id`].
    pub fn project_dir(&self, id: &str) -> PathBuf {
        self.projects_dir().join(id)
    }

    pub fn project_file(&self, id: &str) -> PathBuf {
        self.project_dir(id).join("project.json")
    }

    /// `projects/<id>/logs/pipeline.log` (no secrets).
    pub fn project_log(&self, id: &str) -> PathBuf {
        self.project_dir(id).join("logs").join("pipeline.log")
    }

    /// `logs/app.log` for events that don't belong to a project (e.g. key tests).
    pub fn app_log(&self) -> PathBuf {
        self.root.join("logs").join("app.log")
    }
}

/// Project ids are ULIDs: 26 chars of Crockford base32. Rejecting anything else
/// keeps ids from escaping the projects folder (`..`, slashes).
pub fn is_valid_id(id: &str) -> bool {
    id.len() == 26 && id.chars().all(|c| c.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_path_like_ids() {
        assert!(is_valid_id("01J9ZQ3V4R8S6T2W5X7Y9A1B3C"));
        assert!(!is_valid_id("..\\..\\Windows"));
        assert!(!is_valid_id("01J9ZQ3V4R8S6T2W5X7Y9A1B3/"));
        assert!(!is_valid_id(""));
    }
}
