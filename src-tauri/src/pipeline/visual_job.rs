//! Real-world photos and clips (`docs/11-real-world-visuals.md` §7): for every photo / video
//! scene without a file, search with its English keywords, take the best-ranked candidate,
//! download it once and make the grid thumbnail. The pick is marked `auto_picked` ("check")
//! until the user confirms it or picks another. Resumable: saved after every scene. When every
//! source is rate-limited the job pauses with the reset time; a scene with no result falls back
//! to an AI image (`Scene::needs_ai_image`).

use std::path::Path;
use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use super::events::{GenerateProgress, GenerateStage, JobEvents, JobKind, SceneProgress};
use super::project::{AssetStatus, Project, Scene, VisualKind};
use super::store::ProjectStore;
use crate::images::postprocess;
use crate::render::clip::Frame;
use crate::stock::assets::{self, AssetRecord, Downloader};
use crate::stock::search::{Query, Searcher, SourceNotice};
use crate::stock::{Candidate, Media, StockError};
use crate::storage::log::LogFile;
use crate::storage::{AppPaths, atomic};

const ASSET: &str = "visual";
/// Candidates kept on the scene for the picker.
const KEEP: usize = 6;
/// Next candidates tried when a download fails.
const DOWNLOAD_TRIES: usize = 3;

pub struct VisualJob {
    pub store: Arc<ProjectStore>,
    pub paths: AppPaths,
    pub searcher: Searcher,
    pub downloader: Arc<dyn Downloader>,
    pub log: LogFile,
    pub cancel: CancellationToken,
}

/// Scenes whose photo / clip still has to be found.
pub fn todo(p: &Project, paths: &AppPaths, only: Option<&[String]>) -> Vec<String> {
    let dir = paths.project_dir(&p.id);
    let registry = assets::AssetRegistry::load(&paths.project_assets_dir(&p.id)).unwrap_or_default();
    p.scenes
        .iter()
        .filter(|s| only.is_none_or(|o| o.contains(&s.id)))
        .filter(|s| matches!(s.visual.kind, VisualKind::RealPhoto | VisualKind::StockVideo))
        .filter(|s| {
            only.is_some()
                || s.visual.asset.as_deref().and_then(|id| registry.get(id)).is_none_or(|r| !dir.join(&r.file).is_file())
        })
        .map(|s| s.id.clone())
        .collect()
}

enum SceneResult {
    Picked { candidates: Vec<Candidate>, record: Box<AssetRecord>, video: bool },
    NotFound(String),
    /// Every source is waiting for its limit to reset (RFC 3339).
    Paused(String),
}

