//! Voice generation (Phase 2): one TTS request per scene, run in parallel up to a limit.
//!
//! Resumable: a scene whose voice is `done`, whose `text_hash` matches the current text +
//! voice settings and whose file still exists is skipped. Everything else is (re)generated.
//! `project.json` is saved after every scene.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use super::ProjectError;
use super::hashing::{normalize_spoken, text_hash};
use super::khmer_numbers;
use super::project::{AssetStatus, Language, ProjectStatus, VoiceSettings};
use super::store::ProjectStore;
use crate::gemini::retry::RetryHooks;
use crate::storage::log::LogFile;
use crate::storage::{AppPaths, StorageError, atomic};
use crate::tts::{EngineKind, TtsEngine, TtsError, TtsOutput, TtsRequest, audio};

/// Payload of `scene-progress`.
#[derive(Debug, Clone, Serialize)]
pub struct SceneProgress {
    pub project_id: String,
    pub scene_id: String,
    pub asset: &'static str,
    pub status: AssetStatus,
    pub duration_ms: Option<u64>,
    pub error: Option<String>,
    pub error_kind: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GenerateStage {
    Running,
    Done,
    Cancelled,
    Failed,
}

/// Payload of `generate-progress`.
#[derive(Debug, Clone, Serialize)]
pub struct GenerateProgress {
    pub project_id: String,
    pub stage: GenerateStage,
    pub done: usize,
    pub failed: usize,
    pub total: usize,
    /// Job-level error (e.g. Edge TTS unavailable): raw detail + stable kind.
    pub error: Option<String>,
    pub error_kind: Option<String>,
}

pub trait VoiceEvents: Send + Sync {
    fn scene(&self, progress: SceneProgress);
    fn progress(&self, progress: GenerateProgress);
}

struct Task {
    scene_id: String,
    text: String,
    hash: String,
}

enum Outcome {
    Done,
    Failed,
    Skipped,
}

/// Shared by all scene tasks of one job.
struct Ctx {
    project_id: String,
    language: Language,
    store: Arc<ProjectStore>,
    paths: AppPaths,
    engine: Arc<dyn TtsEngine>,
    voice: VoiceSettings,
    hooks: RetryHooks,
    log: LogFile,
    events: Arc<dyn VoiceEvents>,
    limit: Semaphore,
    /// Set after an engine-level failure (Edge unavailable): remaining scenes are skipped.
    engine_down: AtomicBool,
    engine_error: Mutex<Option<(String, String)>>,
}

pub struct VoiceJob {
    pub store: Arc<ProjectStore>,
    pub paths: AppPaths,
    pub engine: Arc<dyn TtsEngine>,
    pub voice: VoiceSettings,
    pub hooks: RetryHooks,
    pub log: LogFile,
    pub concurrency: usize,
}

impl VoiceJob {
    /// Generates voices for all scenes that need one, or only for `only` (forced).
    pub async fn run(
        self,
        project_id: &str,
        only: Option<Vec<String>>,
        events: Arc<dyn VoiceEvents>,
    ) {
        let prepared = self.prepare(project_id, only.as_deref()).await;
        let (language, tasks) = match prepared {
            Ok(v) => v,
            Err(e) => {
                self.log
                    .error(&format!("voice generation could not start: {e}"));
                events.progress(GenerateProgress {
                    project_id: project_id.to_owned(),
                    stage: GenerateStage::Failed,
                    done: 0,
                    failed: 0,
                    total: 0,
                    error: Some(e.detail()),
                    error_kind: Some(e.kind().to_owned()),
                });
                return;
            }
        };
        let total = tasks.len();
        self.log.info(&format!(
            "voice generation started: engine {}, voice {}, {total} scene(s)",
            self.voice.engine.as_str(),
            self.voice.voice_name
        ));
        let ctx = Arc::new(Ctx {
            project_id: project_id.to_owned(),
            language,
            store: self.store,
            paths: self.paths,
            engine: self.engine,
            voice: self.voice,
            hooks: self.hooks,
            log: self.log,
            events: events.clone(),
            limit: Semaphore::new(self.concurrency.max(1)),
            engine_down: AtomicBool::new(false),
            engine_error: Mutex::new(None),
        });
        let report = |stage, done, failed, error: Option<(String, String)>| {
            let (error, error_kind) = error.map_or((None, None), |(d, k)| (Some(d), Some(k)));
            events.progress(GenerateProgress {
                project_id: project_id.to_owned(),
                stage,
                done,
                failed,
                total,
                error,
                error_kind,
            });
        };
        report(GenerateStage::Running, 0, 0, None);

        let mut set = JoinSet::new();
        for task in tasks {
            let ctx = ctx.clone();
            set.spawn(async move { ctx.run_scene(task).await });
        }
        let (mut done, mut failed) = (0, 0);
        while let Some(result) = set.join_next().await {
            match result {
                Ok(Outcome::Done) => done += 1,
                Ok(Outcome::Failed) | Err(_) => failed += 1,
                Ok(Outcome::Skipped) => {}
            }
            report(GenerateStage::Running, done, failed, None);
        }

        let engine_error = ctx.engine_error.lock().map(|e| e.clone()).unwrap_or(None);
        let stage = if ctx.hooks.cancel.is_cancelled() {
            GenerateStage::Cancelled
        } else if engine_error.is_some() {
            GenerateStage::Failed
        } else {
            GenerateStage::Done
        };
        ctx.log.info(&format!(
            "voice generation {stage:?}: {done} done, {failed} failed, {} not run",
            total - done - failed
        ));
        report(stage, done, failed, engine_error);
    }

