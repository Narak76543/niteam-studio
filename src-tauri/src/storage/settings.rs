//! `settings.json`: everything except the API key. Model IDs live here, not in code.

use serde::{Deserialize, Serialize};

use super::{AppPaths, StorageError, atomic};
use crate::images::provider::ProviderId;
use crate::pipeline::editing::DEFAULT_VOICE_DIRECTION;
use crate::pipeline::project::{Language, VoiceSettings};
use crate::tts::{EngineKind, voices};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub schema_version: u32,
    /// "km" | "en"; `None` = follow the Windows display language.
    pub ui_language: Option<String>,
    /// "light" | "dark"; `None` = follow the Windows app mode.
    pub theme: Option<String>,
    /// Where finished videos are copied. `None` = inside the project folder.
    pub output_dir: Option<String>,
    pub models: ModelSettings,
    pub concurrency: ConcurrencySettings,
    pub voice: VoiceDefaults,
    pub image: ImageSettings,
    pub render: RenderPrefs,
    /// Detected GPU (cached at startup, 08 §2).
    pub gpu: Option<crate::local::gpu::GpuInfo>,
    pub local: LocalPrefs,
}

/// Settings → Local images (08 §8, §9).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct LocalPrefs {
    /// Pause after every N local images (0 = off).
    pub cooldown_every: u32,
    pub cooldown_seconds: u32,
    /// The one-time "your GPU can make images for free" card was answered.
    pub offer_dismissed: bool,
    /// From the benchmark (2 test images); used for time estimates.
    pub seconds_per_image: Option<f32>,
}

/// Settings → Video. The detected encoder is cached here (03 "Encoder detection").
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct RenderPrefs {
    /// `h264_qsv` | `h264_amf` | `h264_nvenc` | `libx264`; `None` = detect on the next render.
    pub encoder: Option<String>,
}

/// Settings → Images (`docs/07-image-generation.md` §1).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ImageSettings {
    pub provider: ProviderId,
    /// Gemini image model (paid only).
    pub gemini_model: String,
    /// Flux Schnell steps, 4–8.
    pub steps: u8,
    /// Optional better provider for hook / chapter-start / ending scenes.
    pub key_scene_provider: Option<ProviderId>,
    /// Cloudflare account ID (not secret; the API token is in the keyring).
    pub cloudflare_account_id: String,
    /// Cloudflare requests at a time.
    pub cloudflare_concurrency: usize,
    /// Installed local model profile used by `local_sdcpp` (08 §4).
    pub local_profile: Option<String>,
}

impl Default for ImageSettings {
    fn default() -> Self {
        Self {
            provider: ProviderId::CloudflareFlux,
            gemini_model: "gemini-3.1-flash-lite-image".into(),
            steps: 4,
            key_scene_provider: None,
            cloudflare_account_id: String::new(),
            cloudflare_concurrency: 2,
            local_profile: None,
        }
    }
}

impl ImageSettings {
    fn validate(&self) -> Result<(), StorageError> {
        let bad = |what: &str| Err(StorageError::InvalidSetting(what.to_owned()));
        if !(4..=8).contains(&self.steps) {
            return bad("image steps must be 4–8");
        }
        if self.gemini_model.is_empty() || !is_model_id(&self.gemini_model) {
            return bad("Gemini image model");
        }
        let id = self.cloudflare_account_id.trim();
        if !id.is_empty() && !(id.len() == 32 && id.chars().all(|c| c.is_ascii_hexdigit())) {
            return bad("Cloudflare account ID (32 hexadecimal characters)");
        }
        if !(1..=4).contains(&self.cloudflare_concurrency) {
            return bad("Cloudflare concurrency must be 1–4");
        }
        Ok(())
    }
}

/// Voice engine and voices (Settings → Voice). Resolved per project language with
/// [`VoiceDefaults::resolve`] when voice generation starts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct VoiceDefaults {
    pub engine: EngineKind,
    pub edge_voice_km: String,
    pub edge_voice_en: String,
    /// Edge speaking rate in % (−50…+50).
    pub rate: i32,
    /// Edge pitch in Hz (−50…+50).
    pub pitch: i32,
    pub gemini_voice: String,
    pub gemini_model: String,
    /// Gemini delivery style.
    pub direction: String,
    /// Default Voice Profile for VoxCPM2 (last used); projects may override it.
    pub voxcpm_profile: String,
}

