//! Local GPU image generation with stable-diffusion.cpp (`docs/08-local-gpu.md`, Phase 6).

pub mod download;
pub mod engine;
#[cfg(test)]
mod engine_probe;
pub mod gpu;
pub mod install;
pub mod manifest;
pub mod provider;
