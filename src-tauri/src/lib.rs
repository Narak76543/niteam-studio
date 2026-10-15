mod commands;
mod error;
mod gemini;
mod images;
mod net;
mod pipeline;
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
    let settings = settings::load(&paths).unwrap_or_default();
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
        ])
        .run(tauri::generate_context!());
    if let Err(e) = result {
        eprintln!("Nitean Studio failed to start: {e}");
        std::process::exit(1);
    }
}
