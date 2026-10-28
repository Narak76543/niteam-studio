//! Image generation job (`docs/07-image-generation.md` §5, §6, §8).
//!
//! - Plans every scene (provider, prompt, seed, hash); up-to-date images are skipped.
//! - Key scenes first, then story order; per-provider concurrency (Cloudflare 2 by default).
//! - "2 scenes per image": the second scene of a pair reuses the first scene's image.
//! - Daily Cloudflare allowance used up → stop scheduling, keep finished images, set
//!   `paused_quota` with the reset time. Bad token / missing billing → stop with a clear error.
//! - `project.json` is saved after every image.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use super::ProjectError;
use super::events::{GenerateProgress, GenerateStage, JobEvents, JobKind, SceneProgress};
use super::project::{AssetStatus, ImagePause, ImageSnapshot, Project, ProjectStatus};
use super::store::ProjectStore;
use crate::gemini::retry::RetryHooks;
use crate::images::generate_with_retry;
use crate::images::plan::{ScenePlan, plan_all};
use crate::images::postprocess::{self, Processed};
use crate::images::provider::{Aspect, ImageError, ImageProvider, ImageRequest, ProviderId};
use crate::render::clip::Frame;
use crate::storage::log::LogFile;
use crate::storage::{AppPaths, StorageError, atomic};

/// Providers available to this job (built from Settings + keyring by the command).
#[derive(Clone, Default)]
pub struct Providers {
    pub cloudflare: Option<Arc<dyn ImageProvider>>,
    pub gemini: Option<Arc<dyn ImageProvider>>,
    pub local: Option<Arc<dyn ImageProvider>>,
}

impl Providers {
    fn get(&self, id: ProviderId) -> Option<&Arc<dyn ImageProvider>> {
        match id {
            ProviderId::CloudflareFlux => self.cloudflare.as_ref(),
            ProviderId::GeminiNanoBanana => self.gemini.as_ref(),
            ProviderId::LocalSdcpp => self.local.as_ref(),
        }
    }
}

pub struct ImageJob {
    pub store: Arc<ProjectStore>,
    pub paths: AppPaths,
    pub providers: Providers,
    /// From Settings → Images (`scenes_per_image` is kept from the project).
    pub settings: ImageSnapshot,
    pub hooks: RetryHooks,
    pub log: LogFile,
    pub cloudflare_concurrency: usize,
    pub gemini_concurrency: usize,
}

struct Task {
    plan: ScenePlan,
    /// Scenes that get this image: the source scene plus a paired scene, if any.
    scene_ids: Vec<String>,
}

enum Outcome {
    Done,
    Failed,
    Skipped,
}

struct Ctx {
    project_id: String,
    resolution: String,
    /// Output frame from the project's aspect (pan/thumb sizes derive from it).
    frame: (u32, u32),
    store: Arc<ProjectStore>,
    paths: AppPaths,
    providers: Providers,
    hooks: RetryHooks,
    log: LogFile,
    events: Arc<dyn JobEvents>,
    cloudflare_limit: Semaphore,
    gemini_limit: Semaphore,
    /// One local image at a time (08 §3).
    local_limit: Semaphore,
    /// Set on quota exhaustion or auth failure: remaining tasks are not started.
    stop: AtomicBool,
    pause_until: Mutex<Option<String>>,
    job_error: Mutex<Option<(String, String)>>,
}

