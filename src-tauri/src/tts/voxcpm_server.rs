//! audio.cpp `audiocpp_server` running VoxCPM2 (`docs/09-voice-profiles.md`). One shared
//! process for the whole app (voice jobs, voice design, previews): started on first use with
//! the model loaded on the chosen Vulkan device, model unloaded after `IDLE_UNLOAD_MS` without
//! requests (frees VRAM for image generation), stopped before a local image job.
//!
//! Request fields verified against the pinned v0.9.0 server (spike 2026-10-16): `input`
//! (a leading `(style)` is voice design / style control), `seed`, `num_inference_steps`,
//! `guidance_scale`, `voice_ref` (file path), `reference_text`. Unknown fields are silently
//! ignored by the server (`cfg_value`, `inference_timesteps`, `instruct` have no effect), so
//! the mapping is pinned by tests.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde_json::json;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(windows)]
const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;

const START_TIMEOUT: Duration = Duration::from_secs(240);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);
/// Unload the model after 2 minutes without requests (next request reloads in ~5 s).
pub const IDLE_UNLOAD_MS: u64 = 120_000;
const MODEL_ID: &str = "voxcpm2";

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum VoxError {
    #[error("The local voice engine was not found ({0}). Reinstall Nitean Studio or choose Edge / Gemini TTS in Settings → Voice.")]
    Missing(String),
    #[error("Install the VoxCPM2 voice model in Settings → Voices first.")]
    ModelMissing,
    #[error("The GPU ran out of memory for this sentence.")]
    OutOfMemory(String),
    #[error("The local voice engine stopped: {0}")]
    Crashed(String),
    #[error("The local voice engine did not answer: {0}")]
    Http(String),
    #[error("Cancelled")]
    Cancelled,
}

/// What to run: the bundled server, the model file and the Vulkan device index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerSpec {
    pub exe: PathBuf,
    pub model: PathBuf,
    pub device: u32,
    /// Where the generated `server.json` and `server.log` go.
    pub work_dir: PathBuf,
}

/// One speech request.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeakRequest {
    /// Text, optionally starting with `(style / voice description)`.
    pub text: String,
    pub seed: u64,
    pub steps: u32,
    pub guidance: f32,
    pub voice_ref: Option<PathBuf>,
    pub reference_text: Option<String>,
}

pub fn request_body(r: &SpeakRequest) -> serde_json::Value {
    let mut b = json!({
        "model": MODEL_ID,
        "input": r.text,
        "seed": r.seed,
        "num_inference_steps": r.steps,
        "guidance_scale": r.guidance,
    });
    if let Some(p) = &r.voice_ref {
        b["voice_ref"] = json!({ "type": "path", "path": p.display().to_string() });
        if let Some(t) = r.reference_text.as_deref().filter(|t| !t.trim().is_empty()) {
            b["reference_text"] = json!(t);
        }
    }
    b
}

/// `--list-devices` lines: `Vulkan:1 "NVIDIA GeForce RTX 4050 Laptop GPU" [GPU]`.
pub fn parse_devices(out: &str) -> Vec<(u32, String, bool)> {
    out.lines()
        .filter_map(|l| {
            let rest = l.trim().strip_prefix("Vulkan:")?;
            let (idx, rest) = rest.split_once(' ')?;
            let name = rest.split('"').nth(1)?.to_owned();
            Some((idx.parse().ok()?, name, rest.contains("[GPU]")))
        })
        .collect()
}

/// The device matching the detected GPU name, else the first discrete GPU, else 0.
pub fn pick_device(devices: &[(u32, String, bool)], gpu_name: Option<&str>) -> u32 {
    let by_name = gpu_name.and_then(|n| {
        let n = n.to_ascii_lowercase();
        devices.iter().find(|(_, d, _)| d.to_ascii_lowercase() == n).map(|d| d.0)
    });
    by_name.or_else(|| devices.iter().find(|d| d.2).map(|d| d.0)).unwrap_or(0)
}

