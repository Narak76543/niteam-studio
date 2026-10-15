//! Manual run of the real `sd-server` on this machine's GPU (model candidates, speed).
//! Ignored by default:
//!
//! ```text
//! NITEAN_SDCPP=<folder with sd-server.exe> NITEAN_MODEL=<model.safetensors>
//! NITEAN_LORA_DIR=<folder> [NITEAN_LORA=<file in it>] NITEAN_OUT=<folder> NITEAN_TAG=<name>
//! [NITEAN_STEPS=6 NITEAN_CFG=1.5 NITEAN_SAMPLER=lcm]
//! cargo test --manifest-path src-tauri/Cargo.toml engine_probe -- --ignored --nocapture
//! ```

use std::path::PathBuf;
use std::time::Instant;

use tokio_util::sync::CancellationToken;

use super::engine::{EngineConfig, GenRequest, SdServer, device_for, list_devices};
use super::gpu;
use crate::images::prompt::PromptParts;
use crate::images::prompt_sd15::{NEGATIVE, build_prompt_sd15};
use crate::pipeline::presets;
use crate::pipeline::project::Shot;

fn env(k: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| panic!("{k}"))
}

#[tokio::test]
#[ignore = "runs the real engine on the GPU (see module docs)"]
async fn engine_probe() {
    let dir = PathBuf::from(env("NITEAN_SDCPP"));
    let exe = dir.join("sd-server.exe");
    let out = PathBuf::from(env("NITEAN_OUT"));
    let tag = env("NITEAN_TAG");
    let num = |k: &str, d: f32| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);

    let gpu = gpu::detect().expect("a GPU");
    let devices = list_devices(&exe).await.unwrap();
    let device = device_for(&devices, &gpu.name);
    println!("gpu {gpu:?} → device {device:?}");

    let cfg = EngineConfig {
        exe,
        model: PathBuf::from(env("NITEAN_MODEL")),
        lora_dir: PathBuf::from(env("NITEAN_LORA_DIR")),
        device,
    };
    let cancel = CancellationToken::new();
    let t0 = Instant::now();
    let mut server = SdServer::start(&cfg, &cancel).await.unwrap();
    println!("model loaded in {:.1} s", t0.elapsed().as_secs_f64());

    let short = presets::find("stickman").unwrap().short_style;
    let scenes = [
        ("the developer sits at a desk writing code on a laptop, a phone beside the laptop shows an app",
         vec![("developer", "stick figure with a small blue cap and an orange hoodie")], None, Shot::Medium),
        ("the farmer points at the three towers of the temple at sunrise",
         vec![("farmer", "stick figure with a red-and-white checked krama scarf and a straw hat")],
         Some("simple line drawing of a temple with three lotus-shaped towers, light orange accent"), Shot::Wide),
        ("two workers pull a huge stone block on wooden rollers",
         vec![], None, Shot::Wide),
    ];
    let loras: Vec<(String, f32)> = std::env::var("NITEAN_LORA").ok().map(|l| vec![(l, 1.0)]).unwrap_or_default();
    for (i, (subject, characters, place, shot)) in scenes.into_iter().enumerate() {
        let prompt = build_prompt_sd15(
            &PromptParts { style: "", shot, subject, characters, place, culture_notes: None, square: false },
            short,
        );
        let req = GenRequest {
            prompt,
            negative_prompt: NEGATIVE.into(),
            width: 768,
            height: 448,
            seed: 1000 + i as i64,
            steps: num("NITEAN_STEPS", 6.0) as u32,
            cfg: num("NITEAN_CFG", 1.5),
            sampler: std::env::var("NITEAN_SAMPLER").unwrap_or_else(|_| "lcm".into()),
            scheduler: None,
            vae_tiling: true,
            loras: loras.clone(),
        };
        let t = Instant::now();
        let png = server.generate(&req, &cancel).await.unwrap();
        println!("image {i}: {:.2} s, {} KB", t.elapsed().as_secs_f64(), png.len() / 1024);
        std::fs::write(out.join(format!("{tag}_{i}.png")), png).unwrap();
    }
    server.stop().await;
}
