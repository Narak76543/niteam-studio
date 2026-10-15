//! FFmpeg arguments for one scene clip (03 Step 6a): slow pan over the enlarged image with a
//! time-based `crop` (never `zoompan`), subtitle PNGs overlaid at their times, voice + 0.25 s
//! of silence. Pure function, unit-tested.
//!
//! Clips are Matroska with H.264 + PCM audio: identical parameters for every clip, and no AAC
//! priming gaps, so the concat step can join them seamlessly (audio is encoded once, there).

/// Silence after each scene's voice so scenes don't run into each other.
pub const TAIL_PAD_MS: u64 = 250;
pub const FPS: u32 = 30;

/// Output frame and subtitle band for a resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// Subtitle PNG height (full frame width); drawn by `src/lib/subtitleCanvas.ts`.
    pub subs_height: u32,
    /// Gap between the subtitle band and the bottom edge.
    pub subs_margin: u32,
}

impl Frame {
    pub fn for_resolution(resolution: &str) -> Self {
        if resolution == "720p" {
            Self { width: 1280, height: 720, subs_height: 160, subs_margin: 16 }
        } else {
            Self { width: 1920, height: 1080, subs_height: 240, subs_margin: 24 }
        }
    }

    pub fn subs_y(self) -> u32 {
        self.height - self.subs_height - self.subs_margin
    }
}

/// Pan direction; alternates per scene so the motion doesn't feel repetitive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pan {
    LeftToRight,
    RightToLeft,
    TopToBottom,
    BottomToTop,
}

impl Pan {
    /// Cycle of four. Consecutive scenes 2k / 2k+1 always move in opposite directions, so the
    /// second scene of a shared image (2 scenes per image, 07 §5) pans back the other way.
    pub fn for_index(index: usize) -> Self {
        match index % 4 {
            0 => Self::LeftToRight,
            1 => Self::RightToLeft,
            2 => Self::TopToBottom,
            _ => Self::BottomToTop,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::LeftToRight => "lr",
            Self::RightToLeft => "rl",
            Self::TopToBottom => "tb",
            Self::BottomToTop => "bt",
        }
    }