fn command(exe: &Path) -> Command {
    let mut cmd = Command::new(exe);
    if let Some(dir) = exe.parent() {
        cmd.current_dir(dir);
    }
    cmd.stdin(Stdio::null()).kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS);
    cmd
}

pub async fn list_devices(exe: &Path) -> Vec<(u32, String, bool)> {
    match command(exe).args(["--backend", "vulkan", "--list-devices"]).output().await {
        Ok(out) => parse_devices(&String::from_utf8_lossy(&out.stdout)),
        Err(_) => Vec::new(),
    }
}

fn classify(status: u16, body: &str) -> VoxError {
    let b = body.to_ascii_lowercase();
    if b.contains("failed to allocate") || b.contains("out of memory") || b.contains("outofdevicememory") || b.contains("insufficient_memory") {
        VoxError::OutOfMemory(body.chars().take(300).collect())
    } else {
        VoxError::Http(format!("HTTP {status}: {}", body.chars().take(300).collect::<String>()))
    }
}

struct Running {
    child: Child,
    base: String,
    spec: ServerSpec,
}

/// The app-wide server handle.
pub struct VoxServer {
    inner: Mutex<Option<Running>>,
    http: reqwest::Client,
}

impl Default for VoxServer {
    fn default() -> Self {
        Self {
            inner: Mutex::new(None),
            http: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .timeout(REQUEST_TIMEOUT)
                .build()
                .unwrap_or_default(),
        }
    }
}

impl VoxServer {
    /// Stops the process (before a local image job, or on app exit).
    pub async fn stop(&self) {
        if let Some(mut r) = self.inner.lock().await.take() {
            let _ = r.child.kill().await;
        }
    }

    pub async fn is_running(&self) -> bool {
        self.inner.lock().await.is_some()
    }

