//! Local model profiles on disk (`docs/08-local-gpu.md` §5): status, install (resumable,
//! sha256-verified downloads), verify, delete. A profile is usable only when every file is
//! there — files are only renamed into place after their hash matched.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use super::download::{self, DownloadError, part_path};
use super::manifest::{FileRole, Manifest, Profile};

#[derive(Debug, Clone, Serialize)]
pub struct FileInfo {
    pub name: String,
    pub role: FileRole,
    pub size_mb: u64,
    pub license: String,
    pub license_url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProfileStatus {
    pub id: String,
    pub label: String,
    pub min_vram_mb: u64,
    pub total_mb: u64,
    /// Enough VRAM on this machine.
    pub fits: bool,
    pub installed: bool,
    /// Bytes already downloaded (finished files + `.part` files), in MB.
    pub downloaded_mb: u64,
    pub files: Vec<FileInfo>,
}

pub fn profile_dir(models_dir: &Path, profile: &Profile) -> PathBuf {
    models_dir.join(&profile.id)
}

pub fn file_path(models_dir: &Path, profile: &Profile, name: &str) -> PathBuf {
    profile_dir(models_dir, profile).join(name)
}

pub fn installed(models_dir: &Path, profile: &Profile) -> bool {
    profile.files.iter().all(|f| file_path(models_dir, profile, &f.name).is_file())
}

fn downloaded_bytes(models_dir: &Path, profile: &Profile) -> u64 {
    profile
        .files
        .iter()
        .map(|f| {
            let p = file_path(models_dir, profile, &f.name);
            std::fs::metadata(&p)
                .or_else(|_| std::fs::metadata(part_path(&p)))
                .map(|m| m.len())
                .unwrap_or(0)
        })
        .sum()
}

pub fn status(manifest: &Manifest, models_dir: &Path, vram_mb: Option<u64>) -> Vec<ProfileStatus> {
    manifest
        .profiles
        .iter()
        .map(|p| ProfileStatus {
            id: p.id.clone(),
            label: p.label.clone(),
            min_vram_mb: p.min_vram_mb,
            total_mb: p.total_mb(),
            fits: vram_mb.is_some_and(|v| v >= p.min_vram_mb),
            installed: installed(models_dir, p),
            downloaded_mb: downloaded_bytes(models_dir, p) / (1024 * 1024),
            files: p
                .files
                .iter()
                .map(|f| FileInfo {
                    name: f.name.clone(),
                    role: f.role,
                    size_mb: f.size_mb,
                    license: f.license.clone(),
                    license_url: f.license_url.clone(),
                })
                .collect(),
        })
        .collect()
}

/// Downloads every missing file. `on_progress(done_bytes, total_bytes)` over the whole profile.
pub async fn install(
    client: &reqwest::Client,
    models_dir: &Path,
    profile: &Profile,
    cancel: &CancellationToken,
    mut on_progress: impl FnMut(u64, u64),
) -> Result<(), DownloadError> {
    let total: u64 = profile.files.iter().map(|f| f.size_mb * 1024 * 1024).sum();
    let have = downloaded_bytes(models_dir, profile);
    download::check_space(models_dir, total.saturating_sub(have))?;
    let mut finished: u64 = 0;
    for f in &profile.files {
        let dest = file_path(models_dir, profile, &f.name);
        if dest.is_file() {
            finished += std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
            on_progress(finished, total);
            continue;
        }
        let base = finished;
        download::download(client, &f.url, &dest, &f.sha256, cancel, |done, _| on_progress(base + done, total)).await?;
        finished += std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
    }
    on_progress(total.max(finished), total.max(finished));
    Ok(())
}

/// Re-checks every file's sha256; a damaged file is deleted. `true` = all good.
pub async fn verify(models_dir: &Path, profile: &Profile) -> Result<bool, DownloadError> {
    let mut ok = true;
    for f in &profile.files {
        let p = file_path(models_dir, profile, &f.name);
        if !p.is_file() {
            ok = false;
            continue;
        }
        if !download::file_sha256(&p).await?.eq_ignore_ascii_case(&f.sha256) {
            tokio::fs::remove_file(&p).await?;
            ok = false;
        }
    }
    Ok(ok)
}

pub fn delete(models_dir: &Path, profile: &Profile) -> std::io::Result<()> {
    let dir = profile_dir(models_dir, profile);
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    Ok(())
}

/// Power state for the laptop hints (08 §8).
#[derive(Debug, Clone, Copy, Serialize, Default)]
pub struct Power {
    pub has_battery: bool,
    pub on_battery: bool,
}

#[cfg(windows)]
pub fn power() -> Power {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut s = SYSTEM_POWER_STATUS::default();
    // SAFETY: fills a plain struct.
    if unsafe { GetSystemPowerStatus(&mut s) }.is_err() {
        return Power::default();
    }
    // BatteryFlag 128 = no system battery, 255 = unknown; ACLineStatus 0 = offline.
    let has_battery = s.BatteryFlag != 128 && s.BatteryFlag != 255;
    Power { has_battery, on_battery: has_battery && s.ACLineStatus == 0 }
}

#[cfg(not(windows))]
pub fn power() -> Power {
    Power::default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::manifest::{GenParams, ModelFile};

    fn profile() -> Profile {
        let file = |role, name: &str| ModelFile {
            role,
            name: name.into(),
            url: "https://x/y".into(),
            sha256: "0".repeat(64),
            size_mb: 1,
            license: "L".into(),
            license_url: "https://x/l".into(),
        };
        Profile {
            id: "sd15_lcm".into(),
            label: "Fast".into(),
            min_vram_mb: 3800,
            files: vec![file(FileRole::Model, "m.safetensors"), file(FileRole::Lora, "l.safetensors")],
            generation: GenParams {
                width: 768,
                height: 448,
                steps: 6,
                cfg: 1.5,
                sampler: "lcm".into(),
                scheduler: None,
                vae_tiling: true,
                lora_multiplier: 1.0,
            },
            prompt_style: "sd15".into(),
            engine_args: vec![],
        }
    }

    #[test]
    fn installed_only_when_every_file_is_there() {
        let dir = tempfile::tempdir().unwrap();
        let p = profile();
        assert!(!installed(dir.path(), &p));
        std::fs::create_dir_all(profile_dir(dir.path(), &p)).unwrap();
        std::fs::write(file_path(dir.path(), &p, "m.safetensors"), vec![0u8; 2048]).unwrap();
        std::fs::write(part_path(&file_path(dir.path(), &p, "l.safetensors")), vec![0u8; 1024]).unwrap();
        assert!(!installed(dir.path(), &p), "a .part file does not count");
        let m = Manifest { engine_version: "x".into(), profiles: vec![p.clone()] };
        let s = &status(&m, dir.path(), Some(5920))[0];
        assert!(s.fits && !s.installed);
        assert_eq!(s.total_mb, 2);
        std::fs::write(file_path(dir.path(), &p, "l.safetensors"), b"x").unwrap();
        assert!(installed(dir.path(), &p));
        assert!(!status(&m, dir.path(), Some(2048))[0].fits);
    }

    #[tokio::test]
    async fn verify_deletes_damaged_files_and_delete_removes_the_profile() {
        let dir = tempfile::tempdir().unwrap();
        let p = profile();
        std::fs::create_dir_all(profile_dir(dir.path(), &p)).unwrap();
        for f in &p.files {
            std::fs::write(file_path(dir.path(), &p, &f.name), b"not the right bytes").unwrap();
        }
        assert!(!verify(dir.path(), &p).await.unwrap());
        assert!(!file_path(dir.path(), &p, "m.safetensors").exists());
        delete(dir.path(), &p).unwrap();
        assert!(!profile_dir(dir.path(), &p).exists());
    }
}
