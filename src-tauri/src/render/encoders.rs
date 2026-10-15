//! H.264 encoder detection (03 "Encoder detection"): encode 1 s of `testsrc` with each
//! encoder in order and keep the first that works. The result is cached in settings.

use std::path::Path;

use tokio_util::sync::CancellationToken;

use super::RenderError;
use super::ffmpeg::Ffmpeg;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoder {
    Qsv,
    Amf,
    Nvenc,
    X264,
}

/// Detection order: hardware encoders first (Intel, AMD, NVIDIA), software last.
pub const ORDER: [Encoder; 4] = [Encoder::Qsv, Encoder::Amf, Encoder::Nvenc, Encoder::X264];

impl Encoder {
    pub fn name(self) -> &'static str {
        match self {
            Self::Qsv => "h264_qsv",
            Self::Amf => "h264_amf",
            Self::Nvenc => "h264_nvenc",
            Self::X264 => "libx264",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        ORDER.into_iter().find(|e| e.name() == name)
    }

    /// `-c:v` plus quality options.
    pub fn args(self) -> Vec<String> {
        let opts: &[&str] = match self {
            Self::Qsv => &["-preset", "veryfast", "-global_quality", "23"],
            Self::Amf => &["-quality", "speed", "-rc", "cqp", "-qp_i", "22", "-qp_p", "24"],
            Self::Nvenc => &["-preset", "p4", "-cq", "23"],
            Self::X264 => &["-preset", "veryfast", "-crf", "23"],
        };
        let mut v = vec!["-c:v".to_owned(), self.name().to_owned()];
        v.extend(opts.iter().map(|s| (*s).to_owned()));
        v
    }
}

fn probe_args(enc: Encoder) -> Vec<String> {
    let mut v: Vec<String> = [
        "-f", "lavfi", "-i", "testsrc=size=1920x1080:rate=30", "-t", "1", "-pix_fmt", "yuv420p",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    v.extend(enc.args());
    v.extend(["-f", "null", "-"].iter().map(|s| (*s).to_owned()));
    v
}

/// First encoder that can encode 1 s of 1080p on this machine.
pub async fn detect(ffmpeg: &Ffmpeg, cwd: &Path, cancel: &CancellationToken) -> Result<Encoder, RenderError> {
    for enc in ORDER {
        match ffmpeg.run(&probe_args(enc), cwd, cancel, |_| {}).await {
            Ok(()) => return Ok(enc),
            Err(RenderError::Cancelled) => return Err(RenderError::Cancelled),
            Err(_) => {}
        }
    }
    Err(RenderError::NoEncoder)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for e in ORDER {
            assert_eq!(Encoder::from_name(e.name()), Some(e));
        }
        assert_eq!(Encoder::from_name("h265"), None);
    }

    #[test]
    fn probe_encodes_to_null() {
        let a = probe_args(Encoder::Nvenc);
        assert!(a.windows(2).any(|w| w[0] == "-c:v" && w[1] == "h264_nvenc"));
        assert_eq!(a.last().map(String::as_str), Some("-"));
    }
}
