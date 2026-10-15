//! Settings → Voices (`docs/09-voice-profiles.md`): the VoxCPM2 voice model, Voice Profiles
//! (design from a description, import a recording, edit, preview) and the local engine used
//! by voice jobs.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use tauri::ipc::Response;
use tauri::{AppHandle, State};
use tokio_util::sync::CancellationToken;

use super::local::{INSTALL_KEY, save_settings, spawn_install};
use crate::error::{CmdResult, to_cmd};
use crate::images::provider::ProviderId;
use crate::local::gpu::GpuInfo;
use crate::local::install::{self, ProfileStatus};
use crate::local::tts_manifest::{self, TtsModel};
use crate::pipeline::project::{Language, Project, VoiceSettings};
use crate::pipeline::store::now;
use crate::render::ffmpeg::Ffmpeg;
use crate::state::AppState;
use crate::storage::Settings;
use crate::tts::profiles::{
    DEFAULT_CFG, DEFAULT_STEPS, PREVIEW_FILE, ProfileError, ProfileMode, ProfileStore, REFERENCE_FILE, VoiceProfile,
};
use crate::tts::voxcpm::{ResolvedVoice, ServerSpeaker, Speaker, VoxCpmTts};
use crate::tts::voxcpm_server::{self, ServerSpec, SpeakRequest, VoxError};
use crate::tts::{EngineKind, TtsEngine, wav};

/// Every scene of every project uses the same seed: with the same reference clip this keeps
/// the voice identical across scenes (a new seed is only tried when a take looks wrong).
const SEED: u64 = 1_016;
/// Candidates per "From description" request.
const DESIGN_SAMPLES: usize = 3;
const DESIGN_DIR: &str = "_design";
const IMPORT_DIR: &str = "_import";
/// Reference clip limits (after trimming silence).
const MIN_REF_MS: u64 = 4_500;
const MAX_REF_MS: u64 = 30_500;

/// Spoken by every design candidate; becomes the profile's reference transcript.
const DESIGN_KM: &str = "សួស្តី! ថ្ងៃនេះ ខ្ញុំនឹងនិទានរឿងដ៏គួរឱ្យចាប់អារម្មណ៍មួយ អំពីប្រវត្តិសាស្ត្រ និងវិទ្យាសាស្ត្រ ជូនអ្នកទាំងអស់គ្នា។";
const DESIGN_EN: &str = "Hello! Today I will tell you a fascinating story about history and science, one step at a time.";
const PREVIEW_KM: &str = "តើអ្នកដឹងទេថា ប្រាសាទអង្គរវត្ត ត្រូវបានសាងសង់ឡើងក្នុងរយៈពេលតិចជាងសាមសិបឆ្នាំ? នេះជាសំឡេងនិទានរបស់អ្នក។";
const PREVIEW_EN: &str = "Did you know that Angkor Wat was built in less than thirty years? This is your narration voice.";

fn design_text(language: Language) -> &'static str {
    match language {
        Language::Km => DESIGN_KM,
        Language::En => DESIGN_EN,
    }
}

fn err(e: ProfileError) -> String {
    e.to_string()
}

fn store(state: &AppState) -> ProfileStore {
    ProfileStore::new(state.paths.voices_dir())
}

fn voice_model() -> CmdResult<TtsModel> {
    tts_manifest::shipped().default_model().cloned().ok_or_else(|| "No voice model is configured.".to_owned())
}

#[derive(Debug, Serialize)]
pub struct VoicesStatus {
    pub gpu: Option<GpuInfo>,
    /// Enough VRAM for the voice model.
    pub capable: bool,
    pub engine_version: String,
    pub engine_present: bool,
    pub model: Option<ProfileStatus>,
    /// A model download (voice or image) is running.
    pub installing: Option<String>,
    pub profiles: Vec<VoiceProfile>,
    pub default_profile: String,
}