impl ImageJob {
    /// Generates every image that needs it, or only the scenes in `only` (forced).
    pub async fn run(
        self,
        project_id: &str,
        only: Option<Vec<String>>,
        events: Arc<dyn JobEvents>,
    ) {
        let report = |stage,
                      done,
                      failed,
                      total,
                      error: Option<(String, String)>,
                      resets: Option<String>| {
            let (error, error_kind) = error.map_or((None, None), |(d, k)| (Some(d), Some(k)));
            events.progress(GenerateProgress {
                project_id: project_id.to_owned(),
                job: JobKind::Image,
                stage,
                done,
                failed,
                total,
                error,
                error_kind,
                resets_at_utc: resets,
            });
        };
        let prepared = self.prepare(project_id, only.as_deref()).await;
        let (resolution, frame, mut tasks) = match prepared {
            Ok(v) => v,
            Err(e) => {
                self.log
                    .error(&format!("image generation could not start: {e}"));
                report(
                    GenerateStage::Failed,
                    0,
                    0,
                    0,
                    Some((e.detail(), e.kind().to_owned())),
                    None,
                );
                return;
            }
        };
        // Key scenes first so the user sees the look early (07 §5); stable otherwise.
        tasks.sort_by_key(|t| !t.plan.key_scene);
        let total = tasks.len();
        let missing: Vec<&str> = tasks
            .iter()
            .filter(|t| self.providers.get(t.plan.provider).is_none())
            .map(|t| t.plan.provider.as_str())
            .collect();
        if let Some(p) = missing.first() {
            let msg = match *p {
                "cloudflare_flux" => {
                    "Add your Cloudflare account ID and API token in Settings → Images."
                }
                _ => "Add your Gemini API key in Settings.",
            };
            report(
                GenerateStage::Failed,
                0,
                0,
                total,
                Some((msg.to_owned(), "image_auth".to_owned())),
                None,
            );
            return;
        }
        self.log.info(&format!(
            "image generation started: {total} image(s), provider {}, steps {}",
            self.settings.provider.as_str(),
            self.settings.steps
        ));
        let ctx = Arc::new(Ctx {
            project_id: project_id.to_owned(),
            resolution,
            frame,
            store: self.store,
            paths: self.paths,
            providers: self.providers,
            hooks: self.hooks,
            log: self.log,
            events: events.clone(),
            cloudflare_limit: Semaphore::new(self.cloudflare_concurrency.max(1)),
            gemini_limit: Semaphore::new(self.gemini_concurrency.max(1)),
            local_limit: Semaphore::new(1),
            stop: AtomicBool::new(false),
            pause_until: Mutex::new(None),
            job_error: Mutex::new(None),
        });
        report(GenerateStage::Running, 0, 0, total, None, None);

        let mut set = JoinSet::new();
        for task in tasks {
            let ctx = ctx.clone();
            set.spawn(async move { ctx.run_task(task).await });
        }
        let (mut done, mut failed) = (0, 0);
        while let Some(result) = set.join_next().await {
            match result {
                Ok(Outcome::Done) => done += 1,
                Ok(Outcome::Failed) | Err(_) => failed += 1,
                Ok(Outcome::Skipped) => {}
            }
            report(GenerateStage::Running, done, failed, total, None, None);
        }

        let pause = ctx.pause_until.lock().map(|p| p.clone()).unwrap_or(None);
        let job_error = ctx.job_error.lock().map(|e| e.clone()).unwrap_or(None);
        let stage = if let Some(resets) = &pause {
            let pause = ImagePause {
                resets_at_utc: resets.clone(),
                done,
                total,
            };
            let _ = ctx
                .store
                .update(project_id, |p| {
                    p.status = ProjectStatus::PausedQuota;
                    p.image_pause = Some(pause);
                    Ok(())
                })
                .await;
            GenerateStage::PausedQuota
        } else if ctx.hooks.cancel.is_cancelled() {
            GenerateStage::Cancelled
        } else if job_error.is_some() {
            GenerateStage::Failed
        } else {
            let _ = ctx
                .store
                .update(project_id, |p| {
                    p.promote_to_review();
                    Ok(())
                })
                .await;
            GenerateStage::Done
        };
        ctx.log.info(&format!(
            "image generation {stage:?}: {done} done, {failed} failed, {} not run",
            total - done - failed
        ));
        report(stage, done, failed, total, job_error, pause);
    }

