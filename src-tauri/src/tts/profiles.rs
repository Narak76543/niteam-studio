//! Voice Profiles (`docs/09-voice-profiles.md`): reusable local voices, stored in app data
//! (`voices/<id>/profile.json` + `reference.wav` + `preview.wav`), not per project. Every
//! profile synthesizes by cloning its reference clip, so the voice stays the same across
//! scenes and projects; a transcript upgrades cloning to "ultimate" mode.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::pipeline::hashing::normalize;
use crate::pipeline::project::Language;
use crate::storage::{StorageError, atomic};

pub const REFERENCE_FILE: &str = "reference.wav";
pub const PREVIEW_FILE: &str = "preview.wav";
pub const DEFAULT_CFG: f32 = 2.0;
pub const DEFAULT_STEPS: u32 = 10;

/// How the profile was made. Synthesis always clones `reference.wav`; `ultimate` = with a
/// transcript of it.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProfileMode {
    /// From a text description (the chosen sample became the reference).
    Design,
    /// From a recording, no transcript.
    Clone,
    /// From a recording with its transcript.
    Ultimate,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VoiceProfile {
    pub id: String,
    pub name: String,
    pub language: Language,
    pub mode: ProfileMode,
    /// Voice description (design) or a note about the recording.
    #[serde(default)]
    pub description: String,
    /// File name inside the profile folder.
    pub reference_wav: String,
    /// Transcript of the reference clip ("ultimate" cloning); empty = plain cloning.
    #[serde(default)]
    pub reference_transcript: String,
    /// Delivery hint sent as a `(…)` prefix, e.g. "calm, warm, medium pace".
    #[serde(default)]
    pub style_hint: String,
    /// VoxCPM2 guidance strength (server `guidance_scale`).
    pub cfg_value: f32,
    /// Flow-matching steps (server `num_inference_steps`).
    pub inference_timesteps: u32,
    pub created_at: String,
    #[serde(default)]
    pub preview_wav: Option<String>,
    /// "From my recording": the user confirmed it is their own voice or one they may use.
    #[serde(default)]
    pub consent_at: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    #[error("Voice profile not found.")]
    NotFound,
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error("Could not write the voice files: {0}")]
    Io(String),
}

impl From<std::io::Error> for ProfileError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

fn is_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 40 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

impl VoiceProfile {
    /// Checks user-editable fields and clamps the numbers to safe ranges.
    pub fn validated(mut self) -> Result<Self, ProfileError> {
        self.name = normalize(&self.name);
        self.style_hint = normalize(&self.style_hint).replace(['(', ')'], "");
        self.reference_transcript = normalize(&self.reference_transcript);
        self.description = normalize(&self.description);
        if self.name.is_empty() || self.name.chars().count() > 60 {
            return Err(ProfileError::Invalid("Give the voice a name (up to 60 characters).".into()));
        }
        if self.style_hint.chars().count() > 200 || self.description.chars().count() > 500 {
            return Err(ProfileError::Invalid("The style hint or description is too long.".into()));
        }
        self.cfg_value = if self.cfg_value.is_finite() { self.cfg_value.clamp(1.0, 5.0) } else { DEFAULT_CFG };
        self.inference_timesteps = self.inference_timesteps.clamp(4, 30);
        Ok(self)
    }

    /// Changes whenever the voice would sound different (reference audio, transcript,
    /// style, settings). Part of every scene's voice hash.
    pub fn fingerprint(&self, dir: &Path) -> String {
        let mut h = Sha256::new();
        h.update(std::fs::read(dir.join(&self.reference_wav)).unwrap_or_default());
        for part in [&self.reference_transcript, &self.style_hint] {
            h.update(part.as_bytes());
            h.update([0x1f]);
        }
        h.update(self.cfg_value.to_le_bytes());
        h.update(self.inference_timesteps.to_le_bytes());
        hex::encode(&h.finalize()[..8])
    }
}

/// `voices/` in app data.
pub struct ProfileStore {
    root: PathBuf,
}