    async fn start(&self, spec: &ServerSpec, cancel: &CancellationToken) -> Result<Running, VoxError> {
        if !spec.exe.is_file() {
            return Err(VoxError::Missing(spec.exe.display().to_string()));
        }
        if !spec.model.is_file() {
            return Err(VoxError::ModelMissing);
        }
        std::fs::create_dir_all(&spec.work_dir).map_err(|e| VoxError::Crashed(e.to_string()))?;
        let port = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| VoxError::Crashed(e.to_string()))?;
            l.local_addr().map_err(|e| VoxError::Crashed(e.to_string()))?.port()
        };
        let cfg = json!({
            "host": "127.0.0.1",
            "port": port,
            "backend": "vulkan",
            "device": spec.device,
            "threads": 4,
            "lazy_load": false,
            "idle_unload_ms": IDLE_UNLOAD_MS,
            "models": [{ "id": MODEL_ID, "family": "voxcpm2", "path": spec.model.display().to_string(),
                         "task": "tts", "mode": "offline" }]
        });
        let cfg_path = spec.work_dir.join("server.json");
        std::fs::write(&cfg_path, cfg.to_string()).map_err(|e| VoxError::Crashed(e.to_string()))?;
        let log = std::fs::File::create(spec.work_dir.join("server.log")).map_err(|e| VoxError::Crashed(e.to_string()))?;
        let log2 = log.try_clone().map_err(|e| VoxError::Crashed(e.to_string()))?;
        let mut child = command(&spec.exe)
            .arg("--config")
            .arg(&cfg_path)
            .arg("--no-ui")
            .stdout(log)
            .stderr(log2)
            .spawn()
            .map_err(|e| VoxError::Crashed(e.to_string()))?;
        let base = format!("http://127.0.0.1:{port}");
        let started = Instant::now();
        loop {
            if cancel.is_cancelled() {
                let _ = child.kill().await;
                return Err(VoxError::Cancelled);
            }
            if let Ok(r) = self.http.get(format!("{base}/v1/models")).timeout(Duration::from_secs(2)).send().await
                && r.status().is_success()
            {
                return Ok(Running { child, base, spec: spec.clone() });
            }
            if let Ok(Some(status)) = child.try_wait() {
                let tail = std::fs::read_to_string(spec.work_dir.join("server.log")).unwrap_or_default();
                let tail: String = tail.chars().rev().take(600).collect::<Vec<_>>().into_iter().rev().collect();
                return Err(VoxError::Crashed(format!("exit {status}: {tail}")));
            }
            if started.elapsed() > START_TIMEOUT {
                let _ = child.kill().await;
                return Err(VoxError::Http("the voice model did not load within 4 minutes".into()));
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
    }

    /// Synthesizes one request; WAV bytes. Starts (or restarts) the server when needed.
    pub async fn speak(&self, spec: &ServerSpec, req: &SpeakRequest, cancel: &CancellationToken) -> Result<Vec<u8>, VoxError> {
        let mut slot = self.inner.lock().await;
        let alive = match slot.as_mut() {
            Some(r) => r.spec == *spec && matches!(r.child.try_wait(), Ok(None)),
            None => false,
        };
        if !alive {
            if let Some(mut old) = slot.take() {
                let _ = old.child.kill().await;
            }
            *slot = Some(self.start(spec, cancel).await?);
        }
        let Some(running) = slot.as_mut() else {
            return Err(VoxError::Crashed("not started".into()));
        };
        let send = self.http.post(format!("{}/v1/audio/speech", running.base)).json(&request_body(req)).send();
        let resp = tokio::select! {
            () = cancel.cancelled() => return Err(VoxError::Cancelled),
            r = send => r,
        };
        let resp = match resp {
            Ok(r) => r,
            Err(e) => {
                // The process may have died mid-request.
                if let Ok(Some(status)) = running.child.try_wait() {
                    slot.take();
                    return Err(VoxError::Crashed(format!("exit {status}")));
                }
                return Err(VoxError::Http(e.to_string()));
            }
        };
        let status = resp.status();
        let bytes = resp.bytes().await.map_err(|e| VoxError::Http(e.to_string()))?;
        if status.is_success() {
            Ok(bytes.to_vec())
        } else {
            Err(classify(status.as_u16(), &String::from_utf8_lossy(&bytes)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_uses_the_field_names_the_server_honours() {
        let b = request_body(&SpeakRequest {
            text: "(calm)សួស្តី។".into(),
            seed: 7,
            steps: 10,
            guidance: 2.0,
            voice_ref: Some(PathBuf::from("C:/v/ref.wav")),
            reference_text: Some("transcript".into()),
        });
        assert_eq!(b["input"], "(calm)សួស្តី។");
        assert_eq!(b["num_inference_steps"], 10);
        assert_eq!(b["guidance_scale"], 2.0);
        assert_eq!(b["voice_ref"]["type"], "path");
        assert_eq!(b["reference_text"], "transcript");
        assert!(b.get("cfg_value").is_none() && b.get("inference_timesteps").is_none());
    }

    #[test]
    fn no_reference_text_without_a_reference_clip() {
        let b = request_body(&SpeakRequest {
            text: "x".into(),
            seed: 1,
            steps: 10,
            guidance: 2.0,
            voice_ref: None,
            reference_text: Some("ignored".into()),
        });
        assert!(b.get("voice_ref").is_none() && b.get("reference_text").is_none());
    }

    #[test]
    fn picks_the_nvidia_device_on_a_hybrid_laptop() {
        let out = "available_devices=3\nVulkan:0 \"Intel(R) UHD Graphics\" [IGPU]\nVulkan:1 \"NVIDIA GeForce RTX 4050 Laptop GPU\" [GPU]\nCPU:0 \"12th Gen Intel\" [CPU]\n";
        let d = parse_devices(out);
        assert_eq!(d.len(), 2);
        assert_eq!(pick_device(&d, Some("NVIDIA GeForce RTX 4050 Laptop GPU")), 1);
        assert_eq!(pick_device(&d, None), 1, "first discrete GPU");
        assert_eq!(pick_device(&[], None), 0);
    }

    #[test]
    fn out_of_memory_is_recognised() {
        let e = classify(500, r#"{"error":{"message":"failed to allocate VoxCPM2 AudioVAE graph"}}"#);
        assert!(matches!(e, VoxError::OutOfMemory(_)));
        assert!(matches!(classify(503, "busy"), VoxError::Http(_)));
    }
}
