//! `resources/local_models.json` (`docs/08-local-gpu.md` §4): pinned engine version and the
//! model profiles the user can install. Files are downloaded on demand, never bundled.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    /// stable-diffusion.cpp release tag bundled with the app (also written to the log).
    pub engine_version: String,
    pub profiles: Vec<Profile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Profile {
    pub id: String,
    pub label: String,
    pub min_vram_mb: u64,
    pub files: Vec<ModelFile>,
    #[serde(rename = "gen")]
    pub generation: GenParams,
    /// Prompt adapter: "sd15".
    pub prompt_style: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FileRole {
    Model,
    Lora,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelFile {
    pub role: FileRole,
    /// File name on disk (inside `models/<profile id>/`).
    pub name: String,
    pub url: String,
    pub sha256: String,
    pub size_mb: u64,
    pub license: String,
    pub license_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GenParams {
    pub width: u32,
    pub height: u32,
    pub steps: u32,
    pub cfg: f32,
    pub sampler: String,
    #[serde(default)]
    pub scheduler: Option<String>,
    #[serde(default)]
    pub vae_tiling: bool,
    /// LoRA strength for `role: lora` files.
    #[serde(default = "one")]
    pub lora_multiplier: f32,
}

fn one() -> f32 {
    1.0
}

fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

fn is_safe_name(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)) && !s.starts_with('.')
}

/// The manifest shipped with this build.
pub fn shipped() -> Manifest {
    // Validated by the `the_shipped_manifest_is_valid` test; an empty list if ever broken.
    Manifest::parse(include_str!("../../resources/local_models.json")).unwrap_or(Manifest {
        engine_version: String::new(),
        profiles: Vec::new(),
    })
}

impl Manifest {
    pub fn parse(json: &str) -> Result<Self, String> {
        let m: Manifest = serde_json::from_str(json).map_err(|e| format!("local_models.json: {e}"))?;
        m.validate()?;
        Ok(m)
    }

    fn validate(&self) -> Result<(), String> {
        let mut ids = std::collections::HashSet::new();
        for p in &self.profiles {
            if !is_safe_name(&p.id) || !ids.insert(&p.id) {
                return Err(format!("profile id '{}' is invalid or duplicated", p.id));
            }
            if !p.files.iter().any(|f| f.role == FileRole::Model) {
                return Err(format!("profile '{}' has no model file", p.id));
            }
            if p.generation.width % 64 != 0 || p.generation.height % 64 != 0 || p.generation.steps == 0 {
                return Err(format!("profile '{}' has an invalid size or step count", p.id));
            }
            for f in &p.files {
                if !f.url.starts_with("https://") || !is_sha256(&f.sha256) || !is_safe_name(&f.name) {
                    return Err(format!("profile '{}': file '{}' needs an https URL, a sha256 and a plain name", p.id, f.name));
                }
                if f.license.trim().is_empty() || !f.license_url.starts_with("https://") {
                    return Err(format!("profile '{}': file '{}' needs its license name and URL", p.id, f.name));
                }
            }
        }
        Ok(())
    }

    pub fn profile(&self, id: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.id == id)
    }
}

impl Profile {
    pub fn total_mb(&self) -> u64 {
        self.files.iter().map(|f| f.size_mb).sum()
    }

    pub fn model(&self) -> Option<&ModelFile> {
        self.files.iter().find(|f| f.role == FileRole::Model)
    }

    pub fn loras(&self) -> impl Iterator<Item = &ModelFile> {
        self.files.iter().filter(|f| f.role == FileRole::Lora)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "8f90d840e075ff588a58e22c6586e2ae9a6f7922996ee6649a7f01072333afe4";

    fn json(min_vram: u64, url: &str, sha: &str) -> String {
        format!(
            r#"{{ "engine_version": "master-929-3f8527a", "profiles": [{{
                "id": "sd15_lcm", "label": "Fast", "min_vram_mb": {min_vram},
                "files": [
                  {{ "role": "model", "name": "model.safetensors", "url": "{url}", "sha256": "{sha}",
                     "size_mb": 2034, "license": "CreativeML OpenRAIL-M", "license_url": "https://example.org/l" }},
                  {{ "role": "lora", "name": "lcm.safetensors", "url": "{url}", "sha256": "{sha}",
                     "size_mb": 128, "license": "OpenRAIL++", "license_url": "https://example.org/l" }}
                ],
                "gen": {{ "width": 768, "height": 448, "steps": 6, "cfg": 1.5, "sampler": "lcm", "vae_tiling": true }},
                "prompt_style": "sd15" }}] }}"#
        )
    }

    #[test]
    fn parses_and_filters_by_vram() {
        let m = Manifest::parse(&json(3800, "https://h/x", SHA)).unwrap();
        let p = m.profile("sd15_lcm").unwrap();
        assert_eq!(p.total_mb(), 2162);
        assert_eq!(p.loras().count(), 1);
        assert!((p.generation.lora_multiplier - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn rejects_http_urls_and_bad_hashes() {
        assert!(Manifest::parse(&json(3800, "http://h/x", SHA)).is_err());
        assert!(Manifest::parse(&json(3800, "https://h/x", "abc")).is_err());
    }

    #[test]
    fn the_shipped_manifest_is_valid() {
        let shipped = include_str!("../../resources/local_models.json");
        Manifest::parse(shipped).unwrap();
    }
}