#[tauri::command]
pub async fn voices_status(state: State<'_, AppState>) -> CmdResult<VoicesStatus> {
    let s = state.settings.read().await.clone();
    let m = tts_manifest::shipped();
    let vram = s.gpu.as_ref().map(|g| g.vram_mb);
    let model = install::status(&m.models, &state.paths.models_dir(), vram).into_iter().next();
    Ok(VoicesStatus {
        capable: model.as_ref().is_some_and(|m| m.fits),
        engine_version: m.engine_version,
        engine_present: state.paths.audiocpp_exe().is_file(),
        model,
        installing: state.install_jobs.running_keys().into_iter().next(),
        profiles: store(&state).list(),
        default_profile: s.voice.voxcpm_profile,
        gpu: s.gpu,
    })
}

/// Downloads the voice model (`model-download-progress` events, `profile_id` = model id).
#[tauri::command]
pub async fn voices_model_install(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    spawn_install(app, &state, voice_model()?, |_, _| {})
}

#[tauri::command]
pub async fn voices_model_verify(state: State<'_, AppState>) -> CmdResult<bool> {
    install::verify(&state.paths.models_dir(), &voice_model()?).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn voices_model_delete(state: State<'_, AppState>) -> CmdResult<()> {
    if state.install_jobs.is_running(INSTALL_KEY) {
        return Err("Wait until the download has finished or stop it first.".into());
    }
    if !state.voice_jobs.running_keys().is_empty() {
        return Err("Wait until the voices have finished.".into());
    }
    state.voice_server.stop().await;
    install::delete(&state.paths.models_dir(), &voice_model()?).map_err(|e| e.to_string())
}

/// Refuses while local images use the GPU (09 §6: one local model on the GPU at a time).
fn ensure_gpu_free(state: &AppState, s: &Settings) -> CmdResult<()> {
    let local_images = s.image.provider == ProviderId::LocalSdcpp || s.image.key_scene_provider == Some(ProviderId::LocalSdcpp);
    if local_images && !state.image_jobs.running_keys().is_empty() {
        return Err("Local images are being made on the GPU. Wait until they finish, then make the voices.".into());
    }
    Ok(())
}

async fn server_spec(state: &AppState, gpu: Option<&GpuInfo>) -> CmdResult<ServerSpec> {
    let model = voice_model()?;
    let file = model.model_file().ok_or_else(|| "No voice model is configured.".to_owned())?;
    let models = state.paths.models_dir();
    if !install::installed(&models, &model) {
        return Err(VoxError::ModelMissing.to_string());
    }
    let exe = state.paths.audiocpp_exe();
    if !exe.is_file() {
        return Err(VoxError::Missing(exe.display().to_string()).to_string());
    }
    let mut cached = state.voice_device.lock().await;
    let device = match *cached {
        Some(d) => d,
        None => {
            let devices = voxcpm_server::list_devices(&exe).await;
            let d = voxcpm_server::pick_device(&devices, gpu.map(|g| g.name.as_str()));
            if !devices.is_empty() {
                *cached = Some(d);
            }
            d
        }
    };
    Ok(ServerSpec {
        exe,
        model: install::file_path(&models, &model, &file.name),
        device,
        work_dir: state.paths.audiocpp_work_dir(),
    })
}

async fn speaker(state: &AppState) -> CmdResult<Arc<ServerSpeaker>> {
    let s = state.settings.read().await.clone();
    ensure_gpu_free(state, &s)?;
    let spec = server_spec(state, s.gpu.as_ref()).await?;
    Ok(Arc::new(ServerSpeaker { server: state.voice_server.clone(), spec, gpu_lock: state.ffmpeg_lock.clone() }))
}

fn resolved(dir: &Path, p: &VoiceProfile) -> ResolvedVoice {
    ResolvedVoice {
        reference: dir.join(&p.reference_wav),
        transcript: p.reference_transcript.clone(),
        style: p.style_hint.clone(),
        cfg: p.cfg_value,
        steps: p.inference_timesteps,
    }
}

pub fn profile_exists(state: &AppState, id: &str) -> bool {
    store(state).get(id).is_ok()
}

/// The engine for a voice job, plus the voice settings stored on the project (their hash
/// includes the profile's fingerprint, so editing a profile marks its scenes stale).
pub async fn vox_engine(state: &AppState, profile_id: &str, language: Language) -> CmdResult<(Arc<dyn TtsEngine>, VoiceSettings)> {
    if profile_id.is_empty() {
        return Err("Choose a voice in Settings → Voices first.".into());
    }
    let store = store(state);
    let p = store.get(profile_id).map_err(err)?;
    let dir = store.dir(&p.id).map_err(err)?;
    let speaker: Arc<dyn Speaker> = speaker(state).await?;
    let engine = VoxCpmTts::new(speaker, resolved(&dir, &p), language, SEED);
    let settings = VoiceSettings {
        engine: EngineKind::Voxcpm,
        model: format!("voxcpm2:{}", p.fingerprint(&dir)),
        voice_name: p.id,
        direction: p.style_hint,
        rate: 0,
        pitch: 0,
    };
    Ok((Arc::new(engine), settings))
}

fn clean_description(d: &str) -> String {
    d.replace(['(', ')'], " ").split_whitespace().collect::<Vec<_>>().join(" ").chars().take(300).collect()
}

/// "From description": speaks the test sentence with `DESIGN_SAMPLES` different seeds.
/// Candidates stay in `voices/_design/` until one is chosen; returns how many were made.
#[tauri::command]
pub async fn voice_design_samples(state: State<'_, AppState>, language: Language, description: String) -> CmdResult<usize> {
    let description = clean_description(&description);
    if description.is_empty() {
        return Err("Describe the voice first (for example: warm, calm female narrator in her thirties).".into());
    }
    let speaker = speaker(&state).await?;
    let dir = state.paths.voices_dir().join(DESIGN_DIR);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let base = u64::from(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(7));
    let cancel = CancellationToken::new();
    for i in 0..DESIGN_SAMPLES {
        let req = SpeakRequest {
            text: format!("({description}){}", design_text(language)),
            seed: base + i as u64,
            steps: DEFAULT_STEPS,
            guidance: DEFAULT_CFG,
            voice_ref: None,
            reference_text: None,
        };
        let bytes = speaker.speak(&req, &cancel).await.map_err(|e| e.to_string())?;
        let mut pcm = wav::trim(&wav::parse(&bytes)?, 120);
        wav::normalize(&mut pcm, -18.0, -1.0);
        std::fs::write(dir.join(format!("{i}.wav")), wav::to_wav(&pcm)).map_err(|e| e.to_string())?;
    }
    Ok(DESIGN_SAMPLES)
}

fn design_sample(state: &AppState, index: usize) -> CmdResult<PathBuf> {
    let p = state.paths.voices_dir().join(DESIGN_DIR).join(format!("{index}.wav"));
    if index >= DESIGN_SAMPLES || !p.is_file() {
        return Err("Make the voice samples again.".into());
    }
    Ok(p)
}

#[tauri::command]
pub async fn voice_design_sample_audio(state: State<'_, AppState>, index: usize) -> CmdResult<Response> {
    let p = design_sample(&state, index)?;
    tokio::fs::read(p).await.map(Response::new).map_err(|e| e.to_string())
}

fn new_profile(name: String, language: Language, mode: ProfileMode) -> VoiceProfile {
    VoiceProfile {
        id: String::new(),
        name,
        language,
        mode,
        description: String::new(),
        reference_wav: REFERENCE_FILE.into(),
        reference_transcript: String::new(),
        style_hint: String::new(),
        cfg_value: DEFAULT_CFG,
        inference_timesteps: DEFAULT_STEPS,
        created_at: now(),
        preview_wav: None,
        consent_at: None,
    }
}

/// Makes the chosen candidate the new profile's reference clip (its transcript is the test
/// sentence, so the profile clones in "ultimate" mode).
#[tauri::command]
pub async fn voice_profile_create_design(
    state: State<'_, AppState>,
    name: String,
    language: Language,
    description: String,
    index: usize,
) -> CmdResult<VoiceProfile> {
    let reference = std::fs::read(design_sample(&state, index)?).map_err(|e| e.to_string())?;
    let mut p = new_profile(name, language, ProfileMode::Design);
    p.description = clean_description(&description);
    p.reference_transcript = design_text(language).into();
    let p = store(&state).create(p, &reference).map_err(err)?;
    let _ = std::fs::remove_dir_all(state.paths.voices_dir().join(DESIGN_DIR));
    remember_default(&state, &p.id, false).await?;
    Ok(p)
}

fn is_audio_file(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| ["wav", "mp3", "m4a", "flac", "ogg", "opus"].contains(&e.to_ascii_lowercase().as_str()))
}

