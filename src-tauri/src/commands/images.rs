//! Image commands (Phase 3, `docs/07-image-generation.md`).

use std::sync::Arc;

use rand::Rng;
use serde::Serialize;
use tauri::ipc::Response;
use tauri::{AppHandle, State};
use tokio_util::sync::CancellationToken;

use super::generate::TauriJobEvents;
use super::retry_events::retry_hooks;
use crate::error::{CmdResult, to_cmd};
use crate::gemini::GeminiClient;
use crate::gemini::client::ModelChain;
use crate::gemini::retry::RetryHooks;
use crate::images::budget::{self, ImageEstimate};
use crate::images::cloudflare::CloudflareFlux;
use crate::images::gemini::GeminiNanoBanana;
use crate::images::plan::{plan_all, source_index};
use crate::images::postprocess;
use crate::images::provider::{ImageProvider, ImageRequest, ProviderId};
use crate::net::transport::ReqwestTransport;
use crate::pipeline::ProjectError;
use crate::pipeline::editing;
use crate::pipeline::events::JobEvents;
use crate::pipeline::image_job::{ImageJob, Providers};
use crate::pipeline::project::{AssetStatus, ImageSnapshot, Project, ProjectStatus, VisualBible};
use crate::pipeline::visual_bible;
use crate::state::AppState;
use crate::storage::log::LogFile;
use crate::storage::secrets::{self, CloudflareToken};
use crate::storage::settings::{ImageSettings, Settings};

// ---------- Cloudflare credentials ----------

#[tauri::command]
pub async fn cloudflare_token_set(token: String) -> CmdResult<()> {
    let token = CloudflareToken::parse(&token).map_err(to_cmd)?;
    secrets::set_cloudflare_token(&token).map_err(to_cmd)
}

#[tauri::command]
pub async fn cloudflare_token_exists() -> CmdResult<bool> {
    secrets::get_cloudflare_token()
        .map(|t| t.is_some())
        .map_err(to_cmd)
}

#[tauri::command]
pub async fn cloudflare_token_clear() -> CmdResult<()> {
    secrets::clear_cloudflare_token().map_err(to_cmd)
}

fn cloudflare_provider(settings: &ImageSettings) -> CmdResult<Option<Arc<dyn ImageProvider>>> {
    let Some(token) = secrets::get_cloudflare_token().map_err(to_cmd)? else {
        return Ok(None);
    };
    if settings.cloudflare_account_id.trim().is_empty() {
        return Ok(None);
    }
    let transport = ReqwestTransport::new().map_err(|e| e.to_string())?;
    Ok(Some(Arc::new(CloudflareFlux::new(
        Arc::new(transport),
        &settings.cloudflare_account_id,
        token,
    ))))
}

fn gemini_provider(
    client: &GeminiClient,
    settings: &ImageSettings,
) -> CmdResult<Option<Arc<dyn ImageProvider>>> {
    Ok(secrets::get_api_key().map_err(to_cmd)?.map(|key| {
        Arc::new(GeminiNanoBanana::new(
            client.clone(),
            key,
            &settings.gemini_model,
        )) as Arc<dyn ImageProvider>
    }))
}

#[derive(Debug, Serialize)]
pub struct ImageTestOutcome {
    pub ok: bool,
    pub error_kind: Option<String>,
    pub error: Option<String>,
}

/// "Test connection": one 1-step image of "a small green leaf" (07 §1a).
#[tauri::command]
pub async fn cloudflare_test(state: State<'_, AppState>) -> CmdResult<ImageTestOutcome> {
    let settings = state.settings.read().await.image.clone();
    let Some(provider) = cloudflare_provider(&settings)? else {
        return Ok(ImageTestOutcome {
            ok: false,
            error_kind: Some("image_auth".into()),
            error: Some("Add your Cloudflare account ID and API token first.".into()),
        });
    };
    let req = ImageRequest {
        prompt: "a small green leaf".into(),
        seed: 1,
        steps: Some(1),
        reference_images: vec![],
    };
    let log = LogFile::new(state.paths.app_log());
    match provider.generate(&req, &CancellationToken::new()).await {
        Ok(img) => {
            log.info(&format!(
                "Cloudflare test ok ({}×{})",
                img.width, img.height
            ));
            Ok(ImageTestOutcome {
                ok: true,
                error_kind: None,
                error: None,
            })
        }
        Err(e) => {
            log.error(&format!("Cloudflare test failed [{}]: {e}", e.kind()));
            Ok(ImageTestOutcome {
                ok: false,
                error_kind: Some(e.kind().into()),
                error: Some(e.to_string()),
            })
        }
    }
}

// ---------- estimate & job ----------