    /// Stores the voice settings on the project and lists the scenes to (re)generate.
    async fn prepare(
        &self,
        project_id: &str,
        only: Option<&[String]>,
    ) -> Result<(Language, Vec<Task>), ProjectError> {
        let paths = self.paths.clone();
        let voice = self.voice.clone();
        let (project, tasks) = self
            .store
            .update(project_id, |p| {
                if p.status == ProjectStatus::ScriptDraft {
                    return Err(ProjectError::Invalid(
                        "Approve the script before generating voices.".into(),
                    ));
                }
                p.voice = voice;
                let dir = paths.project_dir(&p.id);
                let mut tasks = Vec::new();
                for i in 0..p.scenes.len() {
                    let Some(scene) = p.scenes.get(i) else {
                        continue;
                    };
                    let hash = text_hash(p, scene);
                    let Some(scene) = p.scenes.get_mut(i) else {
                        continue;
                    };
                    let forced = only.is_some_and(|o| o.contains(&scene.id));
                    if only.is_some() && !forced {
                        continue;
                    }
                    let file_ok = scene
                        .voice
                        .path
                        .as_ref()
                        .is_some_and(|rel| dir.join(rel).is_file());
                    let fresh = scene.voice.status == AssetStatus::Done
                        && scene.voice.text_hash.as_deref() == Some(hash.as_str())
                        && file_ok;
                    if fresh && !forced {
                        continue;
                    }
                    if scene.voice.status == AssetStatus::Done {
                        scene.voice.status = AssetStatus::Stale;
                    }
                    tasks.push(Task {
                        scene_id: scene.id.clone(),
                        text: scene.spoken_text.clone(),
                        hash,
                    });
                }
                Ok(tasks)
            })
            .await?;
        Ok((project.language, tasks))
    }
}

impl Ctx {
    fn emit(
        &self,
        scene_id: &str,
        status: AssetStatus,
        duration_ms: Option<u64>,
        error: Option<(String, String)>,
    ) {
        let (error, error_kind) = error.map_or((None, None), |(d, k)| (Some(d), Some(k)));
        self.events.scene(SceneProgress {
            project_id: self.project_id.clone(),
            scene_id: scene_id.to_owned(),
            asset: "voice",
            status,
            duration_ms,
            error,
            error_kind,
        });
    }

    async fn set_status(
        &self,
        scene_id: &str,
        status: AssetStatus,
        error: Option<(String, String)>,
    ) {
        let err = error.clone();
        let result = self
            .store
            .update(&self.project_id, |p| {
                let s = p
                    .scenes
                    .iter_mut()
                    .find(|s| s.id == scene_id)
                    .ok_or(ProjectError::SceneNotFound)?;
                s.voice.status = status;
                let (e, k) = err.map_or((None, None), |(d, k)| (Some(d), Some(k)));
                s.voice.error = e;
                s.voice.error_kind = k;
                Ok(())
            })
            .await;
        if result.is_ok() {
            self.emit(scene_id, status, None, error);
        }
    }

