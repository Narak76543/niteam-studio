//! Stock media commands ("Real-world Tech Explainer", `docs/11-real-world-visuals.md`):
//! Pexels / Pixabay keys, key test, search, pick, scene visual edits, screenshot import,
//! credits, the "Find photos & clips" job.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::ipc::Response;
use tauri::{AppHandle, State};
use tokio_util::sync::CancellationToken;

use super::generate::TauriJobEvents;
use crate::error::{CmdResult, to_cmd};
use crate::gemini::script;
use crate::net::transport::{ReqwestTransport, Transport};
use crate::pipeline::ProjectError;
use crate::pipeline::events::JobEvents;
use crate::pipeline::hashing::normalize;
use crate::pipeline::project::{Aspect, AssetStatus, CodeSpec, DiagramSpec, Project, ProjectStatus, VisualKind};
use crate::pipeline::visual_job::{VisualJob, write_thumb};
use crate::render::clip::Frame;
use crate::state::AppState;
use crate::stock::assets::{self, AssetRecord, AssetRegistry, ReqwestDownloader};
use crate::stock::cache::SearchCache;
use crate::stock::credits;
use crate::stock::search::{Keys, Query, SearchOutcome, Searcher};
use crate::stock::{Candidate, Media, Source, StockError, pexels, pixabay, transport_error};
use crate::storage::log::LogFile;
use crate::storage::secrets::{self, StockKey};

/// Width of the picker's subject-point preview.
const PREVIEW_WIDTH: u32 = 1280;

/// Results shown per search in the picker.
pub const CANDIDATES_PER_SEARCH: usize = 6;

/// Sources that take a key ("pexels" / "pixabay").
fn keyed(source: &str) -> CmdResult<(Source, &'static str)> {
    let s = match source {
        "pexels" => Source::Pexels,
        "pixabay" => Source::Pixabay,
        _ => return Err(format!("unknown source '{source}'")),
    };
    s.key_user().map(|u| (s, u)).ok_or_else(|| "this source has no key".into())
}

#[tauri::command]
pub async fn stock_key_set(source: String, key: String) -> CmdResult<()> {
    let (_, user) = keyed(&source)?;
    let key = StockKey::parse(&key).map_err(to_cmd)?;
    secrets::set_stock_key(user, &key).map_err(to_cmd)
}

#[tauri::command]
pub async fn stock_key_exists(source: String) -> CmdResult<bool> {
    let (_, user) = keyed(&source)?;
    secrets::get_stock_key(user).map(|k| k.is_some()).map_err(to_cmd)
}

#[tauri::command]
pub async fn stock_key_clear(source: String) -> CmdResult<()> {
    let (_, user) = keyed(&source)?;
    secrets::clear_stock_key(user).map_err(to_cmd)
}

/// Failures are data (like the Gemini key test) so the UI can explain them.
#[derive(Debug, Serialize)]
pub struct StockTestOutcome {
    pub ok: bool,
    pub results: usize,
    pub error_kind: Option<String>,
    pub details: Option<String>,
}

/// One live photo search ("computer") with the stored key; no cache.
#[tauri::command]
pub async fn stock_key_test(source: String) -> CmdResult<StockTestOutcome> {
    let (source, user) = keyed(&source)?;
    let Some(key) = secrets::get_stock_key(user).map_err(to_cmd)? else {
        let e = StockError::NoKey(source);
        return Ok(StockTestOutcome { ok: false, results: 0, error_kind: Some(e.kind().into()), details: Some(e.to_string()) });
    };
    let transport = ReqwestTransport::new().map_err(|e| e.to_string())?;
    let run = async {
        let request = match source {
            Source::Pexels => pexels::request("computer", Media::Photo, 1, &key)?,
            _ => pixabay::request("computer", Media::Photo, 1, &key)?,
        };
        let resp = transport.send(request).await.map_err(|e| transport_error(source, e))?;
        match source {
            Source::Pexels => pexels::check(&resp).and_then(|()| pexels::parse(&resp.body, Media::Photo)),
            _ => pixabay::check(&resp).and_then(|()| pixabay::parse(&resp.body, Media::Photo)),
        }
    };
    Ok(match run.await {
        Ok(found) => StockTestOutcome { ok: true, results: found.len(), error_kind: None, details: None },
        Err(e) => StockTestOutcome { ok: false, results: 0, error_kind: Some(e.kind().into()), details: Some(e.to_string()) },
    })
}

