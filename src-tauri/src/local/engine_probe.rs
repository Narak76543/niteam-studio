//! Manual run of the real `sd-server` on this machine's GPU (model candidates, speed).
//! Ignored by default:
//!
//! ```text
//! NITEAN_SDCPP=<folder with sd-server.exe> NITEAN_OUT=<folder> NITEAN_TAG=<name>
//! NITEAN_FILES="--diffusion-model=<gguf>;--llm=<gguf>;--vae=<file>"
//! [NITEAN_ARGS="--offload-to-cpu --diffusion-fa"] [NITEAN_PRESET=<id>, default stickman]
//! [NITEAN_STYLE=<style text>, default: the preset's]
//! [NITEAN_SUBJECTS="<scene text>|<scene text>", default: three built-in scenes]
//! [NITEAN_STEPS=8 NITEAN_CFG=1.0 NITEAN_SAMPLER=<engine default> NITEAN_W=1024 NITEAN_H=576]
//! cargo test --manifest-path src-tauri/Cargo.toml engine_probe -- --ignored --nocapture
//! ```

use std::path::PathBuf;
use std::time::Instant;

use tokio_util::sync::CancellationToken;

use super::engine::{EngineConfig, GenRequest, SdServer, device_for, list_devices};
use super::gpu;
use crate::images::prompt::{PromptParts, build_prompt};
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
        files: env("NITEAN_FILES")
            .split(';')
            .filter_map(|p| p.split_once('='))
            .map(|(f, p)| (f.to_owned(), PathBuf::from(p)))
            .collect(),
        args: std::env::var("NITEAN_ARGS").map(|a| a.split(' ').map(str::to_owned).collect()).unwrap_or_default(),
        lora_dir: out.clone(),
        device,
    };
    let cancel = CancellationToken::new();
    let t0 = Instant::now();
    let mut server = SdServer::start(&cfg, &cancel).await.unwrap();
    println!("model loaded in {:.1} s", t0.elapsed().as_secs_f64());

    // NITEAN_PRESET=<id> (default stickman): style and fixed rules; NITEAN_STYLE overrides the style.
    let preset_id = std::env::var("NITEAN_PRESET").unwrap_or_else(|_| "stickman".into());
    let preset = presets::find(&preset_id).unwrap();
    let style_owned = std::env::var("NITEAN_STYLE").unwrap_or_else(|_| preset.style_prompt.to_owned());
    let style = style_owned.as_str();
    let rules = preset.prompt_rules;
    let scenes = [
        ("the developer sits at a desk writing code on a laptop, a phone beside the laptop shows an app",
         vec![("developer", "stick figure with a small blue cap and an orange hoodie")], None, Shot::Medium),
        ("the farmer points at the three towers of the temple at sunrise",
         vec![("farmer", "stick figure with a red-and-white checked krama scarf and a straw hat")],
         Some("simple line drawing of a temple with three lotus-shaped towers, light orange accent"), Shot::Wide),
        ("two workers pull a huge stone block on wooden rollers",
         vec![], None, Shot::Wide),
    ];
    // NITEAN_SUBJECTS="subject 1|subject 2": own scene texts instead of the built-in ones.
    let custom = std::env::var("NITEAN_SUBJECTS").unwrap_or_default();
    let scenes: Vec<_> = if custom.is_empty() {
        scenes.into_iter().collect()
    } else {
        custom.split('|').map(|s| (s, vec![], None, Shot::Wide)).collect()
    };
    for (i, (subject, characters, place, shot)) in scenes.into_iter().enumerate() {
        let prompt = build_prompt(&PromptParts { style, shot, subject, characters, place, culture_notes: None, square: false, rules });
        let req = GenRequest {
            prompt,
            negative_prompt: String::new(),
            width: num("NITEAN_W", 1024.0) as u32,
            height: num("NITEAN_H", 576.0) as u32,
            seed: 1000 + i as i64,
            steps: num("NITEAN_STEPS", 8.0) as u32,
            cfg: num("NITEAN_CFG", 1.0),
            sampler: std::env::var("NITEAN_SAMPLER").ok(),
            scheduler: None,
            vae_tiling: false,
            loras: vec![],
        };
        let t = Instant::now();
        let png = server.generate(&req, &cancel).await.unwrap();
        println!("image {i}: {:.2} s, {} KB", t.elapsed().as_secs_f64(), png.len() / 1024);
        std::fs::write(out.join(format!("{tag}_{i}.png")), png).unwrap();
    }
    server.stop().await;
}
