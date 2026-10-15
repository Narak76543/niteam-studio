//! Cost estimate shown before generating (07 §5). Numbers are estimates.
//! Cloudflare: 10,000 free neurons/day; Flux Schnell 4.80 neurons per 512×512 tile +
//! 9.60 per step (developers.cloudflare.com/workers-ai/platform/pricing, 2026-10-15).
//! Gemini: price per 1K image from ai.google.dev/gemini-api/docs/pricing (2026-10-15).

use serde::Serialize;

use super::plan::ScenePlan;
use super::provider::ProviderId;

pub const DAILY_FREE_NEURONS: f64 = 10_000.0;
const NEURONS_PER_TILE: f64 = 4.80;
const NEURONS_PER_STEP: f64 = 9.60;
/// Flux Schnell on Workers AI returns 1024×1024.
const FLUX_SIZE: (u32, u32) = (1024, 1024);

pub fn flux_neurons(width: u32, height: u32, steps: u8) -> f64 {
    let tiles = f64::from(width.div_ceil(512) * height.div_ceil(512));
    tiles * NEURONS_PER_TILE + f64::from(steps) * NEURONS_PER_STEP
}

/// USD per 1K image (standard tier).
pub fn gemini_price_usd(model: &str) -> f64 {
    match model {
        "gemini-3.1-flash-lite-image" | "gemini-nano-banana-2.1" => 0.0336,
        "gemini-3-pro-image" => 0.134,
        _ => 0.067,
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ImageEstimate {
    /// Images that will be generated (scenes sharing an image or already up to date excluded).
    pub images: usize,
    pub cloudflare: usize,
    pub gemini: usize,
    /// Already generated and up to date.
    pub cached: usize,
    pub cloudflare_neurons: f64,
    /// Share of one day's free allowance.
    pub cloudflare_percent_of_daily: f64,
    pub gemini_cost_usd: f64,
    /// How many Cloudflare images fit in one day's allowance at these settings.
    pub cloudflare_images_per_day: u32,
}

/// `todo` = plans that need generating; `cached` = up-to-date images.
pub fn estimate(todo: &[&ScenePlan], cached: usize) -> ImageEstimate {
    let mut e = ImageEstimate {
        images: 0,
        cloudflare: 0,
        gemini: 0,
        cached,
        cloudflare_neurons: 0.0,
        cloudflare_percent_of_daily: 0.0,
        gemini_cost_usd: 0.0,
        cloudflare_images_per_day: 0,
    };
    let mut per_image = flux_neurons(FLUX_SIZE.0, FLUX_SIZE.1, 4);
    for p in todo.iter().filter(|p| p.shared_from.is_none()) {
        e.images += 1;
        match p.provider {
            ProviderId::CloudflareFlux => {
                per_image = flux_neurons(FLUX_SIZE.0, FLUX_SIZE.1, p.steps);
                e.cloudflare += 1;
                e.cloudflare_neurons += per_image;
            }
            ProviderId::GeminiNanoBanana => {
                e.gemini += 1;
                e.gemini_cost_usd += gemini_price_usd(&p.model);
            }
        }
    }
    e.cloudflare_percent_of_daily = (e.cloudflare_neurons / DAILY_FREE_NEURONS * 100.0).round();
    e.cloudflare_images_per_day = (DAILY_FREE_NEURONS / per_image).floor() as u32;
    e.gemini_cost_usd = (e.gemini_cost_usd * 100.0).round() / 100.0;
    e
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(provider: ProviderId, model: &str, shared: bool) -> ScenePlan {
        ScenePlan {
            scene_id: "s001".into(),
            shared_from: shared.then(|| "s000".into()),
            provider,
            model: model.into(),
            prompt: String::new(),
            seed: 0,
            steps: 4,
            hash: String::new(),
            key_scene: false,
        }
    }

    #[test]
    fn flux_cost_matches_doc_estimate() {
        // 07 §1a: ~58 neurons per image at 4 steps → ~170 images/day.
        assert!((flux_neurons(1024, 1024, 4) - 57.6).abs() < 1e-9);
        let todo = [plan(ProviderId::CloudflareFlux, "m", false)];
        let e = estimate(&todo.iter().collect::<Vec<_>>(), 0);
        assert_eq!(e.cloudflare_images_per_day, 173);
    }

    #[test]
    fn doc_example_80_cloudflare_6_gemini() {
        let mut todo: Vec<ScenePlan> = (0..80)
            .map(|_| plan(ProviderId::CloudflareFlux, "m", false))
            .collect();
        todo.extend((0..6).map(|_| {
            plan(
                ProviderId::GeminiNanoBanana,
                "gemini-3.1-flash-lite-image",
                false,
            )
        }));
        todo.push(plan(ProviderId::CloudflareFlux, "m", true));
        let e = estimate(&todo.iter().collect::<Vec<_>>(), 3);
        assert_eq!((e.images, e.cloudflare, e.gemini, e.cached), (86, 80, 6, 3));
        assert_eq!(e.cloudflare_percent_of_daily, 46.0);
        assert!((e.gemini_cost_usd - 0.2).abs() < 1e-9);
    }
}
