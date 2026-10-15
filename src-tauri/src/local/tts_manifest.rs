//! `resources/local_tts_models.json` (`docs/09-voice-profiles.md`): the pinned audio.cpp
//! build and the VoxCPM2 model files, downloaded on demand with the same installer as the
//! image profiles (`local::install`).

use serde::{Deserialize, Serialize};

use super::manifest::{ModelFile, Package, is_safe_id, validate_files};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TtsManifest {
    pub engine_version: String,
    pub models: Vec<TtsModel>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TtsModel {
    pub id: String,
    pub label: String,
    /// VoxCPM2 Q8 peaks at ~5.0 GB with one sentence per request (measured on Vulkan).
    pub min_vram_mb: u64,
    pub files: Vec<ModelFile>,
}

impl Package for TtsModel {
    fn id(&self) -> &str {
        &self.id
    }
    fn label(&self) -> &str {
        &self.label
    }
    fn min_vram_mb(&self) -> u64 {
        self.min_vram_mb
    }
    fn files(&self) -> &[ModelFile] {
        &self.files
    }
}

impl TtsModel {
    /// The GGUF given to the server (`path` of the configured model).
    pub fn model_file(&self) -> Option<&ModelFile> {
        self.files.first()
    }
}

impl TtsManifest {
    pub fn parse(json: &str) -> Result<Self, String> {
        let m: TtsManifest = serde_json::from_str(json).map_err(|e| format!("local_tts_models.json: {e}"))?;
        let mut ids = std::collections::HashSet::new();
        for model in &m.models {
            if !is_safe_id(&model.id) || !ids.insert(&model.id) || model.files.is_empty() {
                return Err(format!("voice model '{}' is invalid or duplicated", model.id));
            }
            validate_files(&model.id, &model.files)?;
        }
        Ok(m)
    }

    /// The default (first) voice model.
    pub fn default_model(&self) -> Option<&TtsModel> {
        self.models.first()
    }
}

pub fn shipped() -> TtsManifest {
    // Validated by `the_shipped_manifest_is_valid`; empty if ever broken.
    TtsManifest::parse(include_str!("../../resources/local_tts_models.json"))
        .unwrap_or(TtsManifest { engine_version: String::new(), models: Vec::new() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_manifest_is_valid() {
        let m = TtsManifest::parse(include_str!("../../resources/local_tts_models.json")).unwrap();
        let v = m.default_model().unwrap();
        assert_eq!(v.id, "voxcpm2_q8");
        assert_eq!(v.model_file().unwrap().sha256, "c8e01ab4416011e12a28f24ede298a1aa5ce64b43f8e8aaad53b1e2fe7c96432");
        assert!(m.engine_version.contains("v0.9.0"));
    }

    #[test]
    fn rejects_bad_files() {
        let bad = r#"{"engine_version":"x","models":[{"id":"m","label":"M","min_vram_mb":1,
            "files":[{"role":"model","name":"m.gguf","url":"http://x","sha256":"00","size_mb":1,
            "license":"L","license_url":"https://l"}]}]}"#;
        assert!(TtsManifest::parse(bad).is_err());
    }
}
