//! Local storage: app data paths, settings file, OS keyring secrets, atomic writes.

pub mod atomic;
pub mod paths;
pub mod secrets;
pub mod settings;

pub use paths::AppPaths;
pub use secrets::ApiKey;
pub use settings::Settings;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("Could not find the app data folder: {0}")]
    NoAppDir(String),
    #[error("File error ({path}): {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("Could not read {path}: {source}")]
    Parse {
        path: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("Could not save data: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("Windows Credential Manager error: {0}")]
    Keyring(String),
    #[error("The API key looks invalid. Paste the key exactly as shown in Google AI Studio.")]
    InvalidKeyFormat,
    #[error("Invalid setting: {0}")]
    InvalidSetting(String),
}

impl StorageError {
    pub fn io(path: &std::path::Path, source: std::io::Error) -> Self {
        Self::Io {
            path: path.display().to_string(),
            source,
        }
    }
}
