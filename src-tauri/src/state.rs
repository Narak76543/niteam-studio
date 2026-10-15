//! Shared state managed by Tauri.

use std::sync::Arc;

use tokio::sync::RwLock;

use crate::gemini::GeminiClient;
use crate::pipeline::ProjectStore;
use crate::pipeline::jobs::JobRegistry;
use crate::storage::{AppPaths, Settings};

pub struct AppState {
    pub paths: AppPaths,
    pub settings: RwLock<Settings>,
    pub gemini: GeminiClient,
    pub store: Arc<ProjectStore>,
    pub script_jobs: Arc<JobRegistry>,
    pub voice_jobs: Arc<JobRegistry>,
    pub image_jobs: Arc<JobRegistry>,
    pub render_jobs: Arc<JobRegistry>,
    /// One FFmpeg process at a time across the app (low-spec laptops).
    pub ffmpeg_lock: Arc<tokio::sync::Mutex<()>>,
}
