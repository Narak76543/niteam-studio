//! Resumable model download (`docs/08-local-gpu.md` §5): stream to `<file>.part` with HTTP
//! Range, verify sha256, then rename. A wrong hash deletes the part file (`corrupt`).

use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("Download failed: {0}")]
    Network(String),
    #[error("The server answered HTTP {0}.")]
    Http(u16),
    #[error("The downloaded file is damaged (checksum mismatch). It was deleted; try again.")]
    Corrupt,
    #[error("Not enough free disk space: {need_mb} MB needed, {free_mb} MB free.")]
    NoSpace { need_mb: u64, free_mb: u64 },
    #[error("Download cancelled.")]
    Cancelled,
    #[error("Could not write the file: {0}")]
    Io(String),
}

impl From<std::io::Error> for DownloadError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

pub fn part_path(dest: &Path) -> PathBuf {
    let mut s = dest.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

/// sha256 of a file, hex (runs on a blocking thread: model files are ~2 GB).
pub async fn file_sha256(path: &Path) -> Result<String, DownloadError> {
    let path = path.to_owned();
    tokio::task::spawn_blocking(move || -> Result<String, DownloadError> {
        let mut f = std::fs::File::open(&path)?;
        let mut h = Sha256::new();
        std::io::copy(&mut f, &mut h)?;
        Ok(hex::encode(h.finalize()))
    })
    .await
    .map_err(|e| DownloadError::Io(e.to_string()))?
}

pub fn http_client() -> Result<reqwest::Client, DownloadError> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        // No total timeout (files are large); a stalled connection is caught per chunk.
        .user_agent(concat!("NiteanStudio/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| DownloadError::Network(e.to_string()))
}

const CHUNK_TIMEOUT: Duration = Duration::from_secs(60);

/// Downloads `url` to `dest` (resuming a `.part` file), verifies `sha256`, renames.
/// `on_progress(done_bytes, total_bytes)`.
pub async fn download(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    sha256: &str,
    cancel: &CancellationToken,
    mut on_progress: impl FnMut(u64, Option<u64>),
) -> Result<(), DownloadError> {
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let part = part_path(dest);
    let have = tokio::fs::metadata(&part).await.map(|m| m.len()).unwrap_or(0);

    let mut req = client.get(url);
    if have > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={have}-"));
    }
    let mut resp = tokio::select! {
        () = cancel.cancelled() => return Err(DownloadError::Cancelled),
        r = req.send() => r.map_err(|e| DownloadError::Network(e.to_string()))?,
    };
    let status = resp.status().as_u16();
    let (mut file, mut done) = match status {
        // Resumed: append.
        206 => (tokio::fs::OpenOptions::new().append(true).open(&part).await?, have),
        // Full body (server ignored the range): start over.
        200 => (tokio::fs::File::create(&part).await?, 0),
        // Range past the end: the part file is already complete.
        416 if have > 0 => {
            return finish(&part, dest, sha256).await;
        }
        s => return Err(DownloadError::Http(s)),
    };
    let total = resp.content_length().map(|len| len + done);
    on_progress(done, total);

    loop {
        let chunk = tokio::select! {
            () = cancel.cancelled() => {
                file.flush().await?;
                return Err(DownloadError::Cancelled);
            }
            c = tokio::time::timeout(CHUNK_TIMEOUT, resp.chunk()) => match c {
                Err(_) => return Err(DownloadError::Network("no data for 60 s".into())),
                Ok(r) => r.map_err(|e| DownloadError::Network(e.to_string()))?,
            },
        };
        let Some(bytes) = chunk else { break };
        file.write_all(&bytes).await?;
        done += bytes.len() as u64;
        on_progress(done, total);
    }
    file.flush().await?;
    drop(file);
    finish(&part, dest, sha256).await
}

async fn finish(part: &Path, dest: &Path, sha256: &str) -> Result<(), DownloadError> {
    if !file_sha256(part).await?.eq_ignore_ascii_case(sha256) {
        let _ = tokio::fs::remove_file(part).await;
        return Err(DownloadError::Corrupt);
    }
    if tokio::fs::metadata(dest).await.is_ok() {
        tokio::fs::remove_file(dest).await?;
    }
    tokio::fs::rename(part, dest).await?;
    Ok(())
}

/// Free bytes on the volume holding `dir` (Windows); `None` if unknown.
#[cfg(windows)]
pub fn free_space(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    use windows::core::PCWSTR;

    let mut probe = dir.to_owned();
    while !probe.exists() {
        probe = probe.parent()?.to_owned();
    }
    let wide: Vec<u16> = probe.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut free = 0u64;
    // SAFETY: `wide` is a NUL-terminated path; the out pointer is valid for the call.
    unsafe { GetDiskFreeSpaceExW(PCWSTR(wide.as_ptr()), Some(&mut free), None, None) }.ok()?;
    Some(free)
}

#[cfg(not(windows))]
pub fn free_space(_dir: &Path) -> Option<u64> {
    None
}

/// 08 §5: need the download size + 10 %.
pub fn check_space(dir: &Path, need_bytes: u64) -> Result<(), DownloadError> {
    let need = need_bytes + need_bytes / 10;
    match free_space(dir) {
        Some(free) if free < need => Err(DownloadError::NoSpace {
            need_mb: need / (1024 * 1024),
            free_mb: free / (1024 * 1024),
        }),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Minimal HTTP server for one file that honours `Range: bytes=N-`.
    async fn serve(body: Vec<u8>, honour_range: bool) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else { return };
                let body = body.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_lowercase();
                    let from = req
                        .lines()
                        .find_map(|l| l.strip_prefix("range: bytes="))
                        .and_then(|r| r.trim_end_matches('-').trim().trim_end_matches('-').parse::<usize>().ok())
                        .filter(|_| honour_range);
                    let (status, slice) = match from {
                        Some(f) if f >= body.len() => ("416 Range Not Satisfiable", Vec::new()),
                        Some(f) => ("206 Partial Content", body[f..].to_vec()),
                        None => ("200 OK", body.clone()),
                    };
                    let head = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", slice.len());
                    let _ = sock.write_all(head.as_bytes()).await;
                    let _ = sock.write_all(&slice).await;
                });
            }
        });
        format!("http://{addr}/model.safetensors")
    }

    fn sha(b: &[u8]) -> String {
        hex::encode(Sha256::digest(b))
    }

    fn body() -> Vec<u8> {
        (0..200_000u32).map(|i| (i % 251) as u8).collect()
    }

    #[tokio::test]
    async fn downloads_and_verifies() {
        let data = body();
        let url = serve(data.clone(), true).await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("m/model.safetensors");
        let mut last = (0, None);
        download(&http_client().unwrap(), &url, &dest, &sha(&data), &CancellationToken::new(), |d, t| last = (d, t))
            .await
            .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), data);
        assert!(!part_path(&dest).exists());
        assert_eq!(last, (data.len() as u64, Some(data.len() as u64)));
    }

    #[tokio::test]
    async fn resumes_from_a_part_file() {
        let data = body();
        let url = serve(data.clone(), true).await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("model.safetensors");
        std::fs::write(part_path(&dest), &data[..70_000]).unwrap();
        let mut first = None;
        download(&http_client().unwrap(), &url, &dest, &sha(&data), &CancellationToken::new(), |d, _| {
            first.get_or_insert(d);
        })
        .await
        .unwrap();
        assert_eq!(first, Some(70_000), "continued where it stopped");
        assert_eq!(std::fs::read(&dest).unwrap(), data);
    }

    #[tokio::test]
    async fn server_without_range_support_restarts_cleanly() {
        let data = body();
        let url = serve(data.clone(), false).await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("model.safetensors");
        std::fs::write(part_path(&dest), b"stale partial bytes").unwrap();
        download(&http_client().unwrap(), &url, &dest, &sha(&data), &CancellationToken::new(), |_, _| {})
            .await
            .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), data);
    }

    #[tokio::test]
    async fn wrong_checksum_is_corrupt_and_removed() {
        let data = body();
        let url = serve(data.clone(), true).await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("model.safetensors");
        let err = download(&http_client().unwrap(), &url, &dest, &sha(b"other"), &CancellationToken::new(), |_, _| {})
            .await
            .unwrap_err();
        assert!(matches!(err, DownloadError::Corrupt), "{err:?}");
        assert!(!dest.exists() && !part_path(&dest).exists());
    }

    #[tokio::test]
    async fn complete_part_file_is_just_verified() {
        let data = body();
        let url = serve(data.clone(), true).await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("model.safetensors");
        std::fs::write(part_path(&dest), &data).unwrap();
        download(&http_client().unwrap(), &url, &dest, &sha(&data), &CancellationToken::new(), |_, _| {})
            .await
            .unwrap();
        assert!(dest.is_file());
    }

    #[test]
    fn space_check_has_ten_percent_margin() {
        let dir = tempfile::tempdir().unwrap();
        assert!(check_space(dir.path(), 1024).is_ok());
        if free_space(dir.path()).is_some() {
            assert!(matches!(check_space(dir.path(), u64::MAX / 2), Err(DownloadError::NoSpace { .. })));
        }
    }
}
