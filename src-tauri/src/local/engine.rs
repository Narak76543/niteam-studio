//! stable-diffusion.cpp `sd-server` (`docs/08-local-gpu.md` §3, persistent mode): started
//! once per generation run with the model loaded on the chosen GPU, one async job per image
//! (`POST /sdcpp/v1/img_gen` → poll `GET /sdcpp/v1/jobs/{id}`, cancel with
//! `POST /sdcpp/v1/jobs/{id}/cancel`). API checked against `examples/server/api.md` at the
//! pinned release (master-929-3f8527a).

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio_util::sync::CancellationToken;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(windows)]
const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;

/// Loading SD 1.5 from disk takes a few seconds; a cold start on a slow HDD much longer.
const START_TIMEOUT: Duration = Duration::from_secs(180);
const POLL: Duration = Duration::from_millis(250);
/// One 768×448 LCM image takes seconds; this only catches a hung engine.
const JOB_TIMEOUT: Duration = Duration::from_secs(300);
const LOG_TAIL: usize = 2_000;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("The local image engine was not found ({0}). Reinstall Nitean Studio or switch to Cloudflare in Settings → Images.")]
    Missing(String),
    #[error("GPU ran out of memory. Try a smaller model profile or close other apps.")]
    OutOfMemory(String),
    #[error("The local image engine stopped: {0}. Switch to Cloudflare in Settings → Images if this keeps happening.")]
    Crashed(String),
    #[error("The local image engine did not answer: {0}")]
    Http(String),
    #[error("Cancelled")]
    Cancelled,
}

impl EngineError {
    fn from_log(log: &str, fallback: String) -> Self {
        let l = log.to_ascii_lowercase();
        if l.contains("out of memory") || l.contains("outofdevicememory") || l.contains("erroroutofdevicememory") || l.contains("failed to allocate") {
            Self::OutOfMemory(tail(log))
        } else {
            Self::Crashed(if log.trim().is_empty() { fallback } else { tail(log) })
        }
    }
}

fn tail(s: &str) -> String {
    let s = s.trim();
    let start = s.char_indices().rev().nth(LOG_TAIL).map_or(0, |(i, _)| i);
    s[start..].to_owned()
}

/// `sd-server --list-devices` lines: `Vulkan1\tNVIDIA GeForce RTX 4050 Laptop GPU`.
pub fn parse_devices(out: &str) -> Vec<(String, String)> {
    out.lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(n, d)| (n.trim().to_owned(), d.trim().to_owned()))
        .filter(|(n, _)| !n.is_empty() && !n.contains(' '))
        .collect()
}

/// Vulkan device whose description matches the detected GPU name (hybrid laptops list the
/// integrated GPU first, so "the first GPU" would be the wrong one).
pub fn device_for(devices: &[(String, String)], gpu_name: &str) -> Option<String> {
    let want = gpu_name.to_ascii_lowercase();
    devices
        .iter()
        .filter(|(n, _)| n.to_ascii_lowercase().starts_with("vulkan"))
        .find(|(_, d)| {
            let d = d.to_ascii_lowercase();
            d == want || d.contains(&want) || want.contains(&d)
        })
        .map(|(n, _)| n.clone())
}

#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// `…/sdcpp/sd-server.exe` (its DLLs sit next to it).
    pub exe: PathBuf,
    pub model: PathBuf,
    /// Folder with the profile's LoRA files.
    pub lora_dir: PathBuf,
    /// Vulkan device name from `--list-devices` (e.g. `Vulkan1`).
    pub device: Option<String>,
}

/// One image request (sizes multiple of 64; LoRA paths relative to `lora_dir`).
#[derive(Debug, Clone, Serialize)]
pub struct GenRequest {
    pub prompt: String,
    pub negative_prompt: String,
    pub width: u32,
    pub height: u32,
    pub seed: i64,
    pub steps: u32,
    pub cfg: f32,
    pub sampler: String,
    pub scheduler: Option<String>,
    pub vae_tiling: bool,
    /// (file name inside `lora_dir`, multiplier)
    pub loras: Vec<(String, f32)>,
}

/// Body for `POST /sdcpp/v1/img_gen` (fields from the server's api.md).
pub fn request_body(r: &GenRequest) -> serde_json::Value {
    let mut sample = json!({
        "sample_method": r.sampler,
        "sample_steps": r.steps,
        "guidance": { "txt_cfg": r.cfg }
    });
    if let Some(s) = &r.scheduler {
        sample["scheduler"] = json!(s);
    }
    json!({
        "prompt": r.prompt,
        "negative_prompt": r.negative_prompt,
        "width": r.width,
        "height": r.height,
        "seed": r.seed,
        "batch_count": 1,
        "embed_image_metadata": false,
        "sample_params": sample,
        "lora": r.loras.iter().map(|(p, m)| json!({ "path": p, "multiplier": m, "is_high_noise": false })).collect::<Vec<_>>(),
        "vae_tiling_params": { "enabled": r.vae_tiling },
        "output_format": "png"
    })
}

