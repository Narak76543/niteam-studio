//! Progress events shared by the voice and image jobs (`scene-progress`, `generate-progress`).

use serde::Serialize;

use super::project::AssetStatus;

/// Payload of `scene-progress`.
#[derive(Debug, Clone, Serialize)]
pub struct SceneProgress {
    pub project_id: String,
    pub scene_id: String,
    /// "voice" | "image"
    pub asset: &'static str,
    pub status: AssetStatus,
    pub duration_ms: Option<u64>,
    pub error: Option<String>,
    pub error_kind: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    Voice,
    Image,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GenerateStage {
    Running,
    Done,
    Cancelled,
    Failed,
    /// Image job only: daily free allowance used up (07 §6).
    PausedQuota,
}

/// Payload of `generate-progress`.
#[derive(Debug, Clone, Serialize)]
pub struct GenerateProgress {
    pub project_id: String,
    pub job: JobKind,
    pub stage: GenerateStage,
    pub done: usize,
    pub failed: usize,
    pub total: usize,
    /// Job-level error (e.g. Edge TTS unavailable, bad Cloudflare token): raw detail + kind.
    pub error: Option<String>,
    pub error_kind: Option<String>,
    /// `PausedQuota`: when the allowance resets (RFC 3339 UTC).
    pub resets_at_utc: Option<String>,
}

pub trait JobEvents: Send + Sync {
    fn scene(&self, progress: SceneProgress);
    fn progress(&self, progress: GenerateProgress);
}