    async fn run_scene(self: Arc<Self>, task: Task) -> Outcome {
        let Ok(_permit) = self.limit.acquire().await else {
            return Outcome::Skipped;
        };
        if self.hooks.cancel.is_cancelled() || self.engine_down.load(Ordering::Relaxed) {
            return Outcome::Skipped;
        }
        self.set_status(&task.scene_id, AssetStatus::Running, None)
            .await;

        let mut text = normalize_spoken(&task.text);
        if self.language == Language::Km {
            text = khmer_numbers::replace_digits(&text);
        }
        if text.is_empty() {
            let err = (
                "This scene has no spoken text.".to_owned(),
                "other".to_owned(),
            );
            self.set_status(&task.scene_id, AssetStatus::Failed, Some(err))
                .await;
            return Outcome::Failed;
        }
        let request = TtsRequest {
            text: text.clone(),
            voice: self.voice.voice_name.clone(),
            rate: self.voice.rate,
            pitch: self.voice.pitch,
            model: self.voice.model.clone(),
            style: self.voice.direction.clone(),
        };
        match self.engine.synthesize(&request, &self.hooks).await {
            Ok(output) => match self.save(&task, &text, output).await {
                Ok(outcome) => outcome,
                Err(e) => {
                    self.log
                        .error(&format!("{} voice could not be saved: {e}", task.scene_id));
                    let err = (e.to_string(), "other".to_owned());
                    self.set_status(&task.scene_id, AssetStatus::Failed, Some(err))
                        .await;
                    Outcome::Failed
                }
            },
            Err(TtsError::Cancelled) => {
                self.set_status(&task.scene_id, AssetStatus::Pending, None)
                    .await;
                Outcome::Skipped
            }
            Err(e) => {
                let err = (e.detail(), e.kind().to_owned());
                self.log.error(&format!(
                    "{} voice failed [{}]: {}",
                    task.scene_id, err.1, err.0
                ));
                if matches!(e, TtsError::EdgeUnavailable(_)) {
                    self.engine_down.store(true, Ordering::Relaxed);
                    if let Ok(mut slot) = self.engine_error.lock() {
                        slot.get_or_insert_with(|| err.clone());
                    }
                }
                self.set_status(&task.scene_id, AssetStatus::Failed, Some(err))
                    .await;
                Outcome::Failed
            }
        }
    }

