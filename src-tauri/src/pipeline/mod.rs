//! Project state and generation pipeline.

pub mod editing;
pub mod events;
pub mod hashing;
pub mod image_job;
pub mod jobs;
pub mod khmer_numbers;
pub mod presets;
pub mod project;
pub mod render_job;
#[cfg(test)]
mod render_e2e;
pub mod script_job;
pub mod store;
pub mod subs;
pub mod visual_bible;
pub mod voice_job;

pub use store::ProjectStore;

use crate::gemini::GeminiError;
use crate::storage::StorageError;

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("Project not found.")]
    NotFound,
    #[error("Invalid project id.")]
    InvalidId,
    #[error("Scene not found.")]
    SceneNotFound,
    #[error("{0}")]
    Invalid(String),
    #[error("This can't be done while the project is in the '{0}' step.")]
    WrongState(String),
    #[error("Script generation is already running for this project.")]
    Busy,
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(transparent)]
    Gemini(#[from] GeminiError),
}

impl ProjectError {
    /// Stable code for the UI's friendly message (Gemini kinds, else "other").
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Gemini(e) => e.kind(),
            _ => "other",
        }
    }

    /// Technical description (exact HTTP status + body for Gemini errors).
    pub fn detail(&self) -> String {
        match self {
            Self::Gemini(e) => e.detail(),
            other => other.to_string(),
        }
    }
}
