//! Project state and generation pipeline.

pub mod editing;
pub mod hashing;
pub mod presets;
pub mod project;
pub mod script_job;
pub mod store;

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