    /// Writes `voice.<ext>` (+ `words.json`), measures it and records the result.
    async fn save(
        &self,
        task: &Task,
        text: &str,
        output: TtsOutput,
    ) -> Result<Outcome, StorageError> {
        let duration = audio::duration_ms(&output.audio, output.format).filter(|d| *d > 0);
        let Some(duration) = duration else {
            let err = (
                "The voice engine returned audio that could not be read.".to_owned(),
                "bad_response".to_owned(),
            );
            self.set_status(&task.scene_id, AssetStatus::Failed, Some(err))
                .await;
            return Ok(Outcome::Failed);
        };
        let rel_dir = format!("scenes/{}", task.scene_id);
        let dir = self.paths.project_dir(&self.project_id).join(&rel_dir);
        remove_old_voice_files(&dir);
        let file = format!("voice.{}", output.format.extension());
        atomic::write_bytes(&dir.join(&file), &output.audio)?;
        let words_path = if output.words.is_empty() {
            None
        } else {
            atomic::write_json(&dir.join("words.json"), &output.words)?;
            Some(format!("{rel_dir}/words.json"))
        };

        let rate = if self.voice.engine == EngineKind::Edge {
            self.voice.rate
        } else {
            0
        };
        let expected = audio::expected_ms(text, self.language, rate);
        let suspicious = audio::suspicious_length(duration, expected);
        let error = suspicious.then(|| {
            (
                format!(
                    "Suspicious audio length: {:.1} s, expected about {:.1} s. Listen and regenerate if it is cut off or repeats.",
                    duration as f64 / 1000.0,
                    expected as f64 / 1000.0
                ),
                "suspicious_length".to_owned(),
            )
        });
        let status = if suspicious {
            AssetStatus::Failed
        } else {
            AssetStatus::Done
        };
        let path = format!("{rel_dir}/{file}");
        let (err, hash, wp) = (error.clone(), task.hash.clone(), words_path.clone());
        let saved = self
            .store
            .update(&self.project_id, |p| {
                let s = p
                    .scenes
                    .iter_mut()
                    .find(|s| s.id == task.scene_id)
                    .ok_or(ProjectError::SceneNotFound)?;
                s.voice.status = status;
                s.voice.path = Some(path);
                s.voice.duration_ms = Some(duration);
                s.voice.text_hash = Some(hash);
                s.voice.words_path = wp;
                let (e, k) = err.map_or((None, None), |(d, k)| (Some(d), Some(k)));
                s.voice.error = e;
                s.voice.error_kind = k;
                Ok(())
            })
            .await;
        if saved.is_err() {
            // Scene deleted meanwhile: nothing to report.
            return Ok(Outcome::Skipped);
        }
        self.log.info(&format!(
            "{} voice {} ({:.1} s, {} timing(s))",
            task.scene_id,
            if suspicious {
                "saved but SUSPICIOUS"
            } else {
                "ok"
            },
            duration as f64 / 1000.0,
            output.words.len()
        ));
        self.emit(&task.scene_id, status, Some(duration), error);
        Ok(if suspicious {
            Outcome::Failed
        } else {
            Outcome::Done
        })
    }
}

fn remove_old_voice_files(dir: &Path) {
    for name in ["voice.mp3", "voice.wav", "words.json"] {
        // Missing files are fine; anything else surfaces when the new file is written.
        let _ = std::fs::remove_file(dir.join(name));
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex as StdMutex;

    use super::*;
    use crate::pipeline::project::Scene;
    use crate::pipeline::project::tests::sample;
    use crate::tts::{AudioFormat, TtsFuture, WordTiming};

    /// Engine that returns a WAV whose length matches the text, or fails on demand.
    struct FakeEngine {
        fail_with: Option<fn() -> TtsError>,
        calls: StdMutex<Vec<String>>,
    }

    impl TtsEngine for FakeEngine {
        fn synthesize<'a>(&'a self, r: &'a TtsRequest, _h: &'a RetryHooks) -> TtsFuture<'a> {
            Box::pin(async move {
                self.calls.lock().unwrap().push(r.text.clone());
                if let Some(f) = self.fail_with {
                    return Err(f());
                }
                let ms = audio::expected_ms(&r.text, Language::Km, 0);
                let pcm = vec![0u8; (ms * 48) as usize]; // 24 kHz × 2 bytes per ms
                Ok(TtsOutput {
                    audio: audio::pcm16_to_wav(&pcm, 24_000, 1),
                    format: AudioFormat::Wav,
                    words: vec![WordTiming {
                        text: r.text.clone(),
                        start_ms: 0,
                        end_ms: ms,
                    }],
                })
            })
        }
    }

    #[derive(Default)]
    struct Recorder {
        scenes: StdMutex<Vec<SceneProgress>>,
        progress: StdMutex<Vec<GenerateProgress>>,
    }
    impl VoiceEvents for Recorder {
        fn scene(&self, p: SceneProgress) {
            self.scenes.lock().unwrap().push(p);
        }
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
        for i in 0..n {
            let mut s = Scene::new(format!("s{:03}", i + 1), 0);
            s.narration = "ស្រូវ".into();
            s.spoken_text = format!("ស្រូវកើតមកពីណា ឆ្នាំ{}", 1800 + i);
            s.image_prompt = "rice".into();
            p.scenes.push(s);
        }
        p.renumber();
        let id = p.id.clone();
        store.insert(&p).await.unwrap();
        (dir, store, paths, id)
    }

    fn job(store: &Arc<ProjectStore>, paths: &AppPaths, engine: Arc<FakeEngine>) -> VoiceJob {
        VoiceJob {
            store: store.clone(),
            paths: paths.clone(),
            engine,
            voice: crate::storage::settings::VoiceDefaults::default().resolve(Language::Km),
            hooks: RetryHooks::default(),
            log: LogFile::new(paths.root().join("test.log")),
            concurrency: 2,
        }
    }

