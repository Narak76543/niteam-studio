//! Shared state managed by Tauri.

use std::sync::Arc;

use tokio::sync::RwLock;

use crate::gemini::GeminiClient;
use crate::pipeline::ProjectStore;
use crate::pipeline::jobs::JobRegistry;
use crate::storage::{AppPaths, Settings};
use crate::tts::voxcpm_server::VoxServer;

pub struct AppState {
    pub paths: AppPaths,
    pub settings: RwLock<Settings>,
    pub gemini: GeminiClient,
    pub store: Arc<ProjectStore>,
    pub script_jobs: Arc<JobRegistry>,
    pub voice_jobs: Arc<JobRegistry>,
    pub image_jobs: Arc<JobRegistry>,
    pub render_jobs: Arc<JobRegistry>,
    pub proofread_jobs: Arc<JobRegistry>,
    /// "Improve image prompts" (Scenes page).
    pub prompt_jobs: Arc<JobRegistry>,
    /// Real-world photos / clips search + download (`visual_job`).
    pub visual_jobs: Arc<JobRegistry>,
    /// Reading "From document" PDFs.
    pub source_jobs: Arc<JobRegistry>,
    /// Local model downloads (one at a time).
    pub install_jobs: Arc<JobRegistry>,
    /// One FFmpeg process at a time across the app (low-spec laptops).
    pub ffmpeg_lock: Arc<tokio::sync::Mutex<()>>,
    /// Local voice engine (VoxCPM2), shared by voice jobs, design and previews.
    pub voice_server: Arc<VoxServer>,
    /// Vulkan device index of the voice engine, found on first use.
    pub voice_device: tokio::sync::Mutex<Option<u32>>,
}