    /// Snapshots the settings, links shared scenes, marks changed images stale and lists the
    /// images to generate.
    async fn prepare(
        &self,
        project_id: &str,
        only: Option<&[String]>,
    ) -> Result<(String, (u32, u32), Vec<Task>), ProjectError> {
        let paths = self.paths.clone();
        let settings = self.settings.clone();
        let (project, tasks) = self
            .store
            .update(project_id, |p| {
                match p.status {
                    ProjectStatus::ScriptDraft => {
                        return Err(ProjectError::Invalid(
                            "Approve the script before generating images.".into(),
                        ));
                    }
                    ProjectStatus::Rendering => {
                        return Err(ProjectError::WrongState("rendering".into()));
                    }
                    ProjectStatus::PausedQuota => {
                        p.status = ProjectStatus::Generating;
                        p.image_pause = None;
                    }
                    _ => {}
                }
                let per_image = p.image_settings.scenes_per_image;
                p.image_settings = ImageSnapshot {
                    scenes_per_image: per_image,
                    ..settings
                };
                Ok(plan_tasks(p, &paths, only))
            })
            .await?;
        let frame = Frame::for_project_settings(&project);
        Ok((project.render.resolution.clone(), (frame.width, frame.height), tasks))
    }
}

fn file_exists(dir: &Path, rel: Option<&String>) -> bool {
    rel.is_some_and(|r| dir.join(r).is_file())
}

/// Decides what to generate. Mutates the project for stale marks and shared-image links.
fn plan_tasks(p: &mut Project, paths: &AppPaths, only: Option<&[String]>) -> Vec<Task> {
    let dir = paths.project_dir(&p.id);
    let plans = plan_all(p);
    let mut tasks: Vec<Task> = Vec::new();
    for plan in plans {
        let Some(idx) = p.scenes.iter().position(|s| s.id == plan.scene_id) else {
            continue;
        };
        let forced = only.is_some_and(|o| o.contains(&plan.scene_id));
        if only.is_some() && !forced && plan.shared_from.is_none() {
            continue;
        }
        if let Some(src_id) = &plan.shared_from {
            // Paired scene: reuse the source's image once it is up to date.
            let src = p
                .scenes
                .iter()
                .find(|s| &s.id == src_id)
                .map(|s| s.image.clone());
            let Some(scene) = p.scenes.get_mut(idx) else {
                continue;
            };
            match src {
                Some(img)
                    if img.status == AssetStatus::Done
                        && img.prompt_hash.as_deref() == Some(plan.hash.as_str()) =>
                {
                    scene.image = img;
                    scene.image.shared_from = Some(src_id.clone());
                }
                _ => {
                    if let Some(t) = tasks.iter_mut().find(|t| &t.plan.scene_id == src_id) {
                        t.scene_ids.push(scene.id.clone());
                    } else if scene.image.status == AssetStatus::Done {
                        scene.image.status = AssetStatus::Stale;
                    }
                }
            }
            continue;
        }
        let Some(scene) = p.scenes.get_mut(idx) else {
            continue;
        };
        let fresh = scene.image.status == AssetStatus::Done
            && scene.image.prompt_hash.as_deref() == Some(plan.hash.as_str())
            && scene.image.shared_from.is_none()
            && file_exists(&dir, scene.image.path.as_ref())
            && file_exists(&dir, scene.image.thumb_path.as_ref());
        if fresh && !forced {
            continue;
        }
        if scene.image.status == AssetStatus::Done {
            scene.image.status = AssetStatus::Stale;
        }
        tasks.push(Task {
            scene_ids: vec![scene.id.clone()],
            plan,
        });
    }
    tasks
}

impl Ctx {
    fn emit(&self, scene_id: &str, status: AssetStatus, error: Option<(String, String)>) {
        let (error, error_kind) = error.map_or((None, None), |(d, k)| (Some(d), Some(k)));
        self.events.scene(SceneProgress {
            project_id: self.project_id.clone(),
            scene_id: scene_id.to_owned(),
            asset: "image",
            status,
            duration_ms: None,
            error,
            error_kind,
        });
    }

    async fn set_status(
        &self,
        ids: &[String],
        status: AssetStatus,
        error: Option<(String, String)>,
    ) {
        let err = error.clone();
        let ok = self
            .store
            .update(&self.project_id, |p| {
                for s in p.scenes.iter_mut().filter(|s| ids.contains(&s.id)) {
                    s.image.status = status;
                    let (e, k) = err
                        .clone()
                        .map_or((None, None), |(d, k)| (Some(d), Some(k)));
                    s.image.error = e;
                    s.image.error_kind = k;
                }
                Ok(())
            })
            .await
            .is_ok();
        if ok {
            for id in ids {
                self.emit(id, status, error.clone());
            }
        }
    }