impl Default for VoiceDefaults {
    fn default() -> Self {
        Self {
            engine: EngineKind::Edge,
            edge_voice_km: "km-KH-SreymomNeural".into(),
            edge_voice_en: "en-US-AvaNeural".into(),
            rate: 0,
            pitch: 0,
            gemini_voice: "Kore".into(),
            gemini_model: ModelSettings::default().tts,
            direction: DEFAULT_VOICE_DIRECTION.into(),
            voxcpm_profile: String::new(),
        }
    }
}

impl VoiceDefaults {
    /// The concrete voice for a project in `language`.
    pub fn resolve(&self, language: Language) -> VoiceSettings {
        let voice_name = match (self.engine, language) {
            (EngineKind::Edge, Language::Km) => self.edge_voice_km.clone(),
            (EngineKind::Edge, Language::En) => self.edge_voice_en.clone(),
            (EngineKind::Gemini, _) => self.gemini_voice.clone(),
            // Replaced by the profile's settings when the engine is built (`commands::voices`).
            (EngineKind::Voxcpm, _) => self.voxcpm_profile.clone(),
        };
        VoiceSettings {
            engine: self.engine,
            model: self.gemini_model.clone(),
            voice_name,
            direction: self.direction.trim().to_owned(),
            rate: self.rate,
            pitch: self.pitch,
        }
    }

    fn validate(&self) -> Result<(), StorageError> {
        let bad = |what: &str| Err(StorageError::InvalidSetting(what.to_owned()));
        if !voices::is_edge_voice(&self.edge_voice_km) || !self.edge_voice_km.starts_with("km-") {
            return bad("Khmer Edge voice");
        }
        if !voices::is_edge_voice(&self.edge_voice_en) || !self.edge_voice_en.starts_with("en-") {
            return bad("English Edge voice");
        }
        if !voices::is_gemini_voice(&self.gemini_voice) {
            return bad("Gemini voice");
        }
        if self.gemini_model.is_empty() || !is_model_id(&self.gemini_model) {
            return bad("Gemini TTS model");
        }
        if !(-50..=50).contains(&self.rate) || !(-50..=50).contains(&self.pitch) {
            return bad("rate and pitch must be between -50 and +50");
        }
        if self.direction.chars().count() > 300 {
            return bad("voice direction is too long");
        }
        if self.voxcpm_profile.len() > 40
            || !self.voxcpm_profile.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return bad("voice profile");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ModelSettings {
    /// Text models in fallback order. Each gets 3 attempts (429 / 5xx / timeouts retried),
    /// then the next one is tried. Old `text` / `text_fallback` fields are ignored on load.
    pub text_chain: Vec<String>,
    /// Reads "From document" PDFs (10 §3). Free-tier models only; Flash, because the
    /// Flash-Lite models invented facts on a scanned Khmer test page (2026-10-17).
    pub document: Vec<String>,
    pub tts: String,
    pub tts_lite: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ConcurrencySettings {
    pub text: usize,
    pub tts: usize,
    pub image: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            ui_language: None,
            theme: None,
            output_dir: None,
            models: ModelSettings::default(),
            concurrency: ConcurrencySettings::default(),
            voice: VoiceDefaults::default(),
            image: ImageSettings::default(),
            render: RenderPrefs::default(),
            gpu: None,
            local: LocalPrefs::default(),
        }
    }
}

impl ModelSettings {
    /// Script models: "From document" projects try the document models first, because the
    /// Flash-Lite models changed a number from the paper (28.4 → "29.4" BLEU, 2026-10-17).
    pub fn script_chain(&self, from_document: bool) -> Vec<String> {
        let mut chain: Vec<String> = if from_document { self.document.clone() } else { vec![] };
        for m in &self.text_chain {
            if !chain.contains(m) {
                chain.push(m.clone());
            }
        }
        chain
    }
}

// Defaults from the Gemini models page (checked 2026-10-15). Users can change them in Settings.
impl Default for ModelSettings {
    fn default() -> Self {
        Self {
            text_chain: vec![
                "gemini-3.5-flash-lite".into(),
                "gemini-3.1-flash-lite".into(),
                "gemini-3.8-flash".into(),
            ],
            document: vec!["gemini-3.8-flash".into()],
            tts: "gemini-3.8-flash-tts".into(),
            tts_lite: "gemini-3.8-flash-lite-tts".into(),
        }
    }
}

impl Default for ConcurrencySettings {
    fn default() -> Self {
        Self {
            text: 3,
            tts: 3,
            image: 3,
        }
    }
}

const MAX_CONCURRENCY: usize = 8;
const MAX_TEXT_CHAIN: usize = 6;

fn is_model_id(m: &str) -> bool {
    m.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '_')
}

