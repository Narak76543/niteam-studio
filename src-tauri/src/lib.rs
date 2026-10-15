mod commands;
mod error;
mod gemini;
mod images;
mod local;
mod net;
mod pipeline;
mod render;
mod state;
mod storage;
mod tts;

use std::sync::Arc;

use tauri::Manager;
use tokio::sync::RwLock;

use crate::gemini::GeminiClient;
use crate::pipeline::ProjectStore;
use crate::pipeline::jobs::JobRegistry;
use crate::state::AppState;
use crate::storage::{AppPaths, settings};

fn init_state(app: &tauri::App) -> Result<AppState, String> {
    let paths = AppPaths::from_app(app.handle()).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(paths.projects_dir()).map_err(|e| e.to_string())?;
    // A corrupt settings file must not brick the app: fall back to defaults.
    let mut settings = settings::load(&paths).unwrap_or_default();
    // 08 §2: cache the best GPU (DXGI) so the UI and image jobs know about local mode.
    let gpu = local::gpu::detect();
    if settings.gpu != gpu {
        settings.gpu = gpu;
        let _ = settings::save(&paths, &settings);
    }
    let gemini = GeminiClient::new(&settings.concurrency).map_err(|e| e.to_string())?;
    let store = ProjectStore::new(paths.clone());
    // Best effort; a failure here only means stale "running" badges.
    let _ = store.recover_interrupted();
    Ok(AppState {
        paths,
        settings: RwLock::new(settings),
        gemini,
        store: Arc::new(store),
        script_jobs: Arc::new(JobRegistry::default()),
        voice_jobs: Arc::new(JobRegistry::default()),
        image_jobs: Arc::new(JobRegistry::default()),
        render_jobs: Arc::new(JobRegistry::default()),
        proofread_jobs: Arc::new(JobRegistry::default()),
        install_jobs: Arc::new(JobRegistry::default()),
        ffmpeg_lock: Arc::new(tokio::sync::Mutex::new(())),
        voice_server: Arc::new(tts::voxcpm_server::VoxServer::default()),
        voice_device: tokio::sync::Mutex::new(None),
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let state = init_state(app)?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::settings::settings_get,
            commands::settings::settings_save,
            commands::settings::api_key_set,
            commands::settings::api_key_exists,
            commands::settings::api_key_clear,
            commands::settings::api_key_test,
            commands::project::presets_list,
            commands::project::project_list,
            commands::project::project_create,
            commands::project::project_get,
            commands::project::project_delete,
            commands::script::script_generate,
            commands::script::script_cancel,
            commands::script::script_running,
            commands::script::scene_update,
            commands::script::scene_add,
            commands::script::scene_delete,
            commands::script::scenes_reorder,
            commands::script::script_approve,
            commands::script::script_reopen,
            commands::render::subs_plan,
            commands::render::subs_save,
            commands::render::encoders_detect,
            commands::render::render_start,
            commands::render::render_cancel,
            commands::render::render_running,
            commands::render::render_open_output,
            commands::render::render_timeline,
            commands::proofread::proofread_start,
            commands::proofread::proofread_cancel,
            commands::proofread::proofread_running,
            commands::proofread::proofread_accept,
            commands::proofread::proofread_dismiss,
            commands::proofread::project_set_keep_terms,
            commands::images::project_set_style,
            commands::local::local_status,
            commands::local::local_install,
            commands::local::local_install_cancel,
            commands::local::local_verify,
            commands::local::local_delete,
            commands::local::local_offer_dismiss,
            commands::local::local_benchmark,
            commands::render::scene_image_full,
            commands::render::render_set_subtitle_style,
            commands::generate::generate_start,
            commands::generate::generate_cancel,
            commands::generate::generate_running,
            commands::generate::scene_regenerate,
            commands::generate::scene_voice_audio,
            commands::generate::voices_catalog,
            commands::generate::voice_preview,
            commands::images::cloudflare_token_set,
            commands::images::cloudflare_token_exists,
            commands::images::cloudflare_token_clear,
            commands::images::cloudflare_test,
            commands::images::images_estimate,
            commands::images::images_start,
            commands::images::images_cancel,
            commands::images::images_running,
            commands::images::scene_image_thumb,
            commands::images::scene_image_prompt_update,
            commands::images::visual_bible_update,
            commands::images::visual_bible_generate,
            commands::images::project_set_scenes_per_image,
            commands::images::images_catalog,
            commands::voices::voices_status,
            commands::voices::voices_model_install,
            commands::voices::voices_model_verify,
            commands::voices::voices_model_delete,
            commands::voices::voice_design_samples,
            commands::voices::voice_design_sample_audio,
            commands::voices::voice_profile_create_design,
            commands::voices::voice_profile_import,
            commands::voices::voice_profile_update,
            commands::voices::voice_profile_duplicate,
            commands::voices::voice_profile_delete,
            commands::voices::voice_profile_preview,
            commands::voices::voice_profile_reference_audio,
            commands::voices::voice_profile_set_default,
            commands::voices::project_set_voice_profile,
        ])
        .build(tauri::generate_context!())
        .map(|app| {
            app.run(|handle, event| {
                // The voice engine is a long-lived child process: stop it with the app.
                if let tauri::RunEvent::Exit = event
                    && let Some(state) = handle.try_state::<AppState>()
                {
                    tauri::async_runtime::block_on(state.voice_server.stop());
                }
            });
        });
    if let Err(e) = result {
        eprintln!("Nitean Studio failed to start: {e}");
        std::process::exit(1);
    }
}