    /// Raw images of finished key scenes, as style references for providers that take them.
    async fn references(&self, exclude: &str) -> Vec<PathBuf> {
        let Ok(p) = self.store.get(&self.project_id).await else {
            return vec![];
        };
        let dir = self.paths.project_dir(&self.project_id);
        p.scenes
            .iter()
            .filter(|s| s.key_scene && s.id != exclude && s.image.status == AssetStatus::Done)
            .filter_map(|s| s.image.raw_path.as_ref().map(|r| dir.join(r)))
            .take(crate::images::gemini::MAX_REFERENCES)
            .collect()
    }

    async fn run_task(self: Arc<Self>, task: Task) -> Outcome {
        let Some(provider) = self.providers.get(task.plan.provider).cloned() else {
            return Outcome::Skipped;
        };
        let limit = match task.plan.provider {
            ProviderId::CloudflareFlux => &self.cloudflare_limit,
            ProviderId::GeminiNanoBanana => &self.gemini_limit,
            ProviderId::LocalSdcpp => &self.local_limit,
        };
        let Ok(_permit) = limit.acquire().await else {
            return Outcome::Skipped;
        };
        if self.hooks.cancel.is_cancelled() || self.stop.load(Ordering::Relaxed) {
            return Outcome::Skipped;
        }
        self.set_status(&task.scene_ids, AssetStatus::Running, None)
            .await;
        let reference_images = if provider.supports_reference_images() {
            self.references(&task.plan.scene_id).await
        } else {
            vec![]
        };
        let request = ImageRequest {
            prompt: task.plan.prompt.clone(),
            seed: task.plan.seed,
            steps: (task.plan.provider == ProviderId::CloudflareFlux).then_some(task.plan.steps),
            reference_images,
        };
        let result = generate_with_retry(provider.as_ref(), &request, &self.hooks).await;
        if self.hooks.cancel.is_cancelled() && result.is_err() {
            self.set_status(&task.scene_ids, AssetStatus::Pending, None)
                .await;
            return Outcome::Skipped;
        }
        match result {
            Ok(image) => match self
                .save(&task, provider.as_ref(), image.bytes, &image.mime)
                .await
            {
                Ok(()) => Outcome::Done,
                Err(e) => {
                    self.log.error(&format!(
                        "{} image could not be saved: {e}",
                        task.plan.scene_id
                    ));
                    self.set_status(
                        &task.scene_ids,
                        AssetStatus::Failed,
                        Some((e, "other".into())),
                    )
                    .await;
                    Outcome::Failed
                }
            },
            Err(ImageError::QuotaExhaustedDaily { resets_at_utc }) => {
                self.stop.store(true, Ordering::Relaxed);
                if let Ok(mut slot) = self.pause_until.lock() {
                    slot.get_or_insert_with(|| resets_at_utc.to_rfc3339());
                }
                self.log.warn(&format!(
                    "daily free image allowance used up; pausing until {resets_at_utc} (no account switching)"
                ));
                self.set_status(&task.scene_ids, AssetStatus::Pending, None)
                    .await;
                Outcome::Skipped
            }
            Err(e) => {
                let detail = match &e {
                    ImageError::SafetyBlocked(_) => format!(
                        "The image service refused this prompt — edit the image prompt and try again. ({e})"
                    ),
                    _ => e.to_string(),
                };
                let err = (detail, e.kind().to_owned());
                self.log.error(&format!(
                    "{} image failed [{}]: {}",
                    task.plan.scene_id, err.1, err.0
                ));
                if matches!(e, ImageError::Auth(_) | ImageError::Engine(_)) {
                    self.stop.store(true, Ordering::Relaxed);
                    if let Ok(mut slot) = self.job_error.lock() {
                        slot.get_or_insert_with(|| err.clone());
                    }
                }
                self.set_status(&task.scene_ids, AssetStatus::Failed, Some(err))
                    .await;
                Outcome::Failed
            }
        }
    }

