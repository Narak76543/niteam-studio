//! Settings → Local images (`docs/08-local-gpu.md`): GPU info, model profiles (install /
//! verify / delete), benchmark, power hints, and the `local_sdcpp` provider for image jobs.

use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tokio_util::sync::CancellationToken;

use crate::error::{CmdResult, to_cmd};
use crate::images::prompt::{PromptParts, build_prompt};
use crate::images::provider::{ImageProvider, ImageRequest, ProviderId};
use crate::local::engine::{EngineConfig, device_for, list_devices};
use crate::local::gpu::GpuInfo;
use crate::local::install::{self, Power, ProfileStatus};
use crate::local::manifest::{self, Manifest, Package, Profile};
use crate::local::provider::{Cooldown, LocalSdCpp};
use crate::pipeline::presets;
use crate::pipeline::project::Shot;
use crate::state::AppState;
use crate::storage::log::LogFile;
use crate::storage::settings::{self, LocalPrefs, Settings};

const DOWNLOAD_EVENT: &str = "model-download-progress";
/// Install jobs are keyed by this (one download at a time).
pub const INSTALL_KEY: &str = "local-install";

#[derive(Debug, Serialize)]
pub struct LocalStatus {
    pub gpu: Option<GpuInfo>,
    /// GPU with ≥ 4 GB dedicated memory (08 §2).
    pub capable: bool,
    pub engine_version: String,
    pub engine_present: bool,
    pub profiles: Vec<ProfileStatus>,
    /// Profile being downloaded right now.
    pub installing: Option<String>,
    pub selected: Option<String>,
    pub prefs: LocalPrefs,
    pub power: Power,
}

fn manifest_profile(id: &str) -> CmdResult<(Manifest, Profile)> {
    let m = manifest::shipped();
    let p = m.profile(id).cloned().ok_or_else(|| "Unknown model profile.".to_owned())?;
    Ok((m, p))
}

#[tauri::command]
pub async fn local_status(state: State<'_, AppState>) -> CmdResult<LocalStatus> {
    let s = state.settings.read().await.clone();
    let m = manifest::shipped();
    let installing = state.install_jobs.running_keys().into_iter().next();
    Ok(LocalStatus {
        capable: s.gpu.as_ref().is_some_and(GpuInfo::local_capable),
        // A small variant is not listed on cards above its `max_vram_mb` (the full one is).
        profiles: install::status(
            m.profiles.iter().filter(|p| p.max_vram_mb.is_none_or(|max| s.gpu.as_ref().is_none_or(|g| g.vram_mb < max))),
            &state.paths.models_dir(),
            s.gpu.as_ref().map(|g| g.vram_mb),
        ),
        engine_version: m.engine_version,
        engine_present: state.paths.engine_exe().is_file(),
        gpu: s.gpu,
        installing,
        selected: s.image.local_profile,
        prefs: s.local,
        power: install::power(),
    })
}

#[derive(Debug, Clone, Serialize)]
struct DownloadProgress {
    profile_id: String,
    stage: &'static str,
    done_mb: u64,
    total_mb: u64,
    error: Option<String>,
}

pub async fn save_settings(state: &AppState, f: impl FnOnce(&mut Settings)) -> CmdResult<Settings> {
    let mut s = state.settings.write().await;
    f(&mut s);
    settings::save(&state.paths, &s).map_err(to_cmd)?;
    Ok(s.clone())
}

/// Downloads a profile in the background (`model-download-progress` events). The first
/// installed profile becomes the image provider (08 §9).
#[tauri::command]
pub async fn local_install(app: AppHandle, state: State<'_, AppState>, profile_id: String) -> CmdResult<()> {
    let (_, profile) = manifest_profile(&profile_id)?;
    spawn_install(app, &state, profile, |s, id| {
        if s.image.local_profile.is_none() {
            s.image.local_profile = Some(id.to_owned());
        }
        // 08 §9: GPU ≥ 4 GB and a profile installed → local by default.
        if s.image.provider == ProviderId::CloudflareFlux {
            s.image.provider = ProviderId::LocalSdcpp;
        }
        s.local.offer_dismissed = true;
    })
}

