//! Progress events shared by the voice and image jobs (`scene-progress`, `generate-progress`).

use serde::Serialize;

use super::project::AssetStatus;

/// Payload of `scene-progress`.
#[derive(Debug, Clone, Serialize)]
pub struct SceneProgress {
    pub project_id: String,
    pub scene_id: String,
    /// "voice" | "image" | "visual"
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
    /// Real-world photos / clips search + download (`visual_job`).
    Visual,
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

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Mutex;

    use super::*;

    /// Collects every event (tests of the jobs).
    #[derive(Default)]
    pub struct Recorder {
        pub scenes: Mutex<Vec<SceneProgress>>,
        pub progress: Mutex<Vec<GenerateProgress>>,
    }

    impl JobEvents for Recorder {
        fn scene(&self, p: SceneProgress) {
            self.scenes.lock().unwrap().push(p);
        }
        fn progress(&self, p: GenerateProgress) {
            self.progress.lock().unwrap().push(p);
        }
    }
}