    /// Post-processes off the async threads, writes the files and records the result.
    async fn save(
        &self,
        task: &Task,
        provider: &dyn ImageProvider,
        raw: Vec<u8>,
        mime: &str,
    ) -> Result<(), String> {
        let raw_for_cpu = raw.clone();
        let frame = self.frame;
        let processed: Processed =
            tokio::task::spawn_blocking(move || postprocess::process(&raw_for_cpu, frame))
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.to_string())?;
        let scene_id = &task.plan.scene_id;
        let rel_dir = format!("scenes/{scene_id}");
        let dir = self.paths.project_dir(&self.project_id).join(&rel_dir);
        remove_old_image_files(&dir);
        let raw_name = if mime.contains("png") {
            "image_raw.png"
        } else {
            "image_raw.jpg"
        };
        let pan_name = postprocess::pan_file_name(&self.resolution);
        let write = |name: &str, bytes: &[u8]| -> Result<(), StorageError> {
            atomic::write_bytes(&dir.join(name), bytes)
        };
        write(raw_name, &raw).map_err(|e| e.to_string())?;
        write(pan_name, &processed.pan_png).map_err(|e| e.to_string())?;
        write("thumb.jpg", &processed.thumb_jpg).map_err(|e| e.to_string())?;

        let plan = task.plan.clone();
        let ids = task.scene_ids.clone();
        let (raw_rel, pan_rel, thumb_rel) = (
            format!("{rel_dir}/{raw_name}"),
            format!("{rel_dir}/{pan_name}"),
            format!("{rel_dir}/thumb.jpg"),
        );
        self.store
            .update(&self.project_id, |p| {
                for s in p.scenes.iter_mut().filter(|s| ids.contains(&s.id)) {
                    let img = &mut s.image;
                    img.status = AssetStatus::Done;
                    img.path = Some(pan_rel.clone());
                    img.raw_path = Some(raw_rel.clone());
                    img.thumb_path = Some(thumb_rel.clone());
                    img.provider = Some(plan.provider.as_str().to_owned());
                    img.prompt_hash = Some(plan.hash.clone());
                    img.shared_from = (s.id != plan.scene_id).then(|| plan.scene_id.clone());
                    img.error = None;
                    img.error_kind = None;
                }
                Ok(())
            })
            .await
            .map_err(|e| e.to_string())?;
        self.log.info(&format!(
            "{scene_id} image ok ({} {}×{} {}, seed {}{})",
            provider.id(),
            processed.source.0,
            processed.source.1,
            match provider.native_aspect() {
                Aspect::Square => "→ 16:9 crop",
                Aspect::Wide16x9 => "16:9",
            },
            plan.seed,
            if task.scene_ids.len() > 1 {
                ", shared with the next scene"
            } else {
                ""
            }
        ));
        for id in &task.scene_ids {
            self.emit(id, AssetStatus::Done, None);
        }
        Ok(())
    }
}