impl Settings {
    /// Rejects values that would break requests or overload a weak laptop.
    pub fn validate(&self) -> Result<(), StorageError> {
        if let Some(lang) = &self.ui_language
            && lang != "km"
            && lang != "en"
        {
            return Err(StorageError::InvalidSetting(format!(
                "ui_language '{lang}'"
            )));
        }
        if let Some(theme) = &self.theme
            && theme != "light"
            && theme != "dark"
        {
            return Err(StorageError::InvalidSetting(format!("theme '{theme}'")));
        }
        let chain = &self.models.text_chain;
        if chain.is_empty() || chain.len() > MAX_TEXT_CHAIN {
            return Err(StorageError::InvalidSetting(format!(
                "the text model list needs 1–{MAX_TEXT_CHAIN} models"
            )));
        }
        for (i, m) in chain.iter().enumerate() {
            if chain[..i].contains(m) {
                return Err(StorageError::InvalidSetting(format!(
                    "model '{m}' is listed twice"
                )));
            }
        }
        let models = chain
            .iter()
            .chain([&self.models.tts, &self.models.tts_lite]);
        for m in models {
            if m.is_empty() || !is_model_id(m) {
                return Err(StorageError::InvalidSetting(format!("model id '{m}'")));
            }
        }
        self.voice.validate()?;
        self.image.validate()?;
        let c = &self.concurrency;
        for n in [c.text, c.tts, c.image] {
            if n == 0 || n > MAX_CONCURRENCY {
                return Err(StorageError::InvalidSetting(format!(
                    "concurrency must be 1–{MAX_CONCURRENCY}"
                )));
            }
        }
        Ok(())
    }
}

pub fn load(paths: &AppPaths) -> Result<Settings, StorageError> {
    let settings: Settings = atomic::read_json(&paths.settings_file())?.unwrap_or_default();
    Ok(settings)
}

pub fn save(paths: &AppPaths, settings: &Settings) -> Result<(), StorageError> {
    settings.validate()?;
    atomic::write_json(&paths.settings_file(), settings)
}

/// Applies a partial update (only the fields one Settings section changed) on top of the
/// current settings, so a section saved from an older copy cannot undo changes made
/// elsewhere (a finished model download, another section). Objects merge field by field;
/// anything else, `null` included, replaces the old value.
pub fn merged(current: &Settings, patch: &serde_json::Value) -> Result<Settings, StorageError> {
    let mut value = serde_json::to_value(current)?;
    merge(&mut value, patch);
    serde_json::from_value(value).map_err(|e| StorageError::InvalidSetting(e.to_string()))
}

