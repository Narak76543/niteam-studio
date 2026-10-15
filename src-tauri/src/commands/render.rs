//! Subtitle + render commands (Phase 4, `docs/03-pipeline.md` Steps 5–6).

use std::sync::Arc;

use base64::Engine as _;
use serde::Deserialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_opener::OpenerExt;
use tokio_util::sync::CancellationToken;

use crate::error::{CmdResult, to_cmd};
use crate::pipeline::ProjectError;
use crate::pipeline::render_job::{RenderEvents, RenderJob, RenderProgress};
use crate::pipeline::subs::{self, CuePng, SubsTask};
use crate::render::encoders::{self, Encoder};
use crate::render::ffmpeg::Ffmpeg;
use crate::state::AppState;
use crate::storage::log::LogFile;
use crate::storage::settings;

const RENDER_PROGRESS_EVENT: &str = "render-progress";

struct TauriRenderEvents(AppHandle);

impl RenderEvents for TauriRenderEvents {
    fn progress(&self, progress: RenderProgress) {
        // Emitting only fails if the window is gone; nothing to do then.
        let _ = self.0.emit(RENDER_PROGRESS_EVENT, progress);
    }
}

// ---------- subtitles ----------

/// Scenes whose subtitle PNGs must be drawn (voice done, subtitles missing or outdated).
#[tauri::command]
pub async fn subs_plan(state: State<'_, AppState>, project_id: String) -> CmdResult<Vec<SubsTask>> {
    let project = state.store.get(&project_id).await.map_err(to_cmd)?;
    Ok(subs::plan(&project, &state.paths.project_dir(&project_id)))
}

#[derive(Debug, Deserialize)]
pub struct CueIn {
    pub text: String,
    pub start_ms: u64,
    pub end_ms: u64,
    /// PNG drawn by the WebView canvas, base64 (no data: prefix).
    pub png_base64: String,
}

/// Stores one scene's subtitle PNGs (`hash` from `subs_plan`).
#[tauri::command]
pub async fn subs_save(
    state: State<'_, AppState>,
    project_id: String,
    scene_id: String,
    hash: String,
    cues: Vec<CueIn>,
) -> CmdResult<()> {
    if state.render_jobs.is_running(&project_id) {
        return Err(to_cmd(ProjectError::WrongState("rendering".into())));
    }
    let cues = cues
        .into_iter()
        .map(|c| {
            let png = base64::engine::general_purpose::STANDARD
                .decode(c.png_base64.as_bytes())
                .map_err(|_| "A subtitle image could not be decoded.".to_owned())?;
            Ok(CuePng { text: c.text, start_ms: c.start_ms, end_ms: c.end_ms, png })
        })
        .collect::<CmdResult<Vec<_>>>()?;
    let dir = state.paths.project_dir(&project_id);
    let project = state.store.get(&project_id).await.map_err(to_cmd)?;
    let asset = subs::write_pngs(&project, &scene_id, &hash, &cues, &dir).map_err(to_cmd)?;
    state
        .store
        .update(&project_id, |p| subs::apply(p, &scene_id, asset))
        .await
        .map(|_| ())
        .map_err(to_cmd)
}

// ---------- encoders ----------

async fn detect_encoder(state: &AppState) -> CmdResult<Encoder> {
    let ffmpeg = Ffmpeg::locate().map_err(|e| e.to_string())?;
    let encoder = {
        let _guard = state.ffmpeg_lock.lock().await;
        encoders::detect(&ffmpeg, state.paths.root(), &CancellationToken::new())
            .await
            .map_err(|e| e.to_string())?
    };
    let mut s = state.settings.write().await;
    s.render.encoder = Some(encoder.name().to_owned());
    settings::save(&state.paths, &s).map_err(to_cmd)?;
    LogFile::new(state.paths.app_log()).info(&format!("encoder detected: {}", encoder.name()));
    Ok(encoder)
}

/// Re-detects the H.264 encoder (Settings → Video) and caches it.
#[tauri::command]
pub async fn encoders_detect(state: State<'_, AppState>) -> CmdResult<String> {
    detect_encoder(&state).await.map(|e| e.name().to_owned())
}

// ---------- render ----------

#[tauri::command]
pub async fn render_start(app: AppHandle, state: State<'_, AppState>, project_id: String) -> CmdResult<()> {
    for jobs in [&state.script_jobs, &state.voice_jobs, &state.image_jobs] {
        if jobs.is_running(&project_id) {
            return Err("Wait until script, voice and image generation have finished.".into());
        }
    }
    let project = state.store.get(&project_id).await.map_err(to_cmd)?;
    let dir = state.paths.project_dir(&project_id);
    crate::pipeline::render_job::check_ready(&project, &dir).map_err(to_cmd)?;
    let ffmpeg = Ffmpeg::locate().map_err(|e| e.to_string())?;

    let cached = state.settings.read().await.render.encoder.clone();
    let encoder = match cached.as_deref().and_then(Encoder::from_name) {
        Some(e) => e,
        None => detect_encoder(&state).await?,
    };
    let output_dir = state.settings.read().await.output_dir.clone();
    let cancel = state.render_jobs.start(&project_id).map_err(to_cmd)?;
    let job = RenderJob {
        store: state.store.clone(),
        paths: state.paths.clone(),
        ffmpeg,
        encoder,
        output_dir,
        log: LogFile::new(state.paths.project_log(&project_id)),
        ffmpeg_lock: state.ffmpeg_lock.clone(),
    };
    let jobs = state.render_jobs.clone();
    let events: Arc<dyn RenderEvents> = Arc::new(TauriRenderEvents(app));
    tauri::async_runtime::spawn(async move {
        job.run(&project_id, cancel, events).await;
        jobs.finish(&project_id);
    });
    Ok(())
}

#[tauri::command]
pub fn render_cancel(state: State<'_, AppState>, project_id: String) {
    state.render_jobs.cancel(&project_id);
}

#[tauri::command]
pub fn render_running(state: State<'_, AppState>, project_id: String) -> bool {
    state.render_jobs.is_running(&project_id)
}

/// Opens the finished video (`reveal = false`) or shows it in Explorer (`reveal = true`).
#[tauri::command]
pub async fn render_open_output(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
    reveal: bool,
) -> CmdResult<()> {
    let project = state.store.get(&project_id).await.map_err(to_cmd)?;
    let path = project
        .render
        .output_path
        .filter(|p| std::path::Path::new(p).is_file())
        .ok_or_else(|| "The video file was not found. Render it again.".to_owned())?;
    let opener = app.opener();
    let result = if reveal {
        opener.reveal_item_in_dir(&path)
    } else {
        opener.open_path(path, None::<&str>)
    };
    result.map_err(|e| format!("Could not open the video: {e}"))
}