/// "From my recording": converts a 5–30 s clip to 16 kHz mono WAV and trims its silence.
/// Only the user's own voice or one they may use (`consent` must be confirmed in the UI).
#[tauri::command]
pub async fn voice_profile_import(
    state: State<'_, AppState>,
    path: String,
    name: String,
    language: Language,
    transcript: String,
    consent: bool,
) -> CmdResult<VoiceProfile> {
    if !consent {
        return Err("Confirm that this is your own voice or a voice you have permission to use.".into());
    }
    let src = PathBuf::from(&path);
    if !src.is_file() || !is_audio_file(&src) {
        return Err("Choose a WAV, MP3, M4A, FLAC or OGG recording.".into());
    }
    let ffmpeg = Ffmpeg::locate().map_err(|e| e.to_string())?;
    let work = state.paths.voices_dir().join(IMPORT_DIR);
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let out = work.join("clip.wav");
    let args: Vec<String> = ["-y", "-i", &path, "-t", "60", "-vn", "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le"]
        .iter()
        .map(|s| (*s).to_owned())
        .chain([out.display().to_string()])
        .collect();
    let converted = ffmpeg.run(&args, &work, &CancellationToken::new(), |_| {}).await;
    let bytes = std::fs::read(&out);
    let _ = std::fs::remove_dir_all(&work);
    converted.map_err(|e| format!("Could not read the recording: {e}"))?;
    let pcm = wav::trim(&wav::parse(&bytes.map_err(|e| e.to_string())?)?, 150);
    let ms = wav::ms(&pcm);
    if !(MIN_REF_MS..=MAX_REF_MS).contains(&ms) {
        return Err(format!("The recording has {:.1} s of speech; use a clip with 5–30 seconds of clear speech.", ms as f64 / 1000.0));
    }
    let transcript = transcript.trim().to_owned();
    let mode = if transcript.is_empty() { ProfileMode::Clone } else { ProfileMode::Ultimate };
    let mut p = new_profile(name, language, mode);
    p.reference_transcript = transcript;
    p.consent_at = Some(now());
    let p = store(&state).create(p, &wav::to_wav(&pcm)).map_err(err)?;
    remember_default(&state, &p.id, false).await?;
    Ok(p)
}