/// A searcher with the stored keys and current settings; `only` narrows the sources (picker
/// filter).
pub async fn searcher(state: &AppState, only: Option<&[Source]>) -> CmdResult<Searcher> {
    let mut settings = state.settings.read().await.stock.clone();
    if let Some(only) = only {
        settings.pexels &= only.contains(&Source::Pexels);
        settings.pixabay &= only.contains(&Source::Pixabay);
        settings.openverse &= only.contains(&Source::Openverse);
        settings.commons &= only.contains(&Source::Wikimedia);
    }
    let key = |s: Source| s.key_user().and_then(|u| secrets::get_stock_key(u).ok().flatten());
    let keys = Keys { pexels: key(Source::Pexels), pixabay: key(Source::Pixabay) };
    let transport: Arc<dyn Transport> = Arc::new(ReqwestTransport::new().map_err(|e| e.to_string())?);
    let cache = SearchCache::new(state.paths.stock_cache_dir());
    // Drop expired searches once per app run.
    static PRUNED: std::sync::Once = std::sync::Once::new();
    PRUNED.call_once(|| cache.prune(chrono::Utc::now()));
    Ok(Searcher::new(transport, cache, keys, settings))
}

/// Picker search: `query` goes to the sources, `keywords` rank the results; the frame width
/// comes from the project's resolution (1080p when no project is given). `sources` narrows
/// the enabled sources.
#[tauri::command]
pub async fn stock_search(
    state: State<'_, AppState>,
    project_id: Option<String>,
    query: String,
    keywords: Vec<String>,
    media: Media,
    page: u32,
    sources: Option<Vec<Source>>,
) -> CmdResult<SearchOutcome> {
    if query.trim().is_empty() {
        return Err("Type what to search for.".into());
    }
    let (resolution, aspect) = match &project_id {
        Some(id) => {
            let p = state.store.get(id).await.map_err(to_cmd)?;
            (p.render.resolution.clone(), p.aspect)
        }
        None => ("1080p".into(), Aspect::default()),
    };
    let frame_width = Frame::for_project(aspect, &resolution).width;
    let searcher = searcher(&state, sources.as_deref()).await?;
    let keywords = if keywords.is_empty() { vec![query.clone()] } else { keywords };
    Ok(searcher
        .search(&Query { text: query, keywords, media, page: page.max(1), frame_width, limit: CANDIDATES_PER_SEARCH })
        .await)
}

/// Sets the scene's file + thumbnail after a pick or an import.
async fn use_asset(state: &AppState, project_id: &str, scene_id: &str, record: &AssetRecord, kind: VisualKind, candidate: Option<Candidate>) -> CmdResult<Project> {
    let project_frame = {
        let p = state.store.get(project_id).await.map_err(to_cmd)?;
        let f = Frame::for_project_settings(&p);
        (f.width, f.height)
    };
    let thumb = write_thumb(&state.paths.project_dir(project_id), scene_id, record, None, project_frame).await.ok();
    let (project, ()) = state
        .store
        .update(project_id, |p| {
            let scene = p.scenes.iter_mut().find(|s| s.id == scene_id).ok_or(ProjectError::SceneNotFound)?;
            let v = &mut scene.visual;
            v.kind = kind;
            v.asset = Some(record.id.clone());
            v.auto_picked = false;
            v.focus = None;
            v.status = AssetStatus::Done;
            v.error = None;
            v.error_kind = None;
            v.thumb_path = thumb;
            if let Some(c) = candidate
                && !v.candidates.iter().any(|x| x.asset_id() == record.id)
            {
                v.candidates.insert(0, c);
            }
            Ok(())
        })
        .await
        .map_err(to_cmd)?;
    Ok(project)
}

/// Picker "Use this": downloads the file once (licence recorded in `assets/assets.json`)
/// and makes it the scene's visual. Confirms an auto-pick.
#[tauri::command]
pub async fn stock_pick(state: State<'_, AppState>, project_id: String, scene_id: String, candidate: Candidate) -> CmdResult<Project> {
    if !candidate.license.allowed(state.settings.read().await.stock.allow_by_sa) {
        return Err("This licence is turned off in Settings.".into());
    }
    let assets_dir = state.paths.project_assets_dir(&project_id);
    let downloader = ReqwestDownloader::new().map_err(|e| e.to_string())?;
    let record = assets::ensure(&assets_dir, &candidate, &downloader, &CancellationToken::new()).await.map_err(|e| e.to_string())?;
    let kind = if candidate.media == Media::Video { VisualKind::StockVideo } else { VisualKind::RealPhoto };
    use_asset(&state, &project_id, &scene_id, &record, kind, Some(candidate)).await
}

