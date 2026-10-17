//! Drawn visuals (diagram, code) of real-world scenes: what to draw, saving the UI's PNG, a
//! failed drawing, and "Fix with Gemini" for a diagram (`docs/11-real-world-visuals.md` §10).

use base64::Engine as _;
use tauri::{AppHandle, State};
use tokio_util::sync::CancellationToken;

use super::retry_events::retry_hooks;
use crate::error::{CmdResult, to_cmd};
use crate::gemini::GeminiError;
use crate::gemini::client::ModelChain;
use crate::gemini::diagram_fix;
use crate::pipeline::ProjectError;
use crate::pipeline::drawn::{self, DrawTask};
use crate::pipeline::project::{AssetStatus, Project, VisualKind};
use crate::state::AppState;
use crate::storage::log::LogFile;
use crate::storage::secrets;

/// Diagram / code pictures that are missing or outdated.
#[tauri::command]
pub async fn visuals_draw_plan(state: State<'_, AppState>, project_id: String) -> CmdResult<Vec<DrawTask>> {
    let project = state.store.get(&project_id).await.map_err(to_cmd)?;
    Ok(drawn::plan(&project, &state.paths.project_dir(&project_id)))
}

/// The UI's frame-size PNG for a diagram / code scene (`hash` from `visuals_draw_plan`), plus
/// one picture per revealed line for typed-out code.
#[tauri::command]
pub async fn visual_draw_save(
    state: State<'_, AppState>,
    project_id: String,
    scene_id: String,
    hash: String,
    png_base64: String,
    frames_base64: Vec<String>,
) -> CmdResult<Project> {
    let decode = |b: &str| base64::engine::general_purpose::STANDARD.decode(b.as_bytes()).map_err(|e| format!("Invalid picture data: {e}"));
    let png = decode(&png_base64)?;
    let frames = frames_base64.iter().map(|f| decode(f)).collect::<Result<Vec<_>, _>>()?;
    let dir = state.paths.project_dir(&project_id);
    let project = state.store.get(&project_id).await.map_err(to_cmd)?;
    let saved = tokio::task::spawn_blocking({
        let (p, d, id, h) = (project, dir, scene_id.clone(), hash.clone());
        move || drawn::save(&p, &d, &id, &h, &png, &frames).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    let (project, ()) = state
        .store
        .update(&project_id, |p| {
            let s = p.scenes.iter_mut().find(|s| s.id == scene_id).ok_or(ProjectError::SceneNotFound)?;
            let v = &mut s.visual;
            v.render_path = Some(saved.picture);
            v.thumb_path = Some(saved.thumb);
            v.frames = saved.frames;
            v.render_hash = Some(hash);
            v.status = AssetStatus::Done;
            v.error = None;
            v.error_kind = None;
            Ok(())
        })
        .await
        .map_err(to_cmd)?;
    Ok(project)
}

/// A drawing failed (e.g. Mermaid didn't parse): shown on the scene until its text changes.
#[tauri::command]
pub async fn visual_draw_failed(state: State<'_, AppState>, project_id: String, scene_id: String, hash: String, error: String) -> CmdResult<Project> {
    let (project, ()) = state
        .store
        .update(&project_id, |p| {
            let s = p.scenes.iter_mut().find(|s| s.id == scene_id).ok_or(ProjectError::SceneNotFound)?;
            let v = &mut s.visual;
            v.status = AssetStatus::Failed;
            v.render_hash = Some(hash);
            v.error = Some(error.chars().take(500).collect());
            v.error_kind = Some("diagram".into());
            Ok(())
        })
        .await
        .map_err(to_cmd)?;
    Ok(project)
}

/// "Fix with Gemini": one text request with the parser's error; the scene is redrawn after.
#[tauri::command]
pub async fn diagram_fix(app: AppHandle, state: State<'_, AppState>, project_id: String, scene_id: String, error: String) -> CmdResult<Project> {
    let key = secrets::get_api_key().map_err(to_cmd)?.ok_or_else(|| to_cmd(GeminiError::MissingKey))?;
    let project = state.store.get(&project_id).await.map_err(to_cmd)?;
    let scene = project.scenes.iter().find(|s| s.id == scene_id).ok_or_else(|| to_cmd(ProjectError::SceneNotFound))?;
    let mermaid = scene.visual.diagram.as_ref().map(|d| d.mermaid.clone()).ok_or("This scene has no diagram.")?;
    let log = LogFile::new(state.paths.project_log(&project_id)).redacting(key.expose());
    let hooks = retry_hooks(app, "script", Some(project_id.clone()), CancellationToken::new(), log.clone());
    let chain = ModelChain::new(&state.settings.read().await.models.text_chain);
    let fixed = diagram_fix::fix(&state.gemini, &key, &chain, &mermaid, &error, project.language, &hooks).await.map_err(to_cmd)?;
    log.info(&format!("scene {scene_id}: diagram fixed by Gemini"));
    let (project, ()) = state
        .store
        .update(&project_id, |p| {
            let s = p.scenes.iter_mut().find(|s| s.id == scene_id).ok_or(ProjectError::SceneNotFound)?;
            if let Some(d) = s.visual.diagram.as_mut() {
                d.mermaid = fixed;
                d.template = crate::gemini::script::mermaid_template(&d.mermaid).unwrap_or("flow").to_owned();
            }
            s.visual.kind = VisualKind::Diagram;
            s.visual.status = AssetStatus::Pending;
            s.visual.error = None;
            s.visual.error_kind = None;
            Ok(())
        })
        .await
        .map_err(to_cmd)?;
    Ok(project)
}
