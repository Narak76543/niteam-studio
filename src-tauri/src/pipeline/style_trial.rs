//! Manual style trial: a 6-scene Khmer history project written by Gemini (script, Visual
//! Bible, image prompts, through `ScriptJob`) and saved in the app's own project list, then
//! 3 local-GPU seeds per scene. Calls the real Gemini API and GPU, so ignored by default:
//!
//! ```text
//! NITEAN_SDCPP=<folder with sd-server.exe> NITEAN_OUT=<folder>
//! [NITEAN_PRESET=soft_2d_explainer] [NITEAN_TRIAL_PROJECT=<id>: reuse, no Gemini calls]
//! cargo test --manifest-path src-tauri/Cargo.toml style_trial -- --ignored --nocapture
//! ```

use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use super::editing::{NewProjectInput, new_project};
use super::project::{Aspect, Language, Outline, OutlineChapter};
use super::script_job::ScriptJob;
use super::store::ProjectStore;
use crate::gemini::GeminiClient;
use crate::gemini::client::ModelChain;
use crate::gemini::retry::{RetryHooks, RetryNotice};
use crate::images::plan::plan_all;
use crate::images::postprocess;
use crate::images::provider::{ImageProvider, ImageRequest};
use crate::local::engine::{EngineConfig, device_for, list_devices};
use crate::local::provider::{Cooldown, LocalSdCpp};
use crate::local::{gpu, install, manifest};
use crate::storage::log::LogFile;
use crate::storage::{AppPaths, secrets, settings};

const SEEDS: u32 = 3;

fn outline() -> Outline {
    let chapter = |title: &str, summary: &str| OutlineChapter {
        title: title.into(),
        summary: summary.into(),
        scene_count: 3,
        done: false,
    };
    Outline {
        title: "ប្រវត្តិសាស្ត្រខ្មែរ ក្នុងមួយនាទី".into(),
        hook: "How did a few families on green hills become the people who built Angkor?".into(),
        chapters: vec![
            chapter(
                "From hills to rice fields",
                "Scene 1: prehistoric people gathered together on green hills. Scene 2: a village of \
                 wooden stilt houses beside a river. Scene 3: farmers planting rice in flooded fields.",
            ),
            chapter(
                "Kings, temples and festivals",
                "Scene 1: a king on a simple throne with two advisers. Scene 2: workers building a \
                 stone temple. Scene 3: a crowd celebrating with lanterns at night.",
            ),
        ],
    }
}

fn app_paths() -> AppPaths {
    let appdata = std::env::var("APPDATA").expect("APPDATA");
    AppPaths::new(PathBuf::from(appdata).join("NiteanStudio"))
}

async fn write_project(paths: &AppPaths, store: &ProjectStore, preset: &str) -> String {
    let key = secrets::get_api_key().expect("keyring").expect("no Gemini key saved");
    let settings = settings::load(paths).expect("settings");
    let mut project = new_project(
        &NewProjectInput {
            topic: "ប្រវត្តិសាស្ត្រខ្មែរ ពីមនុស្សបុរេប្រវត្តិ ដល់ពិធីបុណ្យនៅសម័យអង្គរ".into(),
            language: Language::Km,
            target_minutes: 1,
            aspect: Aspect::Wide,
            style_preset: preset.into(),
        },
        &settings.models.tts,
    )
    .unwrap();
    let o = outline();
    project.title = o.title.clone();
    project.outline = Some(o);
    store.insert(&project).await.unwrap();
    println!("project {} created", project.id);

    let client = GeminiClient::new(&settings.concurrency).unwrap();
    let chain = ModelChain::new(&settings.models.text_chain);
    let log = LogFile::new(paths.project_log(&project.id)).redacting(key.expose());
    let hooks = RetryHooks::new(
        Default::default(),
        Arc::new(|n: &RetryNotice| eprintln!("[retry] {} {}/{}: {}", n.model, n.attempt, n.max_attempts, n.error)),
    );
    let job = ScriptJob { store, client: &client, key: &key, models: &chain, hooks: &hooks, log: &log };
    job.run(&project.id, |p| eprintln!("[script] {:?} {}/{}", p.stage, p.chapters_done, p.chapters_total))
        .await;
    project.id
}