fn merge(target: &mut serde_json::Value, patch: &serde_json::Value) {
    match (target, patch) {
        (serde_json::Value::Object(t), serde_json::Value::Object(p)) => {
            for (k, v) in p {
                match t.get_mut(k) {
                    Some(old) if old.is_object() && v.is_object() => merge(old, v),
                    _ => {
                        t.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (t, p) => *t = p.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_projects_write_scripts_with_the_document_model_first() {
        let m = ModelSettings::default();
        assert_eq!(m.script_chain(false), m.text_chain);
        let doc = m.script_chain(true);
        assert_eq!(doc[0], "gemini-3.8-flash");
        assert_eq!(doc.len(), m.text_chain.len(), "no duplicates");
    }

    #[test]
    fn a_patch_changes_only_its_own_fields() {
        let mut current = Settings::default();
        current.local.offer_dismissed = true;
        current.image.local_profile = Some("flux_schnell_gguf".into());
        let patch = serde_json::json!({ "local": { "cooldown_every": 7 }, "output_dir": "D:/videos" });
        let s = merged(&current, &patch).unwrap();
        assert_eq!(s.local.cooldown_every, 7);
        assert!(s.local.offer_dismissed, "a field the section did not touch survives");
        assert_eq!(s.image.local_profile.as_deref(), Some("flux_schnell_gguf"));
        assert_eq!(s.output_dir.as_deref(), Some("D:/videos"));
        let cleared = merged(&s, &serde_json::json!({ "output_dir": null })).unwrap();
        assert_eq!(cleared.output_dir, None);
    }

    #[test]
    fn a_patch_with_a_wrong_type_is_rejected() {
        assert!(merged(&Settings::default(), &serde_json::json!({ "local": { "cooldown_every": "x" } })).is_err());
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        // Old files with `text` / `text_fallback` load with the new default chain.
        let s: Settings = serde_json::from_str(
            r#"{"ui_language":"km","models":{"text":"gemini-3.8-flash","text_fallback":"x"}}"#,
        )
        .unwrap();
        assert_eq!(s.ui_language.as_deref(), Some("km"));
        assert_eq!(s.models, ModelSettings::default());
        assert_eq!(s.models.text_chain[0], "gemini-3.5-flash-lite");
        assert!(s.validate().is_ok());
    }

    #[test]
    fn rejects_bad_values() {
        let mut s = Settings::default();
        s.models.text_chain[0] = "gemini/../x".into();
        assert!(s.validate().is_err());
        let mut s = Settings::default();
        s.concurrency.tts = 0;
        assert!(s.validate().is_err());
        let s = Settings { theme: Some("blue".into()), ..Settings::default() };
        assert!(s.validate().is_err());
        let s = Settings { theme: Some("dark".into()), ..Settings::default() };
        assert!(s.validate().is_ok());
        let mut s = Settings::default();
        s.models.text_chain.clear();
        assert!(s.validate().is_err(), "chain must not be empty");
        let mut s = Settings::default();
        s.models.text_chain.push("gemini-3.8-flash".into());
        assert!(s.validate().is_err(), "duplicates rejected");
        let mut s = Settings::default();
        s.models.text_chain[1] = "bad id!".into();
        assert!(s.validate().is_err());
    }

    #[test]
    fn voice_defaults_resolve_per_language() {
        let mut v = VoiceDefaults::default();
        assert_eq!(v.resolve(Language::Km).voice_name, "km-KH-SreymomNeural");
        assert_eq!(v.resolve(Language::En).voice_name, "en-US-AvaNeural");
        v.engine = EngineKind::Gemini;
        assert_eq!(v.resolve(Language::Km).voice_name, "Kore");
        assert!(v.validate().is_ok());
        v.edge_voice_km = "en-US-AvaNeural".into();
        assert!(v.validate().is_err(), "Khmer slot needs a Khmer voice");
        let v = VoiceDefaults {
            rate: 80,
            ..VoiceDefaults::default()
        };
        assert!(v.validate().is_err());
    }

    #[test]
    fn image_settings_validation() {
        let mut s = ImageSettings::default();
        assert!(s.validate().is_ok());
        s.steps = 9;
        assert!(s.validate().is_err());
        let mut s = ImageSettings {
            cloudflare_account_id: "0123456789abcdef0123456789abcdef".into(),
            ..ImageSettings::default()
        };
        assert!(s.validate().is_ok());
        s.cloudflare_account_id = "not-an-id".into();
        assert!(s.validate().is_err());
    }

    #[test]
    fn save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::new(dir.path().to_path_buf());
        let s = Settings {
            ui_language: Some("en".into()),
            ..Settings::default()
        };
        save(&paths, &s).unwrap();
        assert_eq!(load(&paths).unwrap(), s);
    }
}
