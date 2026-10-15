//! All on-disk locations. Root is `%APPDATA%\NiteanStudio\` resolved through Tauri's
//! path API (never hard-coded).

use std::path::PathBuf;

use tauri::{AppHandle, Manager, Runtime};

use super::StorageError;

const APP_DIR_NAME: &str = "NiteanStudio";

#[derive(Debug, Clone)]
pub struct AppPaths {
    root: PathBuf,
    /// `%LOCALAPPDATA%/NiteanStudio` (large files: local models, 08 §5).
    local_root: PathBuf,
    /// Bundled stable-diffusion.cpp (`sd-server.exe` + DLLs), from the app resources.
    engine_dir: PathBuf,
    /// Bundled audio.cpp (`audiocpp_server.exe` + DLLs), from the app resources.
    audiocpp_dir: PathBuf,
}

impl AppPaths {
    pub fn from_app<R: Runtime>(app: &AppHandle<R>) -> Result<Self, StorageError> {
        let data_dir = app
            .path()
            .data_dir()
            .map_err(|e| StorageError::NoAppDir(e.to_string()))?;
        let local = app
            .path()
            .local_data_dir()
            .map_err(|e| StorageError::NoAppDir(e.to_string()))?;
        let resources = app
            .path()
            .resource_dir()
            .map_err(|e| StorageError::NoAppDir(e.to_string()))?;
        Ok(Self {
            root: data_dir.join(APP_DIR_NAME),
            local_root: local.join(APP_DIR_NAME),
            engine_dir: resources.join("sdcpp"),
            audiocpp_dir: resources.join("audiocpp"),
        })
    }

    /// Everything under one folder (tests).
    #[cfg(test)]
    pub fn new(root: PathBuf) -> Self {
        Self { local_root: root.join("local"), engine_dir: root.join("sdcpp"), audiocpp_dir: root.join("audiocpp"), root }
    }

    /// `%LOCALAPPDATA%/NiteanStudio/models`: one folder per model profile.
    pub fn models_dir(&self) -> PathBuf {
        self.local_root.join("models")
    }

    pub fn engine_exe(&self) -> PathBuf {
        self.engine_dir.join(if cfg!(windows) { "sd-server.exe" } else { "sd-server" })
    }

    pub fn audiocpp_exe(&self) -> PathBuf {
        self.audiocpp_dir.join(if cfg!(windows) { "audiocpp_server.exe" } else { "audiocpp_server" })
    }

    /// Server config + log of the local voice engine.
    pub fn audiocpp_work_dir(&self) -> PathBuf {
        self.local_root.join("audiocpp")
    }

    /// Voice Profiles (`docs/09-voice-profiles.md`): one folder per profile.
    pub fn voices_dir(&self) -> PathBuf {
        self.root.join("voices")
    }

    pub fn root(&self) -> &std::path::Path {
        &self.root
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
