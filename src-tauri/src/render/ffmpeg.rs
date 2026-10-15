//! Runs the FFmpeg sidecar: below-normal priority, no console window, `-progress` parsing,
//! cancellation (kills the child) and the last lines of stderr for error messages.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use super::RenderError;

/// Characters of stderr kept for the error message.
const STDERR_TAIL: usize = 1_500;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(windows)]
const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;

#[derive(Debug, Clone)]
pub struct Ffmpeg {
    path: PathBuf,
}

impl Ffmpeg {
    /// Tauri copies `externalBin` sidecars next to the app executable without the target
    /// triple (`ffmpeg.exe`), both in `tauri dev` and in the installed app.
    pub fn locate() -> Result<Self, RenderError> {
        let exe = std::env::current_exe()?;
        let dir = exe
            .parent()
            .ok_or_else(|| RenderError::FfmpegMissing(exe.display().to_string()))?;
        let path = dir.join(if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" });
        if path.is_file() {
            Ok(Self { path })
        } else {
            Err(RenderError::FfmpegMissing(path.display().to_string()))
        }
    }

    #[cfg(test)]
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    /// Runs FFmpeg in `cwd` with `args` (input/output options only; logging and progress
    /// flags are added here). `on_progress` gets the output position in milliseconds.
    pub async fn run(
        &self,
        args: &[String],
        cwd: &Path,
        cancel: &CancellationToken,
        mut on_progress: impl FnMut(u64),
    ) -> Result<(), RenderError> {
        let mut cmd = Command::new(&self.path);
        cmd.args(["-hide_banner", "-nostdin", "-loglevel", "error", "-nostats"])
            .args(["-progress", "pipe:1"])
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS);

        let mut child = cmd.spawn()?;
        let stdout = child.stdout.take().ok_or_else(|| RenderError::Io("no stdout".into()))?;
        let mut stderr = child.stderr.take().ok_or_else(|| RenderError::Io("no stderr".into()))?;
        let stderr_task = tokio::spawn(async move {
            let mut buf = Vec::new();
            let _ = stderr.read_to_end(&mut buf).await;
            String::from_utf8_lossy(&buf).into_owned()
        });

        let mut lines = BufReader::new(stdout).lines();
        let status = loop {
            tokio::select! {
                () = cancel.cancelled() => {
                    let _ = child.kill().await;
                    return Err(RenderError::Cancelled);
                }
                line = lines.next_line() => match line? {
                    Some(l) => {
                        if let Some(ms) = parse_progress(&l) {
                            on_progress(ms);
                        }
                    }
                    None => break child.wait().await?,
                },
            }
        };
        let stderr = stderr_task.await.unwrap_or_default();
        if status.success() {
            Ok(())
        } else {
            Err(RenderError::Ffmpeg {
                code: status.code().unwrap_or(-1),
                tail: tail(&stderr),
            })
        }
    }
}

/// `out_time_us=1234567` (microseconds, despite FFmpeg also printing `out_time_ms` in µs)
/// → milliseconds. `N/A` before the first frame is ignored.
fn parse_progress(line: &str) -> Option<u64> {
    let v = line.strip_prefix("out_time_us=")?;
    v.trim().parse::<u64>().ok().map(|us| us / 1000)
}

fn tail(stderr: &str) -> String {
    let s = stderr.trim();
    let start = s
        .char_indices()
        .rev()
        .nth(STDERR_TAIL)
        .map_or(0, |(i, _)| i);
    s[start..].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_progress_lines() {
        assert_eq!(parse_progress("out_time_us=2500000"), Some(2500));
        assert_eq!(parse_progress("out_time_us=N/A"), None);
        assert_eq!(parse_progress("frame=12"), None);
    }

    #[test]
    fn keeps_the_end_of_stderr() {
        let long = "x".repeat(5000) + "the real error";
        let t = tail(&long);
        assert!(t.ends_with("the real error"));
        assert!(t.chars().count() <= STDERR_TAIL + 1);
    }

    #[tokio::test]
    async fn missing_binary_is_an_io_error() {
        let ff = Ffmpeg::at(PathBuf::from("Z:/definitely/not/ffmpeg.exe"));
        let dir = tempfile::tempdir().unwrap();
        let err = ff
            .run(&[], dir.path(), &CancellationToken::new(), |_| {})
            .await
            .unwrap_err();
        assert!(matches!(err, RenderError::Io(_)), "{err:?}");
    }
}
