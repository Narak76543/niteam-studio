//! Video rendering with the bundled FFmpeg sidecar (`docs/03-pipeline.md` Step 6).
//! Per-scene clips with pan motion + subtitle overlays, then one stream-copied concat.

pub mod clip;
pub mod concat;
pub mod encoders;
pub mod ffmpeg;

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("FFmpeg was not found next to the app ({0}). Reinstall Nitean Studio.")]
    FfmpegMissing(String),
    #[error("FFmpeg failed (exit code {code}): {tail}")]
    Ffmpeg { code: i32, tail: String },
    #[error("No working H.264 encoder was found on this computer.")]
    NoEncoder,
    #[error("Rendering was cancelled.")]
    Cancelled,
    #[error("{0}")]
    Io(String),
}

impl RenderError {
    /// Stable code for the UI's friendly message.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::FfmpegMissing(_) => "ffmpeg_missing",
            Self::Ffmpeg { .. } | Self::NoEncoder => "ffmpeg",
            Self::Cancelled => "cancelled",
            Self::Io(_) => "other",
        }
    }
}

impl From<std::io::Error> for RenderError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}