fn snapshot(settings: &ImageSettings) -> ImageSnapshot {
    ImageSnapshot {
        provider: settings.provider,
        gemini_model: settings.gemini_model.clone(),
        steps: settings.steps,
        key_scene_provider: settings.key_scene_provider,
        scenes_per_image: 1,
        local_profile: settings.local_profile.clone(),
    }
}

/// What the project would cost with the current Settings (nothing is changed).
#[tauri::command]
pub async fn images_estimate(
    state: State<'_, AppState>,
    project_id: String,
) -> CmdResult<ImageEstimate> {
    let settings = state.settings.read().await.image.clone();
    let mut project = state.store.get(&project_id).await.map_err(to_cmd)?;
    project.image_settings = ImageSnapshot {
        scenes_per_image: project.image_settings.scenes_per_image,
        ..snapshot(&settings)
    };
    let dir = state.paths.project_dir(&project_id);
    let plans = plan_all(&project);
    let mut cached = 0;
    let mut todo = Vec::new();
    for plan in &plans {
        let fresh = project
            .scenes
            .iter()
            .find(|s| s.id == plan.scene_id)
            .is_some_and(|s| {
                s.image.status == AssetStatus::Done
                    && s.image.prompt_hash.as_deref() == Some(plan.hash.as_str())
                    && s.image.path.as_ref().is_some_and(|p| dir.join(p).is_file())
            });
        if fresh && plan.shared_from.is_none() {
            cached += 1;
        } else if !fresh {
            todo.push(plan);
        }
    }
    Ok(budget::estimate(&todo, cached))
}

pub async fn start_image_job(
    app: AppHandle,
    state: &AppState,
    project_id: String,
    only: Option<Vec<String>>,
) -> CmdResult<()> {
    if state.script_jobs.is_running(&project_id) || state.prompt_jobs.is_running(&project_id) {
        return Err(to_cmd(ProjectError::Busy));
    }
    let settings: Settings = state.settings.read().await.clone();
    let project = state.store.get(&project_id).await.map_err(to_cmd)?;
    if project.status == ProjectStatus::ScriptDraft {
        return Err("Approve the script before generating images.".into());
    }
    // Projects scripted before Phase 3 have no Visual Bible yet: write one first.
    if project.visual_bible.is_none() {
        generate_bible(state, &project_id).await?;
    }
    let uses_local = settings.image.provider == ProviderId::LocalSdcpp
        || settings.image.key_scene_provider == Some(ProviderId::LocalSdcpp);
    let local = if uses_local { super::local::local_provider(state, &settings).await? } else { None };
    if uses_local && local.is_none() {
        return Err("Install a local model in Settings → Local images, or choose Cloudflare Flux in Settings → Images.".into());
    }
    let providers = Providers {
        cloudflare: cloudflare_provider(&settings.image)?,
        gemini: gemini_provider(&state.gemini, &settings.image)?,
        local,
    };
    let cancel = state.image_jobs.start(&project_id).map_err(to_cmd)?;
    let mut log = LogFile::new(state.paths.project_log(&project_id));
    if let Ok(Some(key)) = secrets::get_api_key() {
        log = log.redacting(key.expose());
    }
    if let Ok(Some(token)) = secrets::get_cloudflare_token() {
        log = log.redacting(token.expose());
    }
    let hooks = retry_hooks(
        app.clone(),
        "image",
        Some(project_id.clone()),
        cancel,
        log.clone(),
    );
    let job = ImageJob {
        store: state.store.clone(),
        paths: state.paths.clone(),
        providers,
        settings: snapshot(&settings.image),
        hooks,
        log,
        cloudflare_concurrency: settings.image.cloudflare_concurrency,
        gemini_concurrency: settings.concurrency.image,
    };
    let jobs = state.image_jobs.clone();
    let events: Arc<dyn JobEvents> = Arc::new(TauriJobEvents(app));
    tauri::async_runtime::spawn(async move {
        job.run(&project_id, only, events).await;
        jobs.finish(&project_id);
    });
    Ok(())
}

/// Generates missing / outdated images; also resumes a `paused_quota` project.
#[tauri::command]
pub async fn images_start(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
) -> CmdResult<()> {
    start_image_job(app, &state, project_id, None).await
}

#[tauri::command]
pub fn images_cancel(state: State<'_, AppState>, project_id: String) {
    state.image_jobs.cancel(&project_id);
}

#[tauri::command]
pub fn images_running(state: State<'_, AppState>, project_id: String) -> bool {
    state.image_jobs.is_running(&project_id)
}

