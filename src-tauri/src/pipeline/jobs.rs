//! Running background jobs per project (script generation, voice generation).
//! At most one job of a kind per project; each can be cancelled.

use std::collections::HashMap;
use std::sync::Mutex;

use tokio_util::sync::CancellationToken;

use super::ProjectError;

#[derive(Default)]
pub struct JobRegistry {
    running: Mutex<HashMap<String, CancellationToken>>,
}

impl JobRegistry {
    pub fn start(&self, project_id: &str) -> Result<CancellationToken, ProjectError> {
        let mut map = self.running.lock().unwrap_or_else(|p| p.into_inner());
        if map.contains_key(project_id) {
            return Err(ProjectError::Busy);
        }
        let token = CancellationToken::new();
        map.insert(project_id.to_owned(), token.clone());
        Ok(token)
    }

    pub fn finish(&self, project_id: &str) {
        let mut map = self.running.lock().unwrap_or_else(|p| p.into_inner());
        map.remove(project_id);
    }

    pub fn cancel(&self, project_id: &str) {
        let map = self.running.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(t) = map.get(project_id) {
            t.cancel();
        }
    }

    /// Keys of the jobs running now.
    pub fn running_keys(&self) -> Vec<String> {
        let map = self.running.lock().unwrap_or_else(|p| p.into_inner());
        map.keys().cloned().collect()
    }

    pub fn is_running(&self, project_id: &str) -> bool {
        let map = self.running.lock().unwrap_or_else(|p| p.into_inner());
        map.contains_key(project_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jobs_are_exclusive_and_cancellable() {
        let jobs = JobRegistry::default();
        let t = jobs.start("a").unwrap();
        assert!(jobs.start("a").is_err());
        assert!(jobs.start("b").is_ok());
        jobs.cancel("a");
        assert!(t.is_cancelled());
        jobs.finish("a");
        assert!(!jobs.is_running("a"));
    }
}