/// Downloads any package (image profile or voice model) in the background, one at a time;
/// `on_done` updates the settings after a complete, verified download.
pub fn spawn_install<P>(
    app: AppHandle,
    state: &AppState,
    profile: P,
    on_done: impl FnOnce(&mut Settings, &str) + Send + 'static,
) -> CmdResult<()>
where
    P: Package + Send + Sync + 'static,
{
    let cancel = state.install_jobs.start(INSTALL_KEY).map_err(to_cmd)?;
    let state_paths = state.paths.clone();
    let jobs = state.install_jobs.clone();
    let log = LogFile::new(state.paths.app_log());
    let client = crate::local::download::http_client().map_err(|e| e.to_string());
    let settings_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let emit = |stage, done: u64, total: u64, error: Option<String>| {
            let _ = app.emit(
                DOWNLOAD_EVENT,
                DownloadProgress {
                    profile_id: profile.id().to_owned(),
                    stage,
                    done_mb: done / (1024 * 1024),
                    total_mb: total / (1024 * 1024),
                    error,
                },
            );
        };
        let mut last = Instant::now();
        let result = match &client {
            Ok(c) => {
                install::install(c, &state_paths.models_dir(), &profile, &cancel, |done, total| {
                    if last.elapsed().as_millis() > 250 {
                        last = Instant::now();
                        emit("running", done, total, None);
                    }
                })
                .await
                .map_err(|e| e.to_string())
            }
            Err(e) => Err(e.clone()),
        };
        let total = profile.total_mb() * 1024 * 1024;
        let id = profile.id().to_owned();
        match result {
            Ok(()) => {
                log.info(&format!("local model '{id}' installed"));
                if let Some(state) = tauri::Manager::try_state::<AppState>(&settings_handle) {
                    let _ = save_settings(&state, |s| on_done(s, &id)).await;
                }
                emit("done", total, total, None);
            }
            Err(e) if cancel.is_cancelled() => {
                log.info(&format!("local model '{id}' download paused: {e}"));
                emit("cancelled", 0, total, None);
            }
            Err(e) => {
                log.error(&format!("local model '{id}' download failed: {e}"));
                emit("failed", 0, total, Some(e));
            }
        }
        jobs.finish(INSTALL_KEY);
    });
    Ok(())
}

#[tauri::command]
pub fn local_install_cancel(state: State<'_, AppState>) {
    state.install_jobs.cancel(INSTALL_KEY);
}