    /// `crop` x/y expressions; `d` = clip duration in seconds.
    fn expr(self, d: &str) -> (String, String) {
        let fwd = |axis: &str| format!("({axis})*t/{d}");
        let back = |axis: &str| format!("({axis})*(1-t/{d})");
        let mid = |axis: &str| format!("({axis})/2");
        match self {
            Self::LeftToRight => (fwd("iw-ow"), mid("ih-oh")),
            Self::RightToLeft => (back("iw-ow"), mid("ih-oh")),
            Self::TopToBottom => (mid("iw-ow"), fwd("ih-oh")),
            Self::BottomToTop => (mid("iw-ow"), back("ih-oh")),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Overlay {
    /// Relative to the project folder.
    pub path: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

#[derive(Debug, Clone)]
pub struct ClipSpec {
    /// Pan-size image (`scenes/<id>/image_1080.png`), relative to the project folder.
    pub image: String,
    pub audio: String,
    pub voice_ms: u64,
    pub subs: Vec<Overlay>,
    pub pan: Pan,
    pub frame: Frame,
    /// `Encoder::args()`.
    pub encoder_args: Vec<String>,
    pub output: String,
}

impl ClipSpec {
    pub fn duration_ms(&self) -> u64 {
        self.voice_ms + TAIL_PAD_MS
    }
}

fn secs(ms: u64) -> String {
    format!("{}.{:03}", ms / 1000, ms % 1000)
}

/// Full argument list (after the runner's logging/progress flags).
pub fn clip_args(spec: &ClipSpec) -> Vec<String> {
    let d = secs(spec.duration_ms());
    let f = spec.frame;
    let mut a: Vec<String> = Vec::new();
    let push = |a: &mut Vec<String>, items: &[&str]| a.extend(items.iter().map(|s| (*s).to_owned()));

    push(&mut a, &["-y", "-loop", "1", "-framerate", &FPS.to_string(), "-t", &d, "-i", &spec.image]);
    push(&mut a, &["-i", &spec.audio]);
    for s in &spec.subs {
        push(&mut a, &["-i", &s.path]);
    }

    let (x, y) = spec.pan.expr(&d);
    let mut graph = format!("[0:v]crop=w={}:h={}:x='{x}':y='{y}'", f.width, f.height);
    let mut last = "bg".to_owned();
    graph.push_str(&format!("[{last}]"));
    for (i, s) in spec.subs.iter().enumerate() {
        let next = format!("s{i}");
        graph.push_str(&format!(
            ";[{last}][{}:v]overlay=x=0:y={}:enable='between(t,{},{})'[{next}]",
            i + 2,
            f.subs_y(),
            secs(s.start_ms),
            secs(s.end_ms),
        ));
        last = next;
    }
    graph.push_str(&format!(";[{last}]format=yuv420p[v];[1:a]apad=pad_dur={}[a]", secs(TAIL_PAD_MS)));

    push(&mut a, &["-filter_complex", &graph, "-map", "[v]", "-map", "[a]", "-t", &d]);
    a.extend(spec.encoder_args.iter().cloned());
    push(&mut a, &["-r", &FPS.to_string(), "-g", "60"]);
    push(&mut a, &["-c:a", "pcm_s16le", "-ar", "48000", "-ac", "2"]);
    push(&mut a, &["-f", "matroska", &spec.output]);
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(subs: Vec<Overlay>, pan: Pan) -> ClipSpec {
        ClipSpec {
            image: "scenes/s001/image_1080.png".into(),
            audio: "scenes/s001/voice.mp3".into(),
            voice_ms: 6150,
            subs,
            pan,
            frame: Frame::for_resolution("1080p"),
            encoder_args: vec!["-c:v".into(), "libx264".into()],
            output: "scenes/s001/clip.tmp.mkv".into(),
        }
    }

    fn graph(a: &[String]) -> &str {
        let i = a.iter().position(|x| x == "-filter_complex").unwrap();
        &a[i + 1]
    }

    #[test]
    fn duration_includes_tail_pad() {
        let a = clip_args(&spec(vec![], Pan::LeftToRight));
        assert_eq!(a.iter().filter(|x| *x == "6.400").count(), 2, "input -t and output -t");
        assert!(graph(&a).contains("apad=pad_dur=0.250"));
    }

    #[test]
    fn pan_uses_time_based_crop_not_zoompan() {
        let a = clip_args(&spec(vec![], Pan::RightToLeft));
        let g = graph(&a);
        assert!(g.contains("crop=w=1920:h=1080:x='(iw-ow)*(1-t/6.400)':y='(ih-oh)/2'"), "{g}");
        assert!(!g.contains("zoompan"));
    }

    #[test]
    fn overlays_chain_with_times_and_band_position() {
        let subs = vec![
            Overlay { path: "scenes/s001/subs/000.png".into(), start_ms: 0, end_ms: 2400 },
            Overlay { path: "scenes/s001/subs/001.png".into(), start_ms: 2400, end_ms: 6150 },
        ];
        let a = clip_args(&spec(subs, Pan::TopToBottom));
        let g = graph(&a);
        assert!(g.contains("[bg][2:v]overlay=x=0:y=816:enable='between(t,0.000,2.400)'[s0]"), "{g}");
        assert!(g.contains("[s0][3:v]overlay=x=0:y=816:enable='between(t,2.400,6.150)'[s1]"), "{g}");
        assert!(g.contains("[s1]format=yuv420p[v]"));
        let inputs: Vec<_> = a.windows(2).filter(|w| w[0] == "-i").map(|w| w[1].clone()).collect();
        assert_eq!(inputs.len(), 4);
    }

    #[test]
    fn identical_stream_parameters_for_concat() {
        let a = clip_args(&spec(vec![], Pan::BottomToTop));
        for w in [["-c:a", "pcm_s16le"], ["-ar", "48000"], ["-ac", "2"], ["-r", "30"], ["-f", "matroska"]] {
            assert!(a.windows(2).any(|x| x[0] == w[0] && x[1] == w[1]), "{w:?}");
        }
        assert_eq!(a.last().map(String::as_str), Some("scenes/s001/clip.tmp.mkv"));
    }

    #[test]
    fn pans_alternate_and_pairs_are_opposite() {
        assert_eq!(Pan::for_index(0), Pan::LeftToRight);
        assert_eq!(Pan::for_index(1), Pan::RightToLeft);
        assert_eq!(Pan::for_index(2), Pan::TopToBottom);
        assert_eq!(Pan::for_index(3), Pan::BottomToTop);
        assert_eq!(Pan::for_index(4), Pan::LeftToRight);
    }

    #[test]
    fn subtitle_band_fits_the_frame() {
        let f = Frame::for_resolution("1080p");
        assert_eq!(f.subs_y(), 816);
        let f = Frame::for_resolution("720p");
        assert_eq!((f.width, f.height, f.subs_y()), (1280, 720, 544));
    }
}