#[tokio::test]
#[ignore = "calls the real Gemini API and the GPU (see module docs)"]
async fn style_trial() {
    let out = PathBuf::from(std::env::var("NITEAN_OUT").expect("NITEAN_OUT"));
    std::fs::create_dir_all(&out).unwrap();
    let preset = std::env::var("NITEAN_PRESET").unwrap_or_else(|_| "soft_2d_explainer".into());
    let paths = app_paths();
    let store = ProjectStore::new(paths.clone());
    let id = match std::env::var("NITEAN_TRIAL_PROJECT") {
        Ok(id) => id,
        Err(_) => write_project(&paths, &store, &preset).await,
    };
    let mut project = store.get(&id).await.unwrap();
    assert_eq!(project.scenes.len(), 6, "script did not finish");
    // What the image job snapshots from Settings at start (`commands::images::snapshot`).
    let image = settings::load(&paths).expect("settings").image;
    project.image_settings.provider = image.provider;
    project.image_settings.key_scene_provider = image.key_scene_provider;
    project.image_settings.local_profile = image.local_profile;

    let mut report = String::new();
    let bible = project.visual_bible.as_ref().expect("visual bible");
    writeln!(report, "# {} ({})\n\nstyle: {}\n", project.title, project.id, bible.style).unwrap();
    for (k, d) in bible.characters.iter().chain(&bible.places) {
        writeln!(report, "- {k}: {d}").unwrap();
    }

    // The app's local provider with the installed profile (OOM retry included).
    let sdcpp = PathBuf::from(std::env::var("NITEAN_SDCPP").expect("NITEAN_SDCPP"));
    let exe = sdcpp.join("sd-server.exe");
    let gpu = gpu::detect().expect("a GPU");
    let device = device_for(&list_devices(&exe).await.unwrap(), &gpu.name);
    let profile_id = project.image_settings.local_profile.clone().unwrap_or_else(|| "zimage_turbo".into());
    let profile = manifest::shipped().profile(&profile_id).cloned().expect("profile");
    let models = PathBuf::from(std::env::var("LOCALAPPDATA").unwrap()).join("NiteanStudio").join("models");
    let dir = install::profile_dir(&models, &profile);
    let engine = |p: &manifest::Profile| {
        let files = p.loaded().map(|(flag, f)| (flag.to_owned(), dir.join(&f.name))).collect();
        let cfg = EngineConfig { exe: exe.clone(), files, args: p.engine_args.clone(), lora_dir: dir.clone(), device: device.clone() };
        LocalSdCpp::new(cfg, p.clone(), Cooldown::default(), Arc::new(Mutex::new(())))
    };
    let local = engine(&profile);
    let cancel = CancellationToken::new();

    let plans = plan_all(&project);
    let total = Instant::now();
    let mut times = Vec::new();
    for (n, (scene, plan)) in project.scenes.iter().zip(&plans).enumerate() {
        writeln!(report, "\n## Scene {} ({})\n{}\n\nprompt: {}", n + 1, scene.id, scene.narration, plan.prompt).unwrap();
        for k in 0..SEEDS {
            let seed = plan.seed + k;
            let req = ImageRequest { prompt: plan.prompt.clone(), seed, steps: None, reference_images: vec![] };
            let t = Instant::now();
            let img = local.generate(&req, &cancel).await.unwrap();
            let secs = t.elapsed().as_secs_f64();
            times.push(secs);
            let name = format!("scene{}_seed{seed}.png", n + 1);
            std::fs::write(out.join(&name), &img.bytes).unwrap();
            let line = format!("{name}: {secs:.1} s, {}x{}", img.width, img.height);
            println!("{line}");
            writeln!(report, "- {line}").unwrap();
        }
    }
    let all = total.elapsed().as_secs_f64();
    let first = times.first().copied().unwrap_or_default();
    let warm: Vec<f64> = times.iter().skip(1).copied().collect();
    writeln!(
        report,
        "\n{} images in {all:.0} s; first {first:.1} s (model load), then {:.1} s average",
        times.len(),
        warm.iter().sum::<f64>() / warm.len().max(1) as f64
    )
    .unwrap();

    // Scene 1 at 1280×720 with the same seed, and both as the 1080p export pan image.
    let scene1 = &plans[0];
    let req = ImageRequest { prompt: scene1.prompt.clone(), seed: scene1.seed, steps: None, reference_images: vec![] };
    let small = std::fs::read(out.join(format!("scene1_seed{}.png", scene1.seed))).unwrap();
    drop(local);
    let mut big_profile = profile.clone();
    (big_profile.generation.width, big_profile.generation.height) = (1280, 720);
    let t = Instant::now();
    let big = engine(&big_profile).generate(&req, &cancel).await.unwrap();
    writeln!(report, "\nscene1 1280x720: {:.1} s (incl. model load), {}x{}", t.elapsed().as_secs_f64(), big.width, big.height).unwrap();
    std::fs::write(out.join("res_1280x720.png"), &big.bytes).unwrap();
    for (name, raw) in [("res_1024x576_pan1080.png", &small), ("res_1280x720_pan1080.png", &big.bytes)] {
        std::fs::write(out.join(name), postprocess::process(raw, (1920, 1080)).unwrap().pan_png).unwrap();
    }
    std::fs::write(out.join("report.md"), &report).unwrap();
    println!("{report}");
}
