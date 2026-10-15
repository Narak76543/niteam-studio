//! Voice engines behind one trait.
//! - [`edge::EdgeTts`] — Microsoft Edge "Read aloud" (unofficial, free, see CLAUDE.md). MP3 +
//!   word timings. Default engine.
//! - [`gemini::GeminiTts`] — Gemini TTS via the user's key. WAV, no word timings.

pub mod audio;
pub mod chunker;
pub mod edge;
pub mod gemini;
pub mod profiles;
pub mod voices;
pub mod voxcpm;
pub mod voxcpm_server;
pub mod wav;

use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

use crate::gemini::GeminiError;
use crate::gemini::retry::RetryHooks;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum EngineKind {
    #[default]
    Edge,
    Gemini,
    /// VoxCPM2 on the user's GPU via audio.cpp (`docs/09-voice-profiles.md`).
    Voxcpm,
}

impl EngineKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Edge => "edge",
            Self::Gemini => "gemini",
            Self::Voxcpm => "voxcpm",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    Mp3,
    Wav,
}

impl AudioFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Mp3 => "mp3",
            Self::Wav => "wav",
        }
    }
}

/// One spoken word with its time span in the audio (from Edge WordBoundary events).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WordTiming {
    pub text: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

/// Everything an engine needs for one scene. Engines ignore fields they don't use
/// (Edge: rate/pitch; Gemini: model/style).
#[derive(Debug, Clone)]
pub struct TtsRequest {
    pub text: String,
    pub voice: String,
    /// Edge: speaking rate in percent (−50…+50).
    pub rate: i32,
    /// Edge: pitch in Hz (−50…+50).
    pub pitch: i32,
    /// Gemini: TTS model ID.
    pub model: String,
    /// Gemini: delivery style ("calm, warm storyteller, medium pace").
    pub style: String,
}

pub struct TtsOutput {
    pub audio: Vec<u8>,
    pub format: AudioFormat,
    /// Empty when the engine has no word timings (Gemini).
    pub words: Vec<WordTiming>,
}

#[derive(Debug, thiserror::Error)]
pub enum TtsError {
    #[error("Edge TTS unavailable — switch to Gemini TTS in Settings. ({0})")]
    EdgeUnavailable(String),
    #[error(transparent)]
    Gemini(#[from] GeminiError),
    #[error("The voice engine returned unusable audio: {0}")]
    BadAudio(String),
    /// Local voice engine missing / crashed / model not installed: stop the job.
    #[error("{0}")]
    LocalUnavailable(String),
    /// Local voice failed for this scene (e.g. out of GPU memory after splitting).
    #[error("{0}")]
    Local(String),
    #[error("Cancelled")]
    Cancelled,
}

impl TtsError {
    /// Stable code for the UI's friendly message.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::EdgeUnavailable(_) => "edge_unavailable",
            Self::Gemini(e) => e.kind(),
            Self::BadAudio(_) => "bad_response",
            Self::LocalUnavailable(_) => "local_voice_unavailable",
            Self::Local(_) => "local_voice",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn detail(&self) -> String {
        match self {
            Self::Gemini(e) => e.detail(),
            other => other.to_string(),
        }
    }
}

pub type TtsFuture<'a> = Pin<Box<dyn Future<Output = Result<TtsOutput, TtsError>> + Send + 'a>>;

pub trait TtsEngine: Send + Sync {
    /// Synthesizes one scene. Implementations retry transient failures themselves and
    /// stop when `hooks.cancel` fires.
    fn synthesize<'a>(&'a self, request: &'a TtsRequest, hooks: &'a RetryHooks) -> TtsFuture<'a>;
}