    fn engine(fail_with: Option<fn() -> TtsError>) -> Arc<FakeEngine> {
        Arc::new(FakeEngine {
            fail_with,
            calls: StdMutex::new(vec![]),
        })
    }

    #[tokio::test]
    async fn generates_saves_and_resumes_without_redoing() {
        let (_d, store, paths, id) = setup(3).await;
        let eng = engine(None);
        let rec = Arc::new(Recorder::default());
        job(&store, &paths, eng.clone())
            .run(&id, None, rec.clone())
            .await;

        let p = store.get(&id).await.unwrap();
        for s in &p.scenes {
            assert_eq!(s.voice.status, AssetStatus::Done, "{}", s.id);
            let file = paths.project_dir(&id).join(s.voice.path.as_ref().unwrap());
            assert!(file.is_file());
            assert!(s.voice.duration_ms.unwrap() > 0);
            assert!(
                paths
                    .project_dir(&id)
                    .join(s.voice.words_path.as_ref().unwrap())
                    .is_file()
            );
        }
        // Digits were converted to Khmer words before TTS.
        assert!(
            eng.calls
                .lock()
                .unwrap()
                .iter()
                .all(|t| !t.chars().any(|c| c.is_ascii_digit()))
        );
        let last = rec.progress.lock().unwrap().last().cloned().unwrap();
        assert_eq!(
            (last.stage, last.done, last.total),
            (GenerateStage::Done, 3, 3)
        );

        // Second run: nothing to do. Edit one scene → only that one is redone.
        let eng2 = engine(None);
        job(&store, &paths, eng2.clone())
            .run(&id, None, Arc::new(Recorder::default()))
            .await;
        assert!(eng2.calls.lock().unwrap().is_empty());
        store
            .update(&id, |p| {
                p.scenes[1].spoken_text = "អត្ថបទថ្មី".into();
                Ok(())
            })
            .await
            .unwrap();
        let eng3 = engine(None);
        job(&store, &paths, eng3.clone())
            .run(&id, None, Arc::new(Recorder::default()))
            .await;
        assert_eq!(eng3.calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn changing_voice_settings_regenerates_all() {
        let (_d, store, paths, id) = setup(2).await;
        job(&store, &paths, engine(None))
            .run(&id, None, Arc::new(Recorder::default()))
            .await;
        let eng = engine(None);
        let mut j = job(&store, &paths, eng.clone());
        j.voice.voice_name = "km-KH-PisethNeural".into();
        j.run(&id, None, Arc::new(Recorder::default())).await;
        assert_eq!(eng.calls.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn single_scene_regenerate_is_forced() {
        let (_d, store, paths, id) = setup(3).await;
        job(&store, &paths, engine(None))
            .run(&id, None, Arc::new(Recorder::default()))
            .await;
        let eng = engine(None);
        job(&store, &paths, eng.clone())
            .run(
                &id,
                Some(vec!["s002".into()]),
                Arc::new(Recorder::default()),
            )
            .await;
        assert_eq!(eng.calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn edge_unavailable_stops_the_job_with_a_clear_error() {
        let (_d, store, paths, id) = setup(5).await;
        let eng = engine(Some(|| {
            TtsError::EdgeUnavailable("HTTP 403 during handshake".into())
        }));
        let rec = Arc::new(Recorder::default());
        let mut j = job(&store, &paths, eng.clone());
        j.concurrency = 1;
        j.run(&id, None, rec.clone()).await;
        assert_eq!(
            eng.calls.lock().unwrap().len(),
            1,
            "remaining scenes are skipped"
        );
        let last = rec.progress.lock().unwrap().last().cloned().unwrap();
        assert_eq!(last.stage, GenerateStage::Failed);
        assert_eq!(last.error_kind.as_deref(), Some("edge_unavailable"));
        assert!(last.error.unwrap().contains("switch to Gemini TTS"));
        let p = store.get(&id).await.unwrap();
        assert_eq!(p.scenes[0].voice.status, AssetStatus::Failed);
        assert_eq!(p.scenes[4].voice.status, AssetStatus::Pending);
    }

    #[tokio::test]
    async fn refuses_unapproved_script() {
        let (_d, store, paths, id) = setup(1).await;
        store
            .update(&id, |p| {
                p.status = ProjectStatus::ScriptDraft;
                Ok(())
            })
            .await
            .unwrap();
        let rec = Arc::new(Recorder::default());
        job(&store, &paths, engine(None))
            .run(&id, None, rec.clone())
            .await;
        assert_eq!(rec.progress.lock().unwrap()[0].stage, GenerateStage::Failed);
    }

    /// End-to-end: real script (Gemini, saved key) + real Edge TTS voices for the
    /// 1-minute Khmer test project. Run with:
    /// `cargo test --manifest-path src-tauri/Cargo.toml live_one_minute -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "calls Gemini and Edge TTS"]
    async fn live_one_minute_khmer_voices() {
        use crate::gemini::GeminiClient;
        use crate::gemini::client::ModelChain;
        use crate::pipeline::editing::{NewProjectInput, approve, new_project};
        use crate::pipeline::script_job::ScriptJob;
        use crate::storage::secrets;
        use crate::storage::settings::Settings;
        use crate::tts::edge::EdgeTts;

        let key = secrets::get_api_key().unwrap().expect("no API key saved");
        let settings = Settings::default();
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(dir.path().to_path_buf());
        let store = Arc::new(ProjectStore::new(paths.clone()));
        let project = new_project(
            &NewProjectInput {
                topic: "How was Angkor Wat built?".into(),
                language: Language::Km,
                target_minutes: 1,
                style_preset: "storybook_watercolor".into(),
            },
            &settings.models.tts,
        )
        .unwrap();
        let id = project.id.clone();
        store.insert(&project).await.unwrap();
        let log = LogFile::new(paths.project_log(&id)).redacting(key.expose());
        let client = GeminiClient::new(&settings.concurrency).unwrap();
        let chain = ModelChain::new(&settings.models.text_chain);
        ScriptJob {
            store: &store,
            client: &client,
            key: &key,
            models: &chain,
            hooks: &RetryHooks::default(),
            log: &log,
        }
        .run(&id, |p| {
            if p.error.is_some() {
                eprintln!("script step failed [{:?}]: {:?}", p.error_kind, p.error);
            }
        })
        .await;
        store
            .update(&id, approve)
            .await
            .expect("script should be complete (see 'script step failed' above)");

        let rec = Arc::new(Recorder::default());
        VoiceJob {
            store: store.clone(),
            paths: paths.clone(),
            engine: Arc::new(EdgeTts::new(settings.concurrency.tts)),
            voice: settings.voice.resolve(Language::Km),
            hooks: RetryHooks::default(),
            log: log.clone(),
            concurrency: settings.concurrency.tts,
        }
        .run(&id, None, rec.clone())
        .await;

        let p = store.get(&id).await.unwrap();
        let mut total_ms = 0;
        for s in &p.scenes {
            let words = s
                .voice
                .words_path
                .as_ref()
                .and_then(|w| std::fs::read_to_string(paths.project_dir(&id).join(w)).ok())
                .map(|t| serde_json::from_str::<Vec<WordTiming>>(&t).unwrap().len())
                .unwrap_or(0);
            eprintln!(
                "{} {:?} {:>5} ms {:>2} timings  {}",
                s.id,
                s.voice.status,
                s.voice.duration_ms.unwrap_or(0),
                words,
                s.voice.error.as_deref().unwrap_or("")
            );
            total_ms += s.voice.duration_ms.unwrap_or(0);
        }
        eprintln!("total narration: {:.1} s", total_ms as f64 / 1000.0);
        eprintln!(
            "---- log ----
{}",
            std::fs::read_to_string(paths.project_log(&id)).unwrap_or_default()
        );
        let last = rec.progress.lock().unwrap().last().cloned().unwrap();
        assert_eq!(last.stage, GenerateStage::Done, "{:?}", last.error);
        assert!(p.scenes.iter().all(|s| s.voice.path.is_some()));
    }
}
