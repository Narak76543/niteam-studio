//! Shared state managed by Tauri.

use std::sync::Arc;

use tokio::sync::RwLock;

use crate::gemini::GeminiClient;
use crate::pipeline::ProjectStore;
use crate::pipeline::script_job::ScriptJobs;
use crate::storage::{AppPaths, Settings};

pub struct AppState {
    pub paths: AppPaths,
    pub settings: RwLock<Settings>,
    pub gemini: GeminiClient,
    pub store: Arc<ProjectStore>,
    pub script_jobs: Arc<ScriptJobs>,
}