/// The user's own screenshot or photo for a scene (consent checkbox in the UI).
#[tauri::command]
pub async fn scene_screenshot_import(state: State<'_, AppState>, project_id: String, scene_id: String, path: String, consent: bool) -> CmdResult<Project> {
    if !consent {
        return Err("Confirm that this is your own screenshot or that you may use it.".into());
    }
    let assets_dir = state.paths.project_assets_dir(&project_id);
    let record = assets::import_own(&assets_dir, std::path::Path::new(&path)).map_err(|e| e.to_string())?;
    use_asset(&state, &project_id, &scene_id, &record, VisualKind::Screenshot, None).await
}

/// Scene editor changes: visual type, keywords, diagram, code, subject point, "looks good".
#[derive(Debug, Deserialize)]
pub struct VisualEdit {
    pub kind: VisualKind,
    pub keywords: Vec<String>,
    pub diagram: Option<DiagramSpec>,
    pub code: Option<CodeSpec>,
    pub focus: Option<[f32; 2]>,
    /// Confirms an automatic pick.
    pub confirm: bool,
}

fn clean_keywords(keywords: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for k in keywords {
        let k = normalize(k).chars().take(60).collect::<String>();
        if !k.is_empty() && !out.iter().any(|x| x.eq_ignore_ascii_case(&k)) && out.len() < 4 {
            out.push(k);
        }
    }
    out
}

#[tauri::command]
pub async fn scene_visual_update(state: State<'_, AppState>, project_id: String, scene_id: String, edit: VisualEdit) -> CmdResult<Project> {
    if edit.kind == VisualKind::Diagram {
        let ok = edit.diagram.as_ref().is_some_and(|d| script::mermaid_template(&d.mermaid).is_some());
        if !ok {
            return Err("The diagram must start with flowchart LR, flowchart TD, sequenceDiagram or timeline.".into());
        }
    }
    if edit.kind == VisualKind::Code && edit.code.as_ref().is_none_or(|c| c.text.trim().is_empty()) {
        return Err("Type the code to show.".into());
    }
    let registry = AssetRegistry::load(&state.paths.project_assets_dir(&project_id)).unwrap_or_default();
    let focus = edit.focus.map(|[x, y]| [x.clamp(0.0, 1.0), y.clamp(0.0, 1.0)]);
    let (project, rethumb) = state
        .store
        .update(&project_id, |p| {
            let scene = p.scenes.iter_mut().find(|s| s.id == scene_id).ok_or(ProjectError::SceneNotFound)?;
            let v = &mut scene.visual;
            v.keywords = clean_keywords(&edit.keywords);
            if let Some(mut d) = edit.diagram.clone() {
                d.mermaid = d.mermaid.trim().to_owned();
                d.template = script::mermaid_template(&d.mermaid).unwrap_or("flow").to_owned();
                v.diagram = Some(d);
            }
            if let Some(c) = edit.code.clone() {
                v.code = Some(CodeSpec { text: c.text.trim_end().to_owned(), ..c });
            }
            let media = v.asset.as_deref().and_then(|id| registry.get(id)).map(|r| (r.media, r.source));
            v.kind = edit.kind;
            v.status = match edit.kind {
                VisualKind::RealPhoto => if matches!(media, Some((Media::Photo, s)) if s != Source::User) { AssetStatus::Done } else { AssetStatus::Pending },
                VisualKind::StockVideo => if matches!(media, Some((Media::Video, _))) { AssetStatus::Done } else { AssetStatus::Pending },
                VisualKind::Screenshot => if matches!(media, Some((_, Source::User))) { AssetStatus::Done } else { AssetStatus::Pending },
                // Drawn again when the text changed (render_hash differs).
                VisualKind::Diagram | VisualKind::Code => AssetStatus::Pending,
                VisualKind::AiImage => AssetStatus::Done,
            };
            if v.status == AssetStatus::Pending && v.uses_asset() {
                v.asset = None;
                v.thumb_path = None;
            }
            v.error = None;
            v.error_kind = None;
            let rethumb = focus != v.focus && v.asset.is_some();
            v.focus = focus;
            if edit.confirm {
                v.auto_picked = false;
            }
            Ok(rethumb.then(|| v.asset.clone()).flatten())
        })
        .await
        .map_err(to_cmd)?;
    let Some(asset) = rethumb else { return Ok(project) };
    let Some(record) = registry.get(&asset) else { return Ok(project) };
    let f = Frame::for_project_settings(&project);
    let thumb = write_thumb(&state.paths.project_dir(&project_id), &scene_id, record, focus, (f.width, f.height))
        .await
        .map_err(|e| format!("Could not update the thumbnail: {e}"))?;
    let (project, ()) = state
        .store
        .update(&project_id, |p| {
            if let Some(s) = p.scenes.iter_mut().find(|s| s.id == scene_id) {
                s.visual.thumb_path = Some(thumb);
            }
            Ok(())
        })
        .await
        .map_err(to_cmd)?;
    Ok(project)
}