/// Re-checks the files' sha256; damaged files are deleted (shown as not installed).
#[tauri::command]
pub async fn local_verify(state: State<'_, AppState>, profile_id: String) -> CmdResult<bool> {
    let (_, profile) = manifest_profile(&profile_id)?;
    install::verify(&state.paths.models_dir(), &profile).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn local_delete(state: State<'_, AppState>, profile_id: String) -> CmdResult<Settings> {
    if state.install_jobs.is_running(INSTALL_KEY) {
        return Err("Wait until the download has finished or stop it first.".into());
    }
    let (_, profile) = manifest_profile(&profile_id)?;
    install::delete(&state.paths.models_dir(), &profile).map_err(|e| e.to_string())?;
    save_settings(&state, |s| {
        if s.image.local_profile.as_deref() == Some(profile_id.as_str()) {
            s.image.local_profile = None;
            if s.image.provider == ProviderId::LocalSdcpp {
                s.image.provider = ProviderId::CloudflareFlux;
            }
            if s.image.key_scene_provider == Some(ProviderId::LocalSdcpp) {
                s.image.key_scene_provider = None;
            }
        }
    })
    .await
}

/// "Use this model": selects an installed profile (files already on disk, or switching
/// between installed profiles) and, like a finished download, moves images off Cloudflare.
#[tauri::command]
pub async fn local_select(state: State<'_, AppState>, profile_id: String) -> CmdResult<Settings> {
    let (_, profile) = manifest_profile(&profile_id)?;
    if !install::installed(&state.paths.models_dir(), &profile) {
        return Err("Install this model first.".into());
    }
    save_settings(&state, |s| {
        s.image.local_profile = Some(profile_id);
        if s.image.provider == ProviderId::CloudflareFlux {
            s.image.provider = ProviderId::LocalSdcpp;
        }
        s.local.offer_dismissed = true;
    })
    .await
}

#[tauri::command]
pub async fn local_offer_dismiss(state: State<'_, AppState>) -> CmdResult<Settings> {
    save_settings(&state, |s| s.local.offer_dismissed = true).await
}

/// Builds the provider for image jobs, or `None` when no profile is installed / selected.
pub async fn local_provider(state: &AppState, settings: &Settings) -> CmdResult<Option<Arc<dyn ImageProvider>>> {
    let Some(id) = settings.image.local_profile.as_deref() else {
        return Ok(None);
    };
    let Some(profile) = manifest::shipped().profile(id).cloned() else {
        return Ok(None);
    };
    let models = state.paths.models_dir();
    if !install::installed(&models, &profile) {
        return Ok(None);
    }
    // One local model on the GPU at a time (09 §6): local voices first finish, then free VRAM.
    if !state.voice_jobs.running_keys().is_empty() && state.voice_server.is_running().await {
        return Err("Local voices are being made on the GPU. Wait until they finish, then make the images.".into());
    }
    state.voice_server.stop().await;
    let exe = state.paths.engine_exe();
    let device = match &settings.gpu {
        Some(g) => device_for(&list_devices(&exe).await.unwrap_or_default(), &g.name),
        None => None,
    };
    let dir = install::profile_dir(&models, &profile);
    let files = profile.loaded().map(|(flag, f)| (flag.to_owned(), dir.join(&f.name))).collect();
    let engine = EngineConfig { exe, files, args: profile.engine_args.clone(), lora_dir: dir, device };
    let cooldown = Cooldown { every: settings.local.cooldown_every, seconds: settings.local.cooldown_seconds };
    Ok(Some(Arc::new(LocalSdCpp::new(engine, profile, cooldown, state.ffmpeg_lock.clone()))))
}

#[derive(Debug, Serialize)]
pub struct Benchmark {
    pub seconds_per_image: f32,
    pub load_seconds: f32,
}

/// Settings → "Benchmark": loads the model and makes 2 test images (08 §8).
#[tauri::command]
pub async fn local_benchmark(state: State<'_, AppState>) -> CmdResult<Benchmark> {
    let settings = state.settings.read().await.clone();
    let Some(provider) = local_provider(&state, &settings).await? else {
        return Err("Install a local model first.".into());
    };
    let style = presets::find("stickman").map_or("", |p| p.style_prompt);
    let prompt = build_prompt(&PromptParts {
        style,
        shot: Shot::Medium,
        subject: "a stick figure waters a small plant on a table",
        characters: vec![],
        place: None,
        culture_notes: None,
        square: false,
    });
    let cancel = CancellationToken::new();
    let started = Instant::now();
    let mut times = Vec::new();
    for seed in [1, 2] {
        let t = Instant::now();
        provider
            .generate(&ImageRequest { prompt: prompt.clone(), seed, steps: None, reference_images: vec![] }, &cancel)
            .await
            .map_err(|e| e.to_string())?;
        times.push(t.elapsed().as_secs_f32());
    }
    // The first image includes loading the model.
    let per_image = times.last().copied().unwrap_or(0.0);
    let load = (started.elapsed().as_secs_f32() - times.iter().sum::<f32>()).max(0.0) + (times[0] - per_image).max(0.0);
    save_settings(&state, |s| s.local.seconds_per_image = Some(per_image)).await?;
    LogFile::new(state.paths.app_log()).info(&format!("local benchmark: {per_image:.2} s/image, load {load:.1} s"));
    Ok(Benchmark { seconds_per_image: per_image, load_seconds: load })
}