impl ProfileStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn dir(&self, id: &str) -> Result<PathBuf, ProfileError> {
        if !is_id(id) {
            return Err(ProfileError::NotFound);
        }
        Ok(self.root.join(id))
    }

    pub fn list(&self) -> Vec<VoiceProfile> {
        let Ok(rd) = std::fs::read_dir(&self.root) else {
            return Vec::new();
        };
        let mut out: Vec<VoiceProfile> = rd
            .flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| atomic::read_json::<VoiceProfile>(&e.path().join("profile.json")).ok().flatten())
            .collect();
        out.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        out
    }

    pub fn get(&self, id: &str) -> Result<VoiceProfile, ProfileError> {
        atomic::read_json::<VoiceProfile>(&self.dir(id)?.join("profile.json"))?.ok_or(ProfileError::NotFound)
    }

    pub fn save(&self, p: &VoiceProfile) -> Result<(), ProfileError> {
        let dir = self.dir(&p.id)?;
        std::fs::create_dir_all(&dir)?;
        atomic::write_json(&dir.join("profile.json"), p)?;
        Ok(())
    }

    /// Creates a profile from a prepared reference clip (moved into the profile folder).
    pub fn create(&self, mut p: VoiceProfile, reference: &[u8]) -> Result<VoiceProfile, ProfileError> {
        p.id = ulid::Ulid::new().to_string().to_ascii_lowercase();
        p.reference_wav = REFERENCE_FILE.into();
        p.preview_wav = None;
        let p = p.validated()?;
        let dir = self.dir(&p.id)?;
        std::fs::create_dir_all(&dir)?;
        atomic::write_bytes(&dir.join(REFERENCE_FILE), reference)?;
        self.save(&p)?;
        Ok(p)
    }

    pub fn duplicate(&self, id: &str, now: &str) -> Result<VoiceProfile, ProfileError> {
        let src = self.get(id)?;
        let reference = std::fs::read(self.dir(id)?.join(&src.reference_wav))?;
        let copy = VoiceProfile { name: format!("{} (copy)", src.name).chars().take(60).collect(), created_at: now.into(), ..src };
        self.create(copy, &reference)
    }

    pub fn delete(&self, id: &str) -> Result<(), ProfileError> {
        let dir = self.dir(id)?;
        if dir.exists() {
            std::fs::remove_dir_all(dir)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> VoiceProfile {
        VoiceProfile {
            id: String::new(),
            name: "  Narrator  ".into(),
            language: Language::Km,
            mode: ProfileMode::Design,
            description: "warm female storyteller".into(),
            reference_wav: String::new(),
            reference_transcript: "សួស្តី។".into(),
            style_hint: "(calm, medium pace)".into(),
            cfg_value: 99.0,
            inference_timesteps: 1,
            created_at: "2026-10-16T00:00:00Z".into(),
            preview_wav: None,
            consent_at: None,
        }
    }

    #[test]
    fn create_list_duplicate_delete() {
        let dir = tempfile::tempdir().unwrap();
        let store = ProfileStore::new(dir.path().to_path_buf());
        let p = store.create(profile(), b"RIFF-ref").unwrap();
        assert_eq!(p.name, "Narrator");
        assert_eq!(p.style_hint, "calm, medium pace", "parentheses would break the (style) prefix");
        assert_eq!((p.cfg_value, p.inference_timesteps), (5.0, 4), "clamped");
        assert!(store.dir(&p.id).unwrap().join(REFERENCE_FILE).is_file());
        let copy = store.duplicate(&p.id, "2026-10-16T01:00:00Z").unwrap();
        assert_eq!(copy.name, "Narrator (copy)");
        assert_ne!(copy.id, p.id);
        assert_eq!(store.list().len(), 2);
        store.delete(&p.id).unwrap();
        assert_eq!(store.list().len(), 1);
        assert!(matches!(store.get(&p.id), Err(ProfileError::NotFound)));
        assert!(matches!(store.get("../escape"), Err(ProfileError::NotFound)));
    }

    #[test]
    fn fingerprint_follows_audio_and_settings() {
        let dir = tempfile::tempdir().unwrap();
        let store = ProfileStore::new(dir.path().to_path_buf());
        let mut p = store.create(profile(), b"one").unwrap();
        let d = store.dir(&p.id).unwrap();
        let a = p.fingerprint(&d);
        p.style_hint = "fast".into();
        let b = p.fingerprint(&d);
        assert_ne!(a, b);
        std::fs::write(d.join(REFERENCE_FILE), b"two").unwrap();
        assert_ne!(p.fingerprint(&d), b);
    }

    #[test]
    fn rejects_an_empty_name() {
        let mut p = profile();
        p.name = " ".into();
        assert!(p.validated().is_err());
    }
}