/// The chosen photo (or a clip's poster), at most 1280 px wide, for setting the subject point.
#[tauri::command]
pub async fn scene_visual_preview(state: State<'_, AppState>, project_id: String, scene_id: String) -> CmdResult<Response> {
    let project = state.store.get(&project_id).await.map_err(to_cmd)?;
    let scene = project.scenes.iter().find(|s| s.id == scene_id).ok_or_else(|| to_cmd(ProjectError::SceneNotFound))?;
    let registry = AssetRegistry::load(&state.paths.project_assets_dir(&project_id)).map_err(to_cmd)?;
    let record = scene.visual.asset.as_deref().and_then(|id| registry.get(id)).ok_or("This scene has no photo or clip yet.")?;
    let rel = match record.media {
        Media::Photo => record.file.clone(),
        Media::Video => record.poster.clone().ok_or("This clip has no preview picture.")?,
    };
    let path = state.paths.project_dir(&project_id).join(rel);
    let bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, String> {
        let img = image::open(&path).map_err(|e| e.to_string())?;
        let img = if img.width() > PREVIEW_WIDTH { img.resize(PREVIEW_WIDTH, PREVIEW_WIDTH, image::imageops::FilterType::Triangle) } else { img };
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img.to_rgb8()).write_to(&mut out, image::ImageFormat::Jpeg).map_err(|e| e.to_string())?;
        Ok(out.into_inner())
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(Response::new(bytes))
}

/// Credits block for the video description, `None` when no stock file is used.
#[tauri::command]
pub async fn project_credits(state: State<'_, AppState>, project_id: String) -> CmdResult<Option<String>> {
    let project = state.store.get(&project_id).await.map_err(to_cmd)?;
    let registry = AssetRegistry::load(&state.paths.project_assets_dir(&project_id)).map_err(to_cmd)?;
    Ok(credits::build(&project, &registry))
}

// ---------- "Find photos & clips" job ----------

#[tauri::command]
pub async fn visuals_start(app: AppHandle, state: State<'_, AppState>, project_id: String, scene_id: Option<String>) -> CmdResult<()> {
    let project = state.store.get(&project_id).await.map_err(to_cmd)?;
    if project.status == ProjectStatus::ScriptDraft {
        return Err("Approve the script before finding photos.".into());
    }
    let searcher = searcher(&state, None).await?;
    let downloader: Arc<dyn assets::Downloader> = Arc::new(ReqwestDownloader::new().map_err(|e| e.to_string())?);
    let cancel = state.visual_jobs.start(&project_id).map_err(to_cmd)?;
    let job = VisualJob {
        store: state.store.clone(),
        paths: state.paths.clone(),
        searcher,
        downloader,
        log: LogFile::new(state.paths.project_log(&project_id)),
        cancel,
    };
    let jobs = state.visual_jobs.clone();
    let events: Arc<dyn JobEvents> = Arc::new(TauriJobEvents(app));
    tauri::async_runtime::spawn(async move {
        job.run(&project_id, scene_id.map(|s| vec![s]), events).await;
        jobs.finish(&project_id);
    });
    Ok(())
}

#[tauri::command]
pub fn visuals_cancel(state: State<'_, AppState>, project_id: String) {
    state.visual_jobs.cancel(&project_id);
}

#[tauri::command]
pub fn visuals_running(state: State<'_, AppState>, project_id: String) -> bool {
    state.visual_jobs.is_running(&project_id)
}