/// Writes `scenes/<id>/visual_thumb.jpg` from the asset (a video's poster) cropped around
/// `focus`; returns the relative path. Decoding big photos is CPU work: blocking thread.
pub async fn write_thumb(project_dir: &Path, scene_id: &str, record: &AssetRecord, focus: Option<[f32; 2]>) -> Result<String, String> {
    let src = match record.media {
        Media::Photo => record.file.clone(),
        Media::Video => record.poster.clone().ok_or("this clip has no preview picture")?,
    };
    let src = project_dir.join(src);
    let rel = format!("scenes/{scene_id}/visual_thumb.jpg");
    let dest = project_dir.join(&rel);
    let focus = focus.unwrap_or([0.5, 0.5]);
    tokio::task::spawn_blocking(move || {
        let raw = std::fs::read(&src).map_err(|e| e.to_string())?;
        let jpg = postprocess::thumb_at(&raw, focus).map_err(|e| e.to_string())?;
        atomic::write_bytes(&dest, &jpg).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(rel)
}

fn notices_all_paused(notices: &[SourceNotice]) -> Option<String> {
    let paused: Vec<&str> = notices.iter().filter(|n| n.kind == "stock_rate_limited").filter_map(|n| n.retry_at_utc.as_deref()).collect();
    (!notices.is_empty() && paused.len() == notices.len()).then(|| paused.iter().min().map(|s| (*s).to_owned())).flatten()
}

impl VisualJob {
    async fn find(&self, scene: &Scene, frame_width: u32, assets_dir: &Path) -> Result<SceneResult, StockError> {
        let keywords = &scene.visual.keywords;
        let mut media = if scene.visual.kind == VisualKind::StockVideo { Media::Video } else { Media::Photo };
        let mut notices = Vec::new();
        let mut found = Vec::new();
        'search: for m in [media, Media::Photo] {
            for kw in keywords.iter().take(2) {
                let q = Query { text: kw.clone(), keywords: keywords.clone(), media: m, page: 1, frame_width, limit: KEEP };
                let out = self.searcher.search(&q).await;
                notices.extend(out.notices);
                if !out.candidates.is_empty() {
                    found = out.candidates;
                    media = m;
                    break 'search;
                }
            }
            if m == Media::Photo {
                break;
            }
            // No clip found: a photo is better than an AI image.
        }
        if found.is_empty() {
            if let Some(until) = notices_all_paused(&notices) {
                return Ok(SceneResult::Paused(until));
            }
            let what = keywords.first().cloned().unwrap_or_default();
            return Ok(SceneResult::NotFound(format!("nothing found for \"{what}\"")));
        }
        // Best full-resolution candidate first, then the rest in rank order.
        let mut order: Vec<&Candidate> = found.iter().filter(|c| !c.low_res).collect();
        order.extend(found.iter().filter(|c| c.low_res));
        let mut last = None;
        for c in order.into_iter().take(DOWNLOAD_TRIES) {
            match assets::ensure(assets_dir, c, self.downloader.as_ref(), &self.cancel).await {
                Ok(record) => return Ok(SceneResult::Picked { record: Box::new(record), video: media == Media::Video, candidates: found }),
                Err(StockError::Cancelled) => return Err(StockError::Cancelled),
                Err(e) => {
                    self.log.info(&format!("scene {}: download of {} failed: {e}", scene.id, c.asset_id()));
                    last = Some(e);
                }
            }
        }
        Err(last.unwrap_or_else(|| StockError::Download("no file could be downloaded".into())))
    }

    pub async fn run(&self, project_id: &str, only: Option<Vec<String>>, events: Arc<dyn JobEvents>) {
        let progress = |stage, done, failed, total, error: Option<(String, String)>, resets: Option<String>| {
            let (error, error_kind) = error.map_or((None, None), |(d, k)| (Some(d), Some(k)));
            events.progress(GenerateProgress {
                project_id: project_id.to_owned(),
                job: JobKind::Visual,
                stage,
                done,
                failed,
                total,
                error,
                error_kind,
                resets_at_utc: resets,
            });
        };
        let project = match self.store.get(project_id).await {
            Ok(p) => p,
            Err(e) => return progress(GenerateStage::Failed, 0, 0, 0, Some((e.to_string(), "other".into())), None),
        };
        let ids = todo(&project, &self.paths, only.as_deref());
        let total = ids.len();
        let (mut done, mut failed) = (0, 0);
        let frame_width = Frame::for_resolution(&project.render.resolution).width;
        let dir = self.paths.project_dir(project_id);
        let assets_dir = self.paths.project_assets_dir(project_id);
        progress(GenerateStage::Running, 0, 0, total, None, None);
        for id in ids {
            if self.cancel.is_cancelled() {
                return progress(GenerateStage::Cancelled, done, failed, total, None, None);
            }
            let Ok(p) = self.store.get(project_id).await else { break };
            let Some(scene) = p.scenes.iter().find(|s| s.id == id).cloned() else { continue };
            let scene_event = |status, error: Option<(String, String)>| {
                let (error, error_kind) = error.map_or((None, None), |(d, k)| (Some(d), Some(k)));
                events.scene(SceneProgress { project_id: project_id.to_owned(), scene_id: id.clone(), asset: ASSET, status, duration_ms: None, error, error_kind });
            };
            scene_event(AssetStatus::Running, None);
            let result = self.find(&scene, frame_width, &assets_dir).await;
            let saved = match result {
                Err(StockError::Cancelled) => return progress(GenerateStage::Cancelled, done, failed, total, None, None),
                Ok(SceneResult::Paused(until)) => {
                    self.log.info(&format!("visuals paused: every source is rate-limited until {until}"));
                    scene_event(AssetStatus::Pending, None);
                    return progress(GenerateStage::PausedQuota, done, failed, total, None, Some(until));
                }
                Ok(SceneResult::Picked { candidates, record, video }) => {
                    let thumb = write_thumb(&dir, &id, &record, None).await;
                    if let Err(e) = &thumb {
                        self.log.info(&format!("scene {id}: thumbnail failed: {e}"));
                    }
                    self.log.info(&format!("scene {id}: picked {} ({}x{}, {})", record.id, record.width, record.height, record.license.label()));
                    self.store
                        .update(project_id, |p| {
                            if let Some(s) = p.scenes.iter_mut().find(|s| s.id == id) {
                                let v = &mut s.visual;
                                v.kind = if video { VisualKind::StockVideo } else { VisualKind::RealPhoto };
                                v.candidates = candidates;
                                v.asset = Some(record.id.clone());
                                v.auto_picked = true;
                                v.focus = None;
                                v.status = AssetStatus::Done;
                                v.error = None;
                                v.error_kind = None;
                                v.thumb_path = thumb.ok();
                            }
                            Ok(())
                        })
                        .await
                        .map(|_| (AssetStatus::Done, None))
                }
                Ok(SceneResult::NotFound(msg)) => {
                    self.log.info(&format!("scene {id}: {msg} → AI image fallback"));
                    self.mark_failed(project_id, &id, &msg, "stock_no_results").await.map(|()| (AssetStatus::Failed, Some((msg, "stock_no_results".into()))))
                }
                Err(e) => {
                    let msg = e.to_string();
                    self.log.info(&format!("scene {id}: {msg}"));
                    self.mark_failed(project_id, &id, &msg, e.kind()).await.map(|()| (AssetStatus::Failed, Some((msg, e.kind().to_owned()))))
                }
            };
            match saved {
                Ok((status, err)) => {
                    if status == AssetStatus::Done {
                        done += 1;
                    } else {
                        failed += 1;
                    }
                    scene_event(status, err);
                    progress(GenerateStage::Running, done, failed, total, None, None);
                }
                Err(e) => return progress(GenerateStage::Failed, done, failed, total, Some((e.to_string(), "other".into())), None),
            }
        }
        progress(GenerateStage::Done, done, failed, total, None, None);
    }

    async fn mark_failed(&self, project_id: &str, id: &str, msg: &str, kind: &str) -> Result<(), super::ProjectError> {
        self.store
            .update(project_id, |p| {
                if let Some(s) = p.scenes.iter_mut().find(|s| s.id == id) {
                    s.visual.status = AssetStatus::Failed;
                    s.visual.asset = None;
                    s.visual.error = Some(msg.to_owned());
                    s.visual.error_kind = Some(kind.to_owned());
                }
                Ok(())
            })
            .await
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::net::transport::mock::{self, MockTransport};
    use crate::pipeline::events::tests::Recorder;
    use crate::pipeline::project::tests::sample;
    use crate::stock::assets::DownloadFuture;
    use crate::stock::cache::SearchCache;
    use crate::stock::search::Keys;
    use crate::storage::settings::StockSettings;

    struct Jpeg(Mutex<usize>);

    impl Downloader for Jpeg {
        fn fetch<'a>(&'a self, _u: &'a str, _ua: Option<String>, _m: u64, dest: &'a Path, _c: &'a CancellationToken) -> DownloadFuture<'a> {
            *self.0.lock().unwrap() += 1;
            Box::pin(async move {
                let img = image::RgbImage::from_pixel(64, 48, image::Rgb([20, 90, 160]));
                let mut out = std::io::Cursor::new(Vec::new());
                image::DynamicImage::ImageRgb8(img).write_to(&mut out, image::ImageFormat::Jpeg).unwrap();
                std::fs::write(dest, out.get_ref()).unwrap();
                Ok(out.get_ref().len() as u64)
            })
        }
    }

    const COMMONS: &str = r#"{"query":{"pages":[{"pageid":11,"title":"File:Submarine cable.jpg","index":1,"imageinfo":[{"size":10,"width":4000,"height":2250,
      "url":"https://u/1.jpg","descriptionurl":"https://c/1","mime":"image/jpeg","extmetadata":{"LicenseShortName":{"value":"CC0"}}}]}]}}"#;

    async fn setup(responses: Vec<Result<crate::net::transport::HttpResponse, crate::net::transport::TransportError>>) -> (tempfile::TempDir, VisualJob, Project) {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(dir.path().to_path_buf());
        let store = Arc::new(ProjectStore::new(paths.clone()));
        let mut p = sample();
        p.style.preset = "tech_explainer".into();
        for (i, kind) in [VisualKind::RealPhoto, VisualKind::Diagram, VisualKind::RealPhoto].into_iter().enumerate() {
            let mut s = Scene::new(format!("s00{}", i + 1), 0);
            s.visual.kind = kind;
            s.visual.keywords = vec!["submarine cable".into()];
            p.scenes.push(s);
        }
        store.insert(&p).await.unwrap();
        let settings = StockSettings { pixabay: false, openverse: false, ..StockSettings::default() };
        let searcher = Searcher::new(Arc::new(MockTransport::new(responses)), SearchCache::new(dir.path().join("cache")), Keys::default(), settings);
        let job = VisualJob { store, paths, searcher, downloader: Arc::new(Jpeg(Mutex::new(0))), log: LogFile::new(dir.path().join("log.txt")), cancel: CancellationToken::new() };
        (dir, job, p)
    }

    #[tokio::test]
    async fn picks_downloads_once_and_marks_for_review() {
        // One live search; the second scene with the same keywords is served from the cache.
        let (_dir, job, p) = setup(vec![mock::status(200, COMMONS)]).await;
        let rec = Arc::new(Recorder::default());
        job.run(&p.id, None, rec.clone()).await;
        let p = job.store.get(&p.id).await.unwrap();
        for id in ["s001", "s003"] {
            let v = &p.scenes.iter().find(|s| s.id == id).unwrap().visual;
            assert_eq!((v.status, v.asset.as_deref(), v.auto_picked), (AssetStatus::Done, Some("wikimedia_11"), true));
            assert!(v.thumb_path.is_some());
        }
        assert_eq!(p.scenes[1].visual.status, AssetStatus::Pending, "diagram scenes are not searched");
        assert!(!p.scenes[0].needs_ai_image());
        let last = rec.progress.lock().unwrap().last().cloned().unwrap();
        assert_eq!((last.stage, last.done, last.total), (GenerateStage::Done, 2, 2));
        assert!(todo(&p, &job.paths, None).is_empty(), "nothing left: resumable");
    }

    #[tokio::test]
    async fn nothing_found_falls_back_to_an_ai_image() {
        let empty = r#"{"query":{"pages":[]}}"#;
        let (_dir, job, p) = setup(vec![mock::status(200, empty), mock::status(200, empty)]).await;
        job.run(&p.id, Some(vec!["s001".into()]), Arc::new(Recorder::default())).await;
        let p = job.store.get(&p.id).await.unwrap();
        let s = &p.scenes[0];
        assert_eq!((s.visual.status, s.visual.error_kind.as_deref()), (AssetStatus::Failed, Some("stock_no_results")));
        assert!(s.needs_ai_image());
    }

    #[tokio::test]
    async fn pauses_when_every_source_is_rate_limited() {
        let (_dir, job, p) = setup(vec![mock::status_retry_after(429, 60)]).await;
        let rec = Arc::new(Recorder::default());
        job.run(&p.id, None, rec.clone()).await;
        let last = rec.progress.lock().unwrap().last().cloned().unwrap();
        assert_eq!(last.stage, GenerateStage::PausedQuota);
        assert!(last.resets_at_utc.is_some());
        let p = job.store.get(&p.id).await.unwrap();
        assert_eq!(p.scenes[0].visual.status, AssetStatus::Pending, "picked up again on resume");
    }
}