/// Rename / style / quality; the preview is made again on the next listen.
#[tauri::command]
pub async fn voice_profile_update(
    state: State<'_, AppState>,
    id: String,
    name: String,
    style_hint: String,
    cfg_value: f32,
    inference_timesteps: u32,
) -> CmdResult<VoiceProfile> {
    let store = store(&state);
    let p = VoiceProfile { name, style_hint, cfg_value, inference_timesteps, ..store.get(&id).map_err(err)? };
    let mut p = p.validated().map_err(err)?;
    if let Some(prev) = p.preview_wav.take() {
        let _ = std::fs::remove_file(store.dir(&id).map_err(err)?.join(prev));
    }
    store.save(&p).map_err(err)?;
    Ok(p)
}

#[tauri::command]
pub async fn voice_profile_duplicate(state: State<'_, AppState>, id: String) -> CmdResult<VoiceProfile> {
    store(&state).duplicate(&id, &now()).map_err(err)
}

#[tauri::command]
pub async fn voice_profile_delete(state: State<'_, AppState>, id: String) -> CmdResult<Settings> {
    store(&state).delete(&id).map_err(err)?;
    save_settings(&state, |s| {
        if s.voice.voxcpm_profile == id {
            s.voice.voxcpm_profile.clear();
        }
    })
    .await
}

