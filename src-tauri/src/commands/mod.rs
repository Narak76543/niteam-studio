//! Thin Tauri command layer. Each command returns `Result<T, String>` with a
//! user-readable message; logic lives in `storage/`, `gemini/` and `pipeline/`.

pub mod generate;
pub mod images;
pub mod local;
pub mod project;
pub mod proofread;
pub mod render;
pub mod retry_events;
pub mod script;
pub mod settings;
