//! App-wide error type. Module errors convert into [`AppError`]; Tauri commands
//! turn it into a user-readable `String` at the boundary.

use crate::gemini::GeminiError;
use crate::pipeline::ProjectError;
use crate::storage::StorageError;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(transparent)]
    Gemini(#[from] GeminiError),
    #[error(transparent)]
    Project(#[from] ProjectError),
}

impl From<AppError> for String {
    fn from(err: AppError) -> Self {
        err.to_string()
    }
}

/// Result type returned by every Tauri command.
pub type CmdResult<T> = Result<T, String>;

/// Converts any module error into the command-boundary message.
pub fn to_cmd<E: Into<AppError>>(err: E) -> String {
    err.into().to_string()
}
