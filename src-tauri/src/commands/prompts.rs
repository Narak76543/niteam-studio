//! Scenes page → "Improve image prompts": rewrites every scene's image prompt with the
//! current rules (narration unchanged); outdated images are made again by the image job.

use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};

use super::retry_events::retry_hooks;
use crate::error::{CmdResult, to_cmd};
use crate::gemini::GeminiError;
use crate::gemini::client::ModelChain;
use crate::pipeline::ProjectError;
use crate::pipeline::editing;
use crate::pipeline::project::ProjectStatus;
use crate::pipeline::prompt_rewrite::{RewriteEvents, RewriteJob, RewriteProgress};
use crate::state::AppState;
use crate::storage::log::LogFile;
use crate::storage::secrets;

const PROMPT_REWRITE_EVENT: &str = "prompt-rewrite-progress";

struct TauriRewriteEvents(AppHandle);

impl RewriteEvents for TauriRewriteEvents {
    fn progress(&self, p: RewriteProgress) {
        // Emitting only fails if the window is gone; nothing to do then.
        let _ = self.0.emit(PROMPT_REWRITE_EVENT, p);
    }
}

#[tauri::command]
pub async fn image_prompts_rewrite(app: AppHandle, state: State<'_, AppState>, project_id: String) -> CmdResult<()> {
    let key = secrets::get_api_key()
        .map_err(to_cmd)?
        .ok_or_else(|| to_cmd(GeminiError::MissingKey))?;
    let project = state.store.get(&project_id).await.map_err(to_cmd)?;
    if project.status == ProjectStatus::Rendering {
        return Err(to_cmd(ProjectError::WrongState(editing::status_name(project.status).into())));
    }
    if state.script_jobs.is_running(&project_id) || state.image_jobs.is_running(&project_id) {
        return Err(to_cmd(ProjectError::Busy));
    }
    let cancel = state.prompt_jobs.start(&project_id).map_err(to_cmd)?;
    let log = LogFile::new(state.paths.project_log(&project_id)).redacting(key.expose());
    let hooks = retry_hooks(app.clone(), "script", Some(project_id.clone()), cancel.clone(), log.clone());
    let job = RewriteJob {
        store: state.store.clone(),
        client: state.gemini.clone(),
        key,
        chain: ModelChain::new(&state.settings.read().await.models.text_chain),
        hooks,
        log,
    };
    let jobs = state.prompt_jobs.clone();
    let events: Arc<dyn RewriteEvents> = Arc::new(TauriRewriteEvents(app));
    tauri::async_runtime::spawn(async move {
        job.run(&project_id, cancel, events).await;
        jobs.finish(&project_id);
    });
    Ok(())
}

#[tauri::command]
pub fn image_prompts_rewrite_cancel(state: State<'_, AppState>, project_id: String) {
    state.prompt_jobs.cancel(&project_id);
}

#[tauri::command]
pub fn image_prompts_rewrite_running(state: State<'_, AppState>, project_id: String) -> bool {
    state.prompt_jobs.is_running(&project_id)
}
