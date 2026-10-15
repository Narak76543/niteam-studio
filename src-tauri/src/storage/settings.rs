//! `settings.json`: everything except the API key. Model IDs live here, not in code.

use serde::{Deserialize, Serialize};

use super::{AppPaths, StorageError, atomic};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub schema_version: u32,
    /// "km" | "en"; `None` = follow the Windows display language.
    pub ui_language: Option<String>,
    /// Where finished videos are copied. `None` = inside the project folder.
    pub output_dir: Option<String>,
    pub models: ModelSettings,
    pub concurrency: ConcurrencySettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ModelSettings {
    pub text: String,
    /// Used once per job when `text` is still overloaded (503) after all retries.
    /// Empty = no fallback.
    pub text_fallback: String,
    pub tts: String,
    pub tts_lite: String,
    pub image: String,
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
            output_dir: None,
            models: ModelSettings::default(),
            concurrency: ConcurrencySettings::default(),
        }
    }
}

// Defaults from the Gemini models page (checked 2026-10-15). Users can change them in Settings.
impl Default for ModelSettings {
    fn default() -> Self {
        Self {
            text: "gemini-3.8-flash".into(),
            text_fallback: "gemini-3.6-flash".into(),
            tts: "gemini-3.8-flash-tts".into(),
            tts_lite: "gemini-3.8-flash-lite-tts".into(),
            image: "gemini-3.1-flash-image".into(),
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
        let models = [
            &self.models.text,
            &self.models.tts,
            &self.models.tts_lite,
            &self.models.image,
        ];
        for m in models {
            if m.is_empty() || !is_model_id(m) {
                return Err(StorageError::InvalidSetting(format!("model id '{m}'")));
            }
        }
        let fb = &self.models.text_fallback;
        if !fb.is_empty() && !is_model_id(fb) {
            return Err(StorageError::InvalidSetting(format!("model id '{fb}'")));
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let s: Settings = serde_json::from_str(r#"{"ui_language":"km"}"#).unwrap();
        assert_eq!(s.ui_language.as_deref(), Some("km"));
        assert_eq!(s.models, ModelSettings::default());
        assert!(s.validate().is_ok());
    }

    #[test]
    fn rejects_bad_values() {
        let mut s = Settings::default();
        s.models.text = "gemini/../x".into();
        assert!(s.validate().is_err());
        let mut s = Settings::default();
        s.concurrency.tts = 0;
        assert!(s.validate().is_err());
        let mut s = Settings::default();
        s.models.text_fallback = String::new();
        assert!(s.validate().is_ok(), "empty fallback = disabled");
        s.models.text_fallback = "bad id!".into();
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