#[derive(Debug, Deserialize)]
struct Submitted {
    id: String,
}

#[derive(Debug, Deserialize)]
struct Job {
    status: String,
    result: Option<JobResult>,
    error: Option<JobErrorBody>,
}

#[derive(Debug, Deserialize)]
struct JobResult {
    images: Vec<JobImage>,
}

#[derive(Debug, Deserialize)]
struct JobImage {
    b64_json: String,
}

#[derive(Debug, Deserialize)]
struct JobErrorBody {
    #[serde(default)]
    code: String,
    #[serde(default)]
    message: String,
}

pub struct SdServer {
    child: Child,
    base: String,
    http: reqwest::Client,
    log: std::sync::Arc<tokio::sync::Mutex<String>>,
}

fn free_port() -> Result<u16, EngineError> {
    let l = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| EngineError::Crashed(e.to_string()))?;
    Ok(l.local_addr().map_err(|e| EngineError::Crashed(e.to_string()))?.port())
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

/// `sd-server --list-devices` → (name, description).
pub async fn list_devices(exe: &Path) -> Result<Vec<(String, String)>, EngineError> {
    if !exe.is_file() {
        return Err(EngineError::Missing(exe.display().to_string()));
    }
    let out = command(exe)
        .arg("--list-devices")
        .output()
        .await
        .map_err(|e| EngineError::Crashed(e.to_string()))?;
    Ok(parse_devices(&String::from_utf8_lossy(&out.stdout)))
}