fn remove_old_image_files(dir: &Path) {
    for name in [
        "image_raw.jpg",
        "image_raw.png",
        "image_1080.png",
        "image_720.png",
        "thumb.jpg",
    ] {
        // Missing files are fine; real problems surface when the new file is written.
        let _ = std::fs::remove_file(dir.join(name));
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::sync::Mutex as StdMutex;

    use async_trait::async_trait;
    use chrono::Utc;
    use image::{DynamicImage, ImageFormat, RgbImage};
    use tokio_util::sync::CancellationToken;

    use super::*;
    use crate::images::provider::{GeneratedImage, next_utc_midnight};
    use crate::pipeline::project::Scene;
    use crate::pipeline::project::tests::sample;

    /// Square JPEG provider; fails from call `quota_after` on with a daily-quota error.
    struct FakeFlux {
        calls: StdMutex<Vec<(String, u32)>>,
        quota_after: Option<usize>,
        fail: Option<fn() -> ImageError>,
    }

    #[async_trait]
    impl ImageProvider for FakeFlux {
        fn id(&self) -> &'static str {
            "cloudflare_flux"
        }
        fn supports_reference_images(&self) -> bool {
            false
        }
        fn native_aspect(&self) -> Aspect {
            Aspect::Square
        }
        fn model(&self) -> &str {
            "fake"
        }
        async fn generate(
            &self,
            req: &ImageRequest,
            _c: &CancellationToken,
        ) -> Result<GeneratedImage, ImageError> {
            let n = {
                let mut calls = self.calls.lock().unwrap();
                calls.push((req.prompt.clone(), req.seed));
                calls.len()
            };
            if let Some(f) = self.fail {
                return Err(f());
            }
            if self.quota_after.is_some_and(|q| n > q) {
                return Err(ImageError::QuotaExhaustedDaily {
                    resets_at_utc: next_utc_midnight(Utc::now()),
                });
            }
            let mut out = Cursor::new(Vec::new());
            DynamicImage::ImageRgb8(RgbImage::new(64, 64))
                .write_to(&mut out, ImageFormat::Jpeg)
                .unwrap();
            Ok(GeneratedImage {
                bytes: out.into_inner(),
                mime: "image/jpeg".into(),
                width: 64,
                height: 64,
            })
        }
    }

    fn fake(quota_after: Option<usize>, fail: Option<fn() -> ImageError>) -> Arc<FakeFlux> {
        Arc::new(FakeFlux {
            calls: StdMutex::new(vec![]),
            quota_after,
            fail,
        })
    }

    #[derive(Default)]
    struct Recorder {
        progress: StdMutex<Vec<GenerateProgress>>,
    }
    impl JobEvents for Recorder {
        fn scene(&self, _p: SceneProgress) {}
        fn progress(&self, p: GenerateProgress) {
            self.progress.lock().unwrap().push(p);
        }
    }

    async fn setup(n: usize) -> (tempfile::TempDir, Arc<ProjectStore>, AppPaths, String) {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(dir.path().to_path_buf());
        let store = Arc::new(ProjectStore::new(paths.clone()));
        let mut p = sample();
        p.status = ProjectStatus::Generating;
        p.render.resolution = "720p".into();
        for i in 0..n {
            let mut s = Scene::new(format!("s{:03}", i + 1), 0);
            s.image_prompt = format!("rice field {i}");
            p.scenes.push(s);
        }
        p.renumber();
        crate::pipeline::visual_bible::apply_key_scenes(&mut p);
        let id = p.id.clone();
        store.insert(&p).await.unwrap();
        (dir, store, paths, id)
    }

    fn job(store: &Arc<ProjectStore>, paths: &AppPaths, provider: Arc<FakeFlux>) -> ImageJob {
        ImageJob {
            store: store.clone(),
            paths: paths.clone(),
            providers: Providers {
                cloudflare: Some(provider),
                gemini: None,
                local: None,
            },
            settings: ImageSnapshot::default(),
            hooks: RetryHooks::default(),
            log: LogFile::new(paths.root().join("test.log")),
            cloudflare_concurrency: 1,
            gemini_concurrency: 1,
        }
    }

    #[tokio::test]
    async fn generates_key_scenes_first_and_writes_files() {
        let (_d, store, paths, id) = setup(4).await;
        let flux = fake(None, None);
        job(&store, &paths, flux.clone())
            .run(&id, None, Arc::new(Recorder::default()))
            .await;
        let p = store.get(&id).await.unwrap();
        for s in &p.scenes {
            assert_eq!(s.image.status, AssetStatus::Done, "{}", s.id);
            let dir = paths.project_dir(&id);
            assert!(dir.join(s.image.path.as_ref().unwrap()).is_file());
            assert!(s.image.path.as_ref().unwrap().ends_with("image_720.png"));
            assert!(dir.join(s.image.thumb_path.as_ref().unwrap()).is_file());
            assert!(dir.join(s.image.raw_path.as_ref().unwrap()).is_file());
        }
        // Key scenes (first and last) were generated before the others.
        let calls = flux.calls.lock().unwrap();
        let first_two: Vec<&str> = calls.iter().take(2).map(|c| c.0.as_str()).collect();
        assert!(first_two.iter().any(|c| c.contains("rice field 0")));
        assert!(first_two.iter().any(|c| c.contains("rice field 3")));
    }

    #[tokio::test]
    async fn cached_images_are_skipped_and_prompt_edits_regenerate_one() {
        let (_d, store, paths, id) = setup(3).await;
        job(&store, &paths, fake(None, None))
            .run(&id, None, Arc::new(Recorder::default()))
            .await;
        let again = fake(None, None);
        job(&store, &paths, again.clone())
            .run(&id, None, Arc::new(Recorder::default()))
            .await;
        assert!(again.calls.lock().unwrap().is_empty(), "everything cached");
        store
            .update(&id, |p| {
                crate::pipeline::editing::update_image_prompt(p, "s002", "a temple at dusk")
            })
            .await
            .unwrap();
        let third = fake(None, None);
        job(&store, &paths, third.clone())
            .run(&id, None, Arc::new(Recorder::default()))
            .await;
        assert_eq!(third.calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn quota_pauses_the_project_and_keeps_finished_images() {
        let (_d, store, paths, id) = setup(5).await;
        let rec = Arc::new(Recorder::default());
        job(&store, &paths, fake(Some(2), None))
            .run(&id, None, rec.clone())
            .await;
        let last = rec.progress.lock().unwrap().last().cloned().unwrap();
        assert_eq!(last.stage, GenerateStage::PausedQuota);
        assert!(
            last.resets_at_utc
                .as_deref()
                .is_some_and(|r| r.contains("T00:00:00"))
        );
        let p = store.get(&id).await.unwrap();
        assert_eq!(p.status, ProjectStatus::PausedQuota);
        let pause = p.image_pause.clone().unwrap();
        assert_eq!((pause.done, pause.total), (2, 5));
        let done = p
            .scenes
            .iter()
            .filter(|s| s.image.status == AssetStatus::Done)
            .count();
        assert_eq!(done, 2);
        assert!(
            p.scenes
                .iter()
                .all(|s| s.image.status != AssetStatus::Running)
        );

        // Resume: back to generating, only the missing three are made.
        let resume = fake(None, None);
        job(&store, &paths, resume.clone())
            .run(&id, None, Arc::new(Recorder::default()))
            .await;
        assert_eq!(resume.calls.lock().unwrap().len(), 3);
        let p = store.get(&id).await.unwrap();
        assert_ne!(p.status, ProjectStatus::PausedQuota);
        assert!(p.image_pause.is_none());
    }

    #[tokio::test]
    async fn auth_error_stops_with_a_job_error() {
        let (_d, store, paths, id) = setup(4).await;
        let flux = fake(None, Some(|| ImageError::Auth("bad token".into())));
        let rec = Arc::new(Recorder::default());
        job(&store, &paths, flux.clone())
            .run(&id, None, rec.clone())
            .await;
        assert_eq!(
            flux.calls.lock().unwrap().len(),
            1,
            "no more requests after an auth error"
        );
        let last = rec.progress.lock().unwrap().last().cloned().unwrap();
        assert_eq!(
            (last.stage, last.error_kind.as_deref()),
            (GenerateStage::Failed, Some("image_auth"))
        );
    }

    #[tokio::test]
    async fn two_scenes_per_image_share_files() {
        let (_d, store, paths, id) = setup(4).await;
        store
            .update(&id, |p| {
                crate::pipeline::editing::set_scenes_per_image(p, 2)
            })
            .await
            .unwrap();
        let flux = fake(None, None);
        job(&store, &paths, flux.clone())
            .run(&id, None, Arc::new(Recorder::default()))
            .await;
        assert_eq!(flux.calls.lock().unwrap().len(), 2);
        let p = store.get(&id).await.unwrap();
        assert_eq!(p.scenes[1].image.shared_from.as_deref(), Some("s001"));
        assert_eq!(p.scenes[1].image.path, p.scenes[0].image.path);
        assert_eq!(p.scenes[1].image.status, AssetStatus::Done);
    }

    #[tokio::test]
    async fn regenerate_uses_the_stored_new_seed() {
        let (_d, store, paths, id) = setup(2).await;
        job(&store, &paths, fake(None, None))
            .run(&id, None, Arc::new(Recorder::default()))
            .await;
        store
            .update(&id, |p| {
                p.scenes[1].image.seed = Some(4242);
                Ok(())
            })
            .await
            .unwrap();
        let flux = fake(None, None);
        job(&store, &paths, flux.clone())
            .run(
                &id,
                Some(vec!["s002".into()]),
                Arc::new(Recorder::default()),
            )
            .await;
        let calls = flux.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1, 4242);
    }
}