/// "Regenerate image": new random seed stored on the scene that owns the image (07 §3).
pub async fn regenerate_image(
    app: AppHandle,
    state: &AppState,
    project_id: String,
    scene_id: String,
) -> CmdResult<()> {
    let seed: u32 = rand::thread_rng().gen_range(1..=999_999);
    let (_, source) = state
        .store
        .update(&project_id, |p| {
            let idx = p
                .scenes
                .iter()
                .position(|s| s.id == scene_id)
                .ok_or(ProjectError::SceneNotFound)?;
            let src = source_index(idx, p.image_settings.scenes_per_image);
            let scene = p.scenes.get_mut(src).ok_or(ProjectError::SceneNotFound)?;
            scene.image.seed = Some(seed);
            Ok(scene.id.clone())
        })
        .await
        .map_err(to_cmd)?;
    start_image_job(app, state, project_id, Some(vec![source])).await
}

/// `thumb.jpg` bytes (384×216) for the scene grid.
#[tauri::command]
pub async fn scene_image_thumb(
    state: State<'_, AppState>,
    project_id: String,
    scene_id: String,
) -> CmdResult<Response> {
    let project = state.store.get(&project_id).await.map_err(to_cmd)?;
    // Real-world scenes show their photo / clip / diagram thumbnail unless they fell back to
    // an AI image.
    let rel = project
        .scenes
        .iter()
        .find(|s| s.id == scene_id)
        .and_then(|s| if s.needs_ai_image() { s.image.thumb_path.clone() } else { s.visual.thumb_path.clone() })
        .ok_or_else(|| "This scene has no image yet.".to_owned())?;
    if !super::generate::is_safe_relative(&rel) {
        return Err("Invalid image path.".into());
    }
    let bytes = tokio::fs::read(state.paths.project_dir(&project_id).join(&rel))
        .await
        .map_err(|e| format!("Could not read the image: {e}"))?;
    Ok(Response::new(bytes))
}

// ---------- edits ----------

#[tauri::command]
pub async fn scene_image_prompt_update(
    state: State<'_, AppState>,
    project_id: String,
    scene_id: String,
    image_prompt: String,
) -> CmdResult<Project> {
    state
        .store
        .update(&project_id, |p| {
            editing::update_image_prompt(p, &scene_id, &image_prompt)
        })
        .await
        .map(|(p, ())| p)
        .map_err(to_cmd)
}

#[tauri::command]
pub async fn visual_bible_update(
    state: State<'_, AppState>,
    project_id: String,
    bible: VisualBible,
) -> CmdResult<Project> {
    state
        .store
        .update(&project_id, |p| editing::update_bible(p, &bible))
        .await
        .map(|(p, ())| p)
        .map_err(to_cmd)
}

async fn generate_bible(state: &AppState, project_id: &str) -> CmdResult<Project> {
    let key = secrets::get_api_key()
        .map_err(to_cmd)?
        .ok_or_else(|| to_cmd(crate::gemini::GeminiError::MissingKey))?;
    let project = state.store.get(project_id).await.map_err(to_cmd)?;
    let chain = ModelChain::new(&state.settings.read().await.models.text_chain);
    let (bible, model) = visual_bible::generate(
        &state.gemini,
        &key,
        &chain,
        &project,
        &RetryHooks::default(),
    )
    .await
    .map_err(to_cmd)?;
    LogFile::new(state.paths.project_log(project_id))
        .redacting(key.expose())
        .info(&format!("visual bible (re)written by {model}"));
    state
        .store
        .update(project_id, |p| editing::update_bible(p, &bible))
        .await
        .map(|(p, ())| p)
        .map_err(to_cmd)
}

/// "Regenerate look & feel" — asks the text model for a new Visual Bible.
#[tauri::command]
pub async fn visual_bible_generate(
    state: State<'_, AppState>,
    project_id: String,
) -> CmdResult<Project> {
    generate_bible(&state, &project_id).await
}

/// Look & feel → "Change style": switches the preset and rewrites the Visual Bible for it.
#[tauri::command]
pub async fn project_set_style(
    state: State<'_, AppState>,
    project_id: String,
    preset: String,
) -> CmdResult<Project> {
    state
        .store
        .update(&project_id, |p| editing::set_style(p, &preset))
        .await
        .map_err(to_cmd)?;
    generate_bible(&state, &project_id).await
}

#[tauri::command]
pub async fn project_set_scenes_per_image(
    state: State<'_, AppState>,
    project_id: String,
    n: u8,
) -> CmdResult<Project> {
    state
        .store
        .update(&project_id, |p| editing::set_scenes_per_image(p, n))
        .await
        .map(|(p, ())| p)
        .map_err(to_cmd)
}

/// Gemini image models + Flux license, for Settings.
#[derive(Debug, Serialize)]
pub struct ImageCatalog {
    pub gemini_models: Vec<&'static str>,
    pub flux_license_url: &'static str,
    pub pan_1080: (u32, u32),
}

#[tauri::command]
pub fn images_catalog() -> ImageCatalog {
    ImageCatalog {
        gemini_models: crate::images::gemini::MODELS.to_vec(),
        flux_license_url: "https://bfl.ai/legal/terms-of-service",
        pan_1080: postprocess::pan_size("1080p"),
    }
}
