//! Thin Tauri command layer. Each command returns `Result<T, String>` with a
//! user-readable message; logic lives in `storage/`, `gemini/` and `pipeline/`.

pub mod project;
pub mod script;
pub mod settings;
