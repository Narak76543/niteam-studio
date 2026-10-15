//! `LocalSdCpp` image provider (`docs/08-local-gpu.md` §6): stable-diffusion.cpp on the
//! user's GPU. The server is started on the first image of a job and stays loaded until the
//! job ends (the provider is dropped → `kill_on_drop`).

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use super::engine::{EngineConfig, EngineError, GenRequest, SdServer};
use super::manifest::Profile;
use crate::images::prompt_sd15::NEGATIVE;
use crate::images::provider::{Aspect, GeneratedImage, ImageError, ImageProvider, ImageRequest};

/// Size for the one automatic retry after running out of GPU memory (08 §6).
const OOM_FALLBACK: (u32, u32) = (640, 384);

/// Optional pause every N images so laptops can cool down (08 §8).
#[derive(Debug, Clone, Copy, Default)]
pub struct Cooldown {
    pub every: u32,
    pub seconds: u32,
}

pub struct LocalSdCpp {
    engine: EngineConfig,
    profile: Profile,
    cooldown: Cooldown,
    /// One FFmpeg / GPU job at a time across the app (08 §3).
    gpu_lock: Arc<Mutex<()>>,
    server: Mutex<Option<SdServer>>,
    generated: Mutex<u32>,
}

impl LocalSdCpp {
    pub fn new(engine: EngineConfig, profile: Profile, cooldown: Cooldown, gpu_lock: Arc<Mutex<()>>) -> Self {
        Self { engine, profile, cooldown, gpu_lock, server: Mutex::new(None), generated: Mutex::new(0) }
    }

    pub fn lora_names(&self) -> Vec<(String, f32)> {
        self.profile.loras().map(|f| (f.name.clone(), self.profile.generation.lora_multiplier)).collect()
    }

    fn request(&self, req: &ImageRequest, size: (u32, u32), vae_tiling: bool) -> GenRequest {
        let g = &self.profile.generation;
        GenRequest {
            prompt: req.prompt.clone(),
            negative_prompt: NEGATIVE.into(),
            width: size.0,
            height: size.1,
            seed: i64::from(req.seed),
            steps: g.steps,
            cfg: g.cfg,
            sampler: g.sampler.clone(),
            scheduler: g.scheduler.clone(),
            vae_tiling,
            loras: self.lora_names(),
        }
    }

    async fn run_once(&self, request: &GenRequest, cancel: &CancellationToken) -> Result<Vec<u8>, EngineError> {
        let mut slot = self.server.lock().await;
        if slot.is_none() {
            *slot = Some(SdServer::start(&self.engine, cancel).await?);
        }
        let Some(server) = slot.as_mut() else {
            return Err(EngineError::Crashed("not started".into()));
        };
        let result = server.generate(request, cancel).await;
        if matches!(result, Err(EngineError::Crashed(_) | EngineError::OutOfMemory(_))) {
            // Start fresh next time (the process may be gone or in a bad state).
            if let Some(s) = slot.take() {
                s.stop().await;
            }
        }
        result
    }

    async fn maybe_cool_down(&self, cancel: &CancellationToken) {
        let mut n = self.generated.lock().await;
        *n += 1;
        if self.cooldown.every > 0 && self.cooldown.seconds > 0 && *n % self.cooldown.every == 0 {
            tokio::select! {
                () = cancel.cancelled() => {}
                () = tokio::time::sleep(Duration::from_secs(u64::from(self.cooldown.seconds))) => {}
            }
        }
    }
}

fn to_image_error(e: EngineError) -> ImageError {
    match e {
        EngineError::Missing(_) | EngineError::Crashed(_) => ImageError::Engine(e.to_string()),
        EngineError::Cancelled => ImageError::Other("Cancelled".into()),
        EngineError::OutOfMemory(_) | EngineError::Http(_) => ImageError::Other(e.to_string()),
    }
}

#[async_trait]
impl ImageProvider for LocalSdCpp {
    fn id(&self) -> &'static str {
        "local_sdcpp"
    }

    fn supports_reference_images(&self) -> bool {
        false
    }

    fn native_aspect(&self) -> Aspect {
        Aspect::Wide16x9
    }

    fn model(&self) -> &str {
        &self.profile.id
    }

    async fn generate(&self, req: &ImageRequest, cancel: &CancellationToken) -> Result<GeneratedImage, ImageError> {
        let _gpu = tokio::select! {
            () = cancel.cancelled() => return Err(ImageError::Other("Cancelled".into())),
            g = self.gpu_lock.lock() => g,
        };
        let g = &self.profile.generation;
        let first = self.request(req, (g.width, g.height), g.vae_tiling);
        let png = match self.run_once(&first, cancel).await {
            Err(EngineError::OutOfMemory(_)) => {
                // 08 §6: retry once smaller, with VAE tiling.
                let retry = self.request(req, OOM_FALLBACK, true);
                self.run_once(&retry, cancel).await.map_err(to_image_error)?
            }
            other => other.map_err(to_image_error)?,
        };
        drop(_gpu);
        self.maybe_cool_down(cancel).await;
        let img = image::load_from_memory(&png).map_err(|e| ImageError::Other(e.to_string()))?;
        Ok(GeneratedImage { width: img.width(), height: img.height(), bytes: png, mime: "image/png".into() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_failures_stop_the_job_but_oom_does_not() {
        assert_eq!(to_image_error(EngineError::Missing("x".into())).kind(), "local_engine");
        assert_eq!(to_image_error(EngineError::Crashed("x".into())).kind(), "local_engine");
        assert_eq!(to_image_error(EngineError::OutOfMemory("x".into())).kind(), "other");
        assert!(to_image_error(EngineError::OutOfMemory("x".into())).to_string().contains("GPU ran out of memory"));
    }
}