/// Speaks the preview sentence with the profile (cached as `preview.wav`).
#[tauri::command]
pub async fn voice_profile_preview(state: State<'_, AppState>, id: String) -> CmdResult<Response> {
    let store = store(&state);
    let mut p = store.get(&id).map_err(err)?;
    let dir = store.dir(&id).map_err(err)?;
    if let Some(prev) = &p.preview_wav
        && let Ok(bytes) = std::fs::read(dir.join(prev))
    {
        return Ok(Response::new(bytes));
    }
    let text = match p.language {
        Language::Km => PREVIEW_KM,
        Language::En => PREVIEW_EN,
    };
    let tts = VoxCpmTts::new(speaker(&state).await?, resolved(&dir, &p), p.language, SEED);
    let (pcm, _) = tts.synth(text, &CancellationToken::new()).await.map_err(|e| e.detail())?;
    let bytes = wav::to_wav(&pcm);
    std::fs::write(dir.join(PREVIEW_FILE), &bytes).map_err(|e| e.to_string())?;
    p.preview_wav = Some(PREVIEW_FILE.into());
    store.save(&p).map_err(err)?;
    Ok(Response::new(bytes))
}

#[tauri::command]
pub async fn voice_profile_reference_audio(state: State<'_, AppState>, id: String) -> CmdResult<Response> {
    let store = store(&state);
    let p = store.get(&id).map_err(err)?;
    let bytes = std::fs::read(store.dir(&id).map_err(err)?.join(&p.reference_wav)).map_err(|e| e.to_string())?;
    Ok(Response::new(bytes))
}

/// Remembers `id` as the default profile; with `force` = false only when none is set yet.
async fn remember_default(state: &AppState, id: &str, force: bool) -> CmdResult<Settings> {
    save_settings(state, |s| {
        if force || s.voice.voxcpm_profile.is_empty() {
            s.voice.voxcpm_profile = id.to_owned();
        }
    })
    .await
}

#[tauri::command]
pub async fn voice_profile_set_default(state: State<'_, AppState>, id: String) -> CmdResult<Settings> {
    store(&state).get(&id).map_err(err)?;
    remember_default(&state, &id, true).await
}

/// The project's voice (`None` = the default); also becomes the default for new projects.
#[tauri::command]
pub async fn project_set_voice_profile(
    state: State<'_, AppState>,
    project_id: String,
    profile_id: Option<String>,
) -> CmdResult<Project> {
    if let Some(id) = &profile_id {
        store(&state).get(id).map_err(err)?;
        remember_default(&state, id, true).await?;
    }
    state
        .store
        .update(&project_id, |p| {
            p.voice_profile = profile_id;
            Ok(())
        })
        .await
        .map(|(p, ())| p)
        .map_err(to_cmd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptions_cannot_break_the_style_prefix() {
        assert_eq!(clean_description("  warm (female)\n narrator "), "warm female narrator");
        assert_eq!(clean_description(&"a".repeat(400)).len(), 300);
    }

    #[test]
    fn only_audio_files_are_imported() {
        assert!(is_audio_file(Path::new("C:/me/voice.MP3")));
        assert!(!is_audio_file(Path::new("C:/me/voice.exe")));
        assert!(!is_audio_file(Path::new("C:/me/voice")));
    }
}
