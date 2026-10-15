//! Manual end-to-end render check with the real FFmpeg on a copy of a real project.
//! Ignored by default:
//!
//! ```text
//! NITEAN_FFMPEG=src-tauri/binaries/ffmpeg-x86_64-pc-windows-gnu.exe \
//! NITEAN_PROJECT=%APPDATA%/NiteanStudio/projects/<id> \
//! NITEAN_OUT=<folder for final.mp4> \
//! cargo test --manifest-path src-tauri/Cargo.toml render_e2e -- --ignored --nocapture
//! ```
//!
//! Subtitles are stand-in PNGs (the real ones are drawn by the WebView).

use std::path::Path;
use std::sync::{Arc, Mutex};

use tokio_util::sync::CancellationToken;

use super::ProjectStore;
use super::render_job::{RenderEvents, RenderJob, RenderProgress, RenderStage};
use super::subs::{self, CuePng};
use crate::render::encoders;
use crate::render::ffmpeg::Ffmpeg;
use crate::storage::AppPaths;
use crate::storage::log::LogFile;

struct Collect(Mutex<Vec<RenderProgress>>);

impl RenderEvents for Collect {
    fn progress(&self, p: RenderProgress) {
        if let Ok(mut v) = self.0.lock() {
            v.push(p);
        }
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A 1920×240 PNG with a red bar (stand-in for a canvas-drawn subtitle).
fn bar_png() -> Vec<u8> {
    let img = image::RgbaImage::from_fn(1920, 240, |x, y| {
        if (300..1620).contains(&x) && (60..180).contains(&y) {
            image::Rgba([220, 40, 40, 255])
        } else {
            image::Rgba([0, 0, 0, 0])
        }
    });
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

#[tokio::test]
#[ignore = "needs FFmpeg and a real project (see module docs)"]
async fn render_e2e() {
    let ffmpeg_path = std::env::var("NITEAN_FFMPEG").expect("NITEAN_FFMPEG");
    let src = std::env::var("NITEAN_PROJECT").expect("NITEAN_PROJECT");
    let out_dir = std::env::var("NITEAN_OUT").expect("NITEAN_OUT");
    let src = Path::new(&src);
    let id = src.file_name().unwrap().to_string_lossy().into_owned();

    let root = tempfile::tempdir().unwrap();
    let paths = AppPaths::new(root.path().to_path_buf());
    let dir = paths.project_dir(&id);
    copy_dir(src, &dir);
    let store = Arc::new(ProjectStore::new(paths.clone()));

    // Stand-in subtitles: two states per scene.
    let project = store.get(&id).await.unwrap();
    for task in subs::plan(&project, &dir) {
        let half = task.duration_ms / 2;
        let cues = [(0, half), (half, task.duration_ms)].map(|(a, b)| CuePng {
            text: "test".into(),
            start_ms: a,
            end_ms: b,
            png: bar_png(),
        });
        let p = store.get(&id).await.unwrap();
        let asset = subs::write_pngs(&p, &task.scene_id, &task.hash, &cues, &dir).unwrap();
        store.update(&id, |p| subs::apply(p, &task.scene_id, asset)).await.unwrap();
    }

    let ffmpeg = Ffmpeg::at(ffmpeg_path.into());
    let encoder = encoders::detect(&ffmpeg, &dir, &CancellationToken::new()).await.unwrap();
    println!("encoder: {}", encoder.name());

    let job = RenderJob {
        store: store.clone(),
        paths: paths.clone(),
        ffmpeg,
        encoder,
        output_dir: Some(out_dir.clone()),
        log: LogFile::new(paths.project_log(&id)),
        ffmpeg_lock: Arc::new(tokio::sync::Mutex::new(())),
    };
    let events = Arc::new(Collect(Mutex::new(Vec::new())));
    let started = std::time::Instant::now();
    job.run(&id, CancellationToken::new(), events.clone()).await;
    let log = std::fs::read_to_string(paths.project_log(&id)).unwrap_or_default();
    println!("{}", log.lines().filter(|l| l.contains("clip ok") || l.contains("render")).collect::<Vec<_>>().join("\n"));

    {
        let events = events.0.lock().unwrap();
        let last = events.last().expect("events");
        assert_eq!(last.stage, RenderStage::Done, "{:?}", last.error);
        println!("{} events, done in {:.1} s → {:?}", events.len(), started.elapsed().as_secs_f64(), last.output_path);
    }

    // Second run: every clip is reused, only the concat runs again.
    let again = std::time::Instant::now();
    let events2 = Arc::new(Collect(Mutex::new(Vec::new())));
    job.run(&id, CancellationToken::new(), events2.clone()).await;
    assert!(events2.0.lock().unwrap().iter().all(|e| e.stage != RenderStage::Clips));
    println!("re-render with cached clips: {:.1} s", again.elapsed().as_secs_f64());
}