impl SdServer {
    /// Starts the server with the model loaded and waits until it answers.
    pub async fn start(cfg: &EngineConfig, cancel: &CancellationToken) -> Result<Self, EngineError> {
        if !cfg.exe.is_file() {
            return Err(EngineError::Missing(cfg.exe.display().to_string()));
        }
        let port = free_port()?;
        let mut cmd = command(&cfg.exe);
        cmd.arg("--listen-ip")
            .arg("127.0.0.1")
            .arg("--listen-port")
            .arg(port.to_string())
            .arg("-m")
            .arg(&cfg.model)
            .arg("--lora-model-dir")
            .arg(&cfg.lora_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(dev) = &cfg.device {
            cmd.arg("--backend").arg(dev);
        }
        let mut child = cmd.spawn().map_err(|e| EngineError::Crashed(e.to_string()))?;
        let log = std::sync::Arc::new(tokio::sync::Mutex::new(String::new()));
        let streams: [Option<Box<dyn tokio::io::AsyncRead + Unpin + Send>>; 2] = [
            child.stdout.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
            child.stderr.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
        ];
        // Keep the last part of the server's output for error messages.
        for mut stream in streams.into_iter().flatten() {
            let log = log.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                while let Ok(n) = stream.read(&mut buf).await {
                    if n == 0 {
                        break;
                    }
                    let mut l = log.lock().await;
                    l.push_str(&String::from_utf8_lossy(&buf[..n]));
                    if l.len() > 64 * 1024 {
                        let from = l.len() - 16 * 1024;
                        let cut = (from..l.len()).find(|&i| l.is_char_boundary(i)).unwrap_or(l.len());
                        l.drain(..cut);
                    }
                }
            });
        }
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| EngineError::Http(e.to_string()))?;
        let mut server = Self { child, base: format!("http://127.0.0.1:{port}"), http, log };
        server.wait_ready(cancel).await?;
        Ok(server)
    }

    async fn wait_ready(&mut self, cancel: &CancellationToken) -> Result<(), EngineError> {
        let started = Instant::now();
        let url = format!("{}/sdcpp/v1/capabilities", self.base);
        loop {
            if cancel.is_cancelled() {
                return Err(EngineError::Cancelled);
            }
            if let Ok(r) = self.http.get(&url).send().await
                && r.status().is_success()
            {
                return Ok(());
            }
            if let Ok(Some(status)) = self.child.try_wait() {
                // Let the reader tasks drain the last output.
                tokio::time::sleep(Duration::from_millis(200)).await;
                let log = self.log.lock().await.clone();
                return Err(EngineError::from_log(&log, format!("exit {status}")));
            }
            if started.elapsed() > START_TIMEOUT {
                return Err(EngineError::Http("the model did not load within 3 minutes".into()));
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
    }

    /// Generates one image; PNG bytes.
    pub async fn generate(&mut self, req: &GenRequest, cancel: &CancellationToken) -> Result<Vec<u8>, EngineError> {
        let resp = self
            .http
            .post(format!("{}/sdcpp/v1/img_gen", self.base))
            .json(&request_body(req))
            .send()
            .await
            .map_err(|e| self.dead_or(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(EngineError::Http(format!("HTTP {status}: {}", tail(&body))));
        }
        let job: Submitted = resp.json().await.map_err(|e| EngineError::Http(e.to_string()))?;
        let started = Instant::now();
        loop {
            if cancel.is_cancelled() {
                let _ = self.http.post(format!("{}/sdcpp/v1/jobs/{}/cancel", self.base, job.id)).send().await;
                return Err(EngineError::Cancelled);
            }
            if started.elapsed() > JOB_TIMEOUT {
                return Err(EngineError::Http("the image took longer than 5 minutes".into()));
            }
            tokio::time::sleep(POLL).await;
            let r = match self.http.get(format!("{}/sdcpp/v1/jobs/{}", self.base, job.id)).send().await {
                Ok(r) => r,
                Err(e) => return Err(self.dead_or(e.to_string())),
            };
            let j: Job = r.json().await.map_err(|e| EngineError::Http(e.to_string()))?;
            match j.status.as_str() {
                "completed" => {
                    let b64 = j
                        .result
                        .and_then(|r| r.images.into_iter().next())
                        .map(|i| i.b64_json)
                        .ok_or_else(|| EngineError::Http("no image in the result".into()))?;
                    return base64::engine::general_purpose::STANDARD
                        .decode(b64.as_bytes())
                        .map_err(|e| EngineError::Http(e.to_string()));
                }
                "failed" => {
                    let e = j.error.unwrap_or(JobErrorBody { code: String::new(), message: String::new() });
                    let log = self.log.lock().await.clone();
                    let msg = format!("{} {}", e.code, e.message);
                    return Err(EngineError::from_log(&format!("{msg}\n{log}"), msg));
                }
                "cancelled" => return Err(EngineError::Cancelled),
                _ => {}
            }
        }
    }

    fn dead_or(&mut self, msg: String) -> EngineError {
        match self.child.try_wait() {
            Ok(Some(status)) => {
                let log = self.log.try_lock().map(|l| l.clone()).unwrap_or_default();
                EngineError::from_log(&log, format!("exit {status}"))
            }
            _ => EngineError::Http(msg),
        }
    }

    pub async fn stop(mut self) {
        let _ = self.child.kill().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEVICES: &str = "Vulkan0\tIntel(R) UHD Graphics\nVulkan1\tNVIDIA GeForce RTX 4050 Laptop GPU\nCPU\t12th Gen Intel(R) Core(TM) i7-12650H\n";

    #[test]
    fn picks_the_vulkan_device_of_the_detected_gpu() {
        let d = parse_devices(DEVICES);
        assert_eq!(d.len(), 3);
        assert_eq!(device_for(&d, "NVIDIA GeForce RTX 4050 Laptop GPU").as_deref(), Some("Vulkan1"));
        assert_eq!(device_for(&d, "Intel(R) UHD Graphics").as_deref(), Some("Vulkan0"));
        assert_eq!(device_for(&d, "AMD Radeon RX 6600"), None);
    }

    #[test]
    fn log_lines_without_tabs_are_ignored() {
        let out = "load_backend: loaded Vulkan backend from x.dll\nVulkan0\tGPU\n";
        assert_eq!(parse_devices(out), vec![("Vulkan0".to_owned(), "GPU".to_owned())]);
    }

    #[test]
    fn body_matches_the_server_api() {
        let b = request_body(&GenRequest {
            prompt: "a cat".into(),
            negative_prompt: "text".into(),
            width: 768,
            height: 448,
            seed: 42,
            steps: 6,
            cfg: 1.5,
            sampler: "lcm".into(),
            scheduler: None,
            vae_tiling: true,
            loras: vec![("lcm-lora-sdv1-5.safetensors".into(), 1.0)],
        });
        assert_eq!(b["sample_params"]["sample_method"], "lcm");
        assert_eq!(b["sample_params"]["guidance"]["txt_cfg"], 1.5);
        assert_eq!(b["lora"][0]["path"], "lcm-lora-sdv1-5.safetensors");
        assert_eq!(b["vae_tiling_params"]["enabled"], true);
        assert_eq!((b["width"].as_u64(), b["height"].as_u64()), (Some(768), Some(448)));
        assert!(b["sample_params"].get("scheduler").is_none());
    }

    #[test]
    fn out_of_memory_is_recognised() {
        assert!(matches!(
            EngineError::from_log("ggml_vulkan: Device memory allocation of size 123 failed.\nErrorOutOfDeviceMemory", "x".into()),
            EngineError::OutOfMemory(_)
        ));
        assert!(matches!(EngineError::from_log("segfault", "x".into()), EngineError::Crashed(_)));
    }
}
