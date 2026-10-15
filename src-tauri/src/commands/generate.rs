//! Voice generation commands (Phase 2) and voice playback / preview.

use std::path::Component;
use std::sync::Arc;

use tauri::ipc::Response;
use tauri::{AppHandle, Emitter, State};

use super::retry_events::retry_hooks;
use crate::error::{CmdResult, to_cmd};
use crate::gemini::GeminiError;
use crate::gemini::retry::RetryHooks;
use crate::pipeline::ProjectError;
use crate::pipeline::project::{Language, ProjectStatus};
use crate::pipeline::voice_job::{GenerateProgress, SceneProgress, VoiceEvents, VoiceJob};
use crate::state::AppState;
use crate::storage::log::LogFile;
use crate::storage::secrets;
use crate::storage::settings::{Settings, VoiceDefaults};
use crate::tts::edge::EdgeTts;
use crate::tts::gemini::GeminiTts;
use crate::tts::voices::{self, VoiceCatalog};
use crate::tts::{EngineKind, TtsEngine, TtsRequest};

pub const SCENE_PROGRESS_EVENT: &str = "scene-progress";
pub const GENERATE_PROGRESS_EVENT: &str = "generate-progress";

struct TauriVoiceEvents(AppHandle);

impl VoiceEvents for TauriVoiceEvents {
    fn scene(&self, progress: SceneProgress) {
        // Emitting only fails if the window is gone; nothing to do then.
        let _ = self.0.emit(SCENE_PROGRESS_EVENT, progress);
    }
    fn progress(&self, progress: GenerateProgress) {
        let _ = self.0.emit(GENERATE_PROGRESS_EVENT, progress);
    }
}

fn build_engine(
    state: &AppState,
    settings: &Settings,
    voice: &VoiceDefaults,
) -> CmdResult<Arc<dyn TtsEngine>> {
    match voice.engine {
        EngineKind::Edge => Ok(Arc::new(EdgeTts::new(settings.concurrency.tts))),
        EngineKind::Gemini => {
            let key = secrets::get_api_key()
                .map_err(to_cmd)?
                .ok_or_else(|| to_cmd(GeminiError::MissingKey))?;
            Ok(Arc::new(GeminiTts::new(state.gemini.clone(), key)))
        }
    }
}

/// Starts the voice job in the background; progress arrives as `scene-progress` /
/// `generate-progress`, retries as `gemini-retry` (scope `voice`).
async fn start_voice_job(
    app: AppHandle,
    state: &AppState,
    project_id: String,
    only: Option<Vec<String>>,
) -> CmdResult<()> {
    if state.script_jobs.is_running(&project_id) {
        return Err(to_cmd(ProjectError::Busy));
    }
    let settings = state.settings.read().await.clone();
    let project = state.store.get(&project_id).await.map_err(to_cmd)?;
    if project.status == ProjectStatus::ScriptDraft {
        return Err("Approve the script before generating voices.".into());
    }
    let engine = build_engine(state, &settings, &settings.voice)?;
    let cancel = state.voice_jobs.start(&project_id).map_err(to_cmd)?;
    let mut log = LogFile::new(state.paths.project_log(&project_id));
    if let Ok(Some(key)) = secrets::get_api_key() {
        log = log.redacting(key.expose());
    }
    let hooks = retry_hooks(
        app.clone(),
        "voice",
        Some(project_id.clone()),
        cancel,
        log.clone(),
    );
    let job = VoiceJob {
        store: state.store.clone(),
        paths: state.paths.clone(),
        engine,
        voice: settings.voice.resolve(project.language),
        hooks,
        log,
        concurrency: settings.concurrency.tts,
    };
    let jobs = state.voice_jobs.clone();
    let events: Arc<dyn VoiceEvents> = Arc::new(TauriVoiceEvents(app));
    tauri::async_runtime::spawn(async move {
        job.run(&project_id, only, events).await;
        jobs.finish(&project_id);
    });
    Ok(())
}

#[tauri::command]
pub async fn generate_start(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
) -> CmdResult<()> {
    start_voice_job(app, &state, project_id, None).await
}

#[tauri::command]
pub fn generate_cancel(state: State<'_, AppState>, project_id: String) {
    state.voice_jobs.cancel(&project_id);
}

#[tauri::command]
pub fn generate_running(state: State<'_, AppState>, project_id: String) -> bool {
    state.voice_jobs.is_running(&project_id)
}

/// Redo one scene's asset. Phase 2 supports `what = "voice"`.
#[tauri::command]
pub async fn scene_regenerate(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
    scene_id: String,
    what: String,
) -> CmdResult<()> {
    if what != "voice" {
        return Err("Only voices can be regenerated in this version.".into());
    }
    start_voice_job(app, &state, project_id, Some(vec![scene_id])).await
}

/// Only `scenes/<id>/voice.*` paths inside the project folder are served.
fn is_safe_relative(rel: &str) -> bool {
    let path = std::path::Path::new(rel);
    rel.starts_with("scenes/") && path.components().all(|c| matches!(c, Component::Normal(_)))
}

/// Raw bytes of a scene's voice file (MP3 or WAV) for playback in the UI.
#[tauri::command]
pub async fn scene_voice_audio(
    state: State<'_, AppState>,
    project_id: String,
    scene_id: String,
) -> CmdResult<Response> {
    let project = state.store.get(&project_id).await.map_err(to_cmd)?;
    let rel = project
        .scenes
        .iter()
        .find(|s| s.id == scene_id)
        .and_then(|s| s.voice.path.clone())
        .ok_or_else(|| "This scene has no voice yet.".to_owned())?;
    if !is_safe_relative(&rel) {
        return Err("Invalid voice path.".into());
    }
    let path = state.paths.project_dir(&project_id).join(&rel);
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| format!("Could not read the voice file: {e}"))?;
    Ok(Response::new(bytes))
}

#[tauri::command]
pub fn voices_catalog() -> VoiceCatalog {
    voices::catalog()
}

const PREVIEW_KM: &str = "សួស្តី! នេះជាការសាកល្បងសំឡេង សម្រាប់វីដេអូរបស់អ្នក។";
const PREVIEW_EN: &str = "Hello! This is a preview of the narration voice for your video.";

/// Speaks a short sample with the given (possibly unsaved) voice settings.
#[tauri::command]
pub async fn voice_preview(
    state: State<'_, AppState>,
    language: Language,
    voice: VoiceDefaults,
) -> CmdResult<Response> {
    let mut check = state.settings.read().await.clone();
    check.voice = voice.clone();
    check.validate().map_err(to_cmd)?;
    let engine = build_engine(&state, &check, &voice)?;
    let resolved = voice.resolve(language);
    let request = TtsRequest {
        text: match language {
            Language::Km => PREVIEW_KM,
            Language::En => PREVIEW_EN,
        }
        .to_owned(),
        voice: resolved.voice_name,
        rate: resolved.rate,
        pitch: resolved.pitch,
        model: resolved.model,
        style: resolved.direction,
    };
    let output = engine
        .synthesize(&request, &RetryHooks::default())
        .await
        .map_err(|e| e.to_string())?;
    Ok(Response::new(output.audio))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_scene_paths_are_served() {
        assert!(is_safe_relative("scenes/s001/voice.mp3"));
        assert!(!is_safe_relative("scenes/../project.json"));
        assert!(!is_safe_relative("C:/Windows/win.ini"));
        assert!(!is_safe_relative("project.json"));
    }
}
