//! Manual check of a style preset with the real Cloudflare Flux endpoint (uses the
//! account ID from settings.json and the token from the keyring; ~58 neurons per image).
//! Ignored by default:
//!
//! ```text
//! NITEAN_SETTINGS=%APPDATA%/NiteanStudio/settings.json NITEAN_OUT=<folder> \
//! cargo test --manifest-path src-tauri/Cargo.toml style_probe -- --ignored --nocapture
//! ```

use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use super::cloudflare::CloudflareFlux;
use super::prompt::{PromptParts, build_prompt};
use super::provider::{ImageProvider, ImageRequest};
use crate::net::transport::ReqwestTransport;
use crate::pipeline::presets;
use crate::pipeline::project::Shot;
use crate::storage::secrets;
use crate::storage::settings::Settings;

#[tokio::test]
#[ignore = "calls Cloudflare with the user's account (see module docs)"]
async fn style_probe() {
    let settings_path = std::env::var("NITEAN_SETTINGS").expect("NITEAN_SETTINGS");
    let out = std::env::var("NITEAN_OUT").expect("NITEAN_OUT");
    let settings: Settings = serde_json::from_str(&std::fs::read_to_string(settings_path).unwrap()).unwrap();
    let token = secrets::get_cloudflare_token().unwrap().expect("Cloudflare token in the keyring");
    let flux = CloudflareFlux::new(
        Arc::new(ReqwestTransport::new().unwrap()),
        &settings.image.cloudflare_account_id,
        token,
    );
    let style = presets::find("stickman").unwrap().style_prompt;
    let cases = [
        (
            "the developer sits at a desk writing code on a laptop, a phone beside the laptop shows an app",
            vec![("developer", "stick figure with a small blue cap and an orange hoodie")],
            None,
            Shot::Medium,
        ),
        (
            "the farmer points at the three towers of the temple at sunrise",
            vec![("farmer", "stick figure with a red-and-white checked krama scarf and a straw hat")],
            Some("simple line drawing of a temple with three lotus-shaped towers, light orange accent"),
            Shot::Wide,
        ),
    ];
    for (i, (subject, characters, place, shot)) in cases.into_iter().enumerate() {
        let prompt = build_prompt(&PromptParts {
            style,
            shot,
            subject,
            characters,
            place,
            culture_notes: None,
            square: true,
            rules: "",
        });
        println!("prompt {i}: {prompt}");
        let req = ImageRequest { prompt, seed: 7, steps: Some(settings.image.steps), reference_images: vec![] };
        let img = flux.generate(&req, &CancellationToken::new()).await.unwrap();
        std::fs::write(format!("{out}/stickman_{i}.jpg"), &img.bytes).unwrap();
        println!("image {i}: {}×{}", img.width, img.height);
    }
}
