//! Loads and saves `project.json`. Every write is atomic and every read-modify-write
//! holds a per-project lock, so concurrent jobs and UI edits can't clobber each other.

use std::collections::HashMap;
use std::fs;
use std::sync::{Arc, Mutex};

use super::ProjectError;
use super::project::{Project, ProjectSummary};
use crate::storage::paths::is_valid_id;
use crate::storage::{AppPaths, StorageError, atomic};

pub struct ProjectStore {
    paths: AppPaths,
    locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

impl ProjectStore {
    pub fn new(paths: AppPaths) -> Self {
        Self {
            paths,
            locks: Mutex::new(HashMap::new()),
        }
    }

    fn check_id(id: &str) -> Result<(), ProjectError> {
        if is_valid_id(id) {
            Ok(())
        } else {
            Err(ProjectError::InvalidId)
        }
    }

    fn lock_for(&self, id: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut map = self
            .locks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        map.entry(id.to_owned()).or_default().clone()
    }

    fn read(&self, id: &str) -> Result<Project, ProjectError> {
        atomic::read_json::<Project>(&self.paths.project_file(id))?.ok_or(ProjectError::NotFound)
    }

    fn write(&self, project: &Project) -> Result<(), ProjectError> {
        atomic::write_json(&self.paths.project_file(&project.id), project)?;
        Ok(())
    }

    /// Saves a brand-new project.
    pub async fn insert(&self, project: &Project) -> Result<(), ProjectError> {
        Self::check_id(&project.id)?;
        let lock = self.lock_for(&project.id);
        let _guard = lock.lock().await;
        self.write(project)
    }

    pub async fn get(&self, id: &str) -> Result<Project, ProjectError> {
        Self::check_id(id)?;
        let lock = self.lock_for(id);
        let _guard = lock.lock().await;
        self.read(id)
    }

    /// Load → mutate → stamp `updated_at` → atomic save, under the project lock.
    /// If `f` fails nothing is written.
    pub async fn update<R>(
        &self,
        id: &str,
        f: impl FnOnce(&mut Project) -> Result<R, ProjectError>,
    ) -> Result<(Project, R), ProjectError> {
        Self::check_id(id)?;
        let lock = self.lock_for(id);
        let _guard = lock.lock().await;
        let mut project = self.read(id)?;
        let out = f(&mut project)?;
        project.updated_at = now();
        self.write(&project)?;
        Ok((project, out))
    }

    pub async fn delete(&self, id: &str) -> Result<(), ProjectError> {
        Self::check_id(id)?;
        let lock = self.lock_for(id);
        let _guard = lock.lock().await;
        let dir = self.paths.project_dir(id);
        match fs::remove_dir_all(&dir) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(ProjectError::NotFound),
            Err(e) => Err(StorageError::io(&dir, e).into()),
        }
    }

    /// All readable projects, newest first. Broken folders are skipped, not fatal.
    pub fn list(&self) -> Result<Vec<ProjectSummary>, ProjectError> {
        let dir = self.paths.projects_dir();
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(StorageError::io(&dir, e).into()),
        };
        let mut out: Vec<ProjectSummary> = entries
            .filter_map(Result::ok)
            .filter_map(|e| e.file_name().to_str().map(str::to_owned))
            .filter(|name| is_valid_id(name))
            .filter_map(|id| self.read(&id).ok())
            .map(|p| ProjectSummary::from(&p))
            .collect();
        out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(out)
    }

    /// Startup recovery: assets left `running` by a crash go back to `pending`.
    pub fn recover_interrupted(&self) -> Result<(), ProjectError> {
        for summary in self.list()? {
            let Ok(mut p) = self.read(&summary.id) else {
                continue;
            };
            if p.reset_running_assets() {
                self.write(&p)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::project::tests::sample;

    fn store() -> (tempfile::TempDir, ProjectStore) {
        let dir = tempfile::tempdir().unwrap();
        let s = ProjectStore::new(AppPaths::new(dir.path().to_path_buf()));
        (dir, s)
    }

    #[tokio::test]
    async fn insert_update_list_delete() {
        let (_d, s) = store();
        let p = sample();
        s.insert(&p).await.unwrap();
        let (updated, ()) = s
            .update(&p.id, |p| {
                p.title = "new".into();
                Ok(())
            })
            .await
            .unwrap();
        assert_eq!(updated.title, "new");
        assert_eq!(s.get(&p.id).await.unwrap().title, "new");
        assert_eq!(s.list().unwrap().len(), 1);
        s.delete(&p.id).await.unwrap();
        assert!(matches!(s.get(&p.id).await, Err(ProjectError::NotFound)));
    }

    #[tokio::test]
    async fn failed_update_writes_nothing() {
        let (_d, s) = store();
        let p = sample();
        s.insert(&p).await.unwrap();
        let r: Result<(Project, ()), _> = s
            .update(&p.id, |p| {
                p.title = "changed".into();
                Err(ProjectError::Invalid("no".into()))
            })
            .await;
        assert!(r.is_err());
        assert_eq!(s.get(&p.id).await.unwrap().title, "t");
    }

    #[tokio::test]
    async fn rejects_traversal_ids() {
        let (_d, s) = store();
        assert!(matches!(s.get("..").await, Err(ProjectError::InvalidId)));
    }
}
