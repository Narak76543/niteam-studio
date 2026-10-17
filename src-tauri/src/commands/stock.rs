//! Stock media commands ("Real-world Tech Explainer", `docs/11-real-world-visuals.md`):
//! Pexels / Pixabay keys, key test, search.

use std::sync::Arc;

use serde::Serialize;
use tauri::State;
use tokio_util::sync::CancellationToken;

use crate::error::{CmdResult, to_cmd};
use crate::net::transport::{ReqwestTransport, Transport};
use crate::pipeline::ProjectError;
use crate::pipeline::project::{AssetStatus, Project, VisualKind};
use crate::render::clip::Frame;
use crate::state::AppState;
use crate::stock::assets::{self, ReqwestDownloader};
use crate::stock::cache::SearchCache;
use crate::stock::search::{Keys, Query, SearchOutcome, Searcher};
use crate::stock::{Candidate, Media, Source, StockError, pexels, pixabay, transport_error};
use crate::storage::secrets::{self, StockKey};

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

/// A searcher with the stored keys and current settings.
pub async fn searcher(state: &AppState) -> CmdResult<Searcher> {
    let settings = state.settings.read().await.stock.clone();
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
/// comes from the project's resolution (1080p when no project is given).
#[tauri::command]
pub async fn stock_search(
    state: State<'_, AppState>,
    project_id: Option<String>,
    query: String,
    keywords: Vec<String>,
    media: Media,
    page: u32,
) -> CmdResult<SearchOutcome> {
    if query.trim().is_empty() {
        return Err("Type what to search for.".into());
    }
    let resolution = match &project_id {
        Some(id) => state.store.get(id).await.map_err(to_cmd)?.render.resolution,
        None => "1080p".into(),
    };
    let frame_width = Frame::for_resolution(&resolution).width;
    let searcher = searcher(&state).await?;
    let keywords = if keywords.is_empty() { vec![query.clone()] } else { keywords };
    Ok(searcher
        .search(&Query { text: query, keywords, media, page: page.max(1), frame_width, limit: CANDIDATES_PER_SEARCH })
        .await)
}

/// Picker "Use this": downloads the file once (license recorded in `assets/assets.json`)
/// and makes it the scene's visual. Confirms an auto-pick.
#[tauri::command]
pub async fn stock_pick(state: State<'_, AppState>, project_id: String, scene_id: String, candidate: Candidate) -> CmdResult<Project> {
    if !candidate.license.allowed(state.settings.read().await.stock.allow_by_sa) {
        return Err("This license is turned off in Settings.".into());
    }
    let assets_dir = state.paths.project_assets_dir(&project_id);
    let downloader = ReqwestDownloader::new().map_err(|e| e.to_string())?;
    let record = assets::ensure(&assets_dir, &candidate, &downloader, &CancellationToken::new()).await.map_err(|e| e.to_string())?;
    let (project, ()) = state
        .store
        .update(&project_id, |p| {
            let scene = p.scenes.iter_mut().find(|s| s.id == scene_id).ok_or(ProjectError::SceneNotFound)?;
            let v = &mut scene.visual;
            v.kind = if candidate.media == Media::Video { VisualKind::StockVideo } else { VisualKind::RealPhoto };
            v.asset = Some(record.id.clone());
            v.auto_picked = false;
            v.status = AssetStatus::Done;
            v.error = None;
            v.error_kind = None;
            if !v.candidates.iter().any(|c| c.asset_id() == record.id) {
                v.candidates.insert(0, candidate);
            }
            Ok(())
        })
        .await
        .map_err(to_cmd)?;
    Ok(project)
}
