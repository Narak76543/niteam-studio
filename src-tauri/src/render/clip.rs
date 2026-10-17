//! FFmpeg arguments for one scene clip (03 Step 6a): slow pan over the enlarged image with a
//! time-based `crop`, or for real-world scenes (`docs/11-real-world-visuals.md` §9) a gentle
//! `zoompan` on a photo prepared at frame × zoom, a still (diagram, code, small photo) or a
//! stock clip cover-cropped around the subject and looped when short; subtitle PNGs overlaid at
//! their times, voice + 0.25 s of silence. Pure function, unit-tested.
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

/// How a picture moves during its clip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Motion {
    /// AI images: pan over the pan-size image.
    Pan(Pan),
    /// Photos: zoom between 1 and `to`. The input is already frame × `to`, so the last frame
    /// is shown 1:1 — never enlarged past the photo's own pixels.
    Zoom { to: f32, zoom_in: bool },
    /// Diagrams, code, photos too small to zoom: frame-size input, no motion.
    Still,
}

impl Motion {
    /// Timeline / preview name: "lr" | "rl" | "tb" | "bt" | "zoom_in" | "zoom_out" | "still".
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pan(p) => p.as_str(),
            Self::Zoom { zoom_in: true, .. } => "zoom_in",
            Self::Zoom { zoom_in: false, .. } => "zoom_out",
            Self::Still => "still",
        }
    }

    /// Largest zoom factor (1.0 when the picture doesn't zoom).
    pub fn zoom(self) -> f32 {
        match self {
            Self::Zoom { to, .. } => to,
            _ => 1.0,
        }
    }
}

/// What the clip shows.
#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    /// A picture relative to the project folder.
    Image { path: String, motion: Motion },
    /// A stock clip (`assets/<id>.mp4`): looped when shorter than the scene, cover-cropped to
    /// the frame around `focus`, its own sound dropped.
    Video { path: String, focus: [f32; 2] },
    /// Frame-size pictures shown one after another (typed-out code): an `ffconcat` track.
    Frames { track: String },
}

#[derive(Debug, Clone)]
pub struct ClipSpec {
    pub input: Input,
    pub audio: String,
    pub voice_ms: u64,
    /// `scenes/<id>/subs/track.ffconcat` (timed subtitle images), relative to the project.
    pub subs_track: Option<String>,
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

    match &spec.input {
        Input::Image { path, .. } => push(&mut a, &["-y", "-loop", "1", "-framerate", &FPS.to_string(), "-t", &d, "-i", path]),
        // -stream_loop: a short clip starts again until the scene ends.
        Input::Video { path, .. } => push(&mut a, &["-y", "-stream_loop", "-1", "-t", &d, "-i", path]),
        Input::Frames { track } => push(&mut a, &["-y", "-f", "concat", "-safe", "0", "-i", track]),
    }
    push(&mut a, &["-i", &spec.audio]);
    if let Some(track) = &spec.subs_track {
        push(&mut a, &["-f", "concat", "-safe", "0", "-i", track]);
    }

    let (w, h) = (f.width, f.height);
    let mut graph = match &spec.input {
        Input::Image { motion: Motion::Pan(pan), .. } => {
            let (x, y) = pan.expr(&d);
            format!("[0:v]crop=w={w}:h={h}:x='{x}':y='{y}'[bg]")
        }
        Input::Image { motion: Motion::Zoom { to, zoom_in }, .. } => {
            // `on` = output frame number; the zoom reaches `to` (or 1) on the last frame.
            let last = ((spec.duration_ms() * u64::from(FPS)).div_ceil(1000)).saturating_sub(1).max(1);
            let (from, end) = if *zoom_in { (1.0, *to) } else { (*to, 1.0) };
            format!(
                "[0:v]zoompan=z='{from:.4}+({:.4})*on/{last}':x='iw/2-(iw/zoom/2)':y='ih/2-(ih/zoom/2)':d=1:s={w}x{h}:fps={FPS}[bg]",
                end - from
            )
        }
        Input::Image { motion: Motion::Still, .. } => format!("[0:v]scale={w}:{h},setsar=1[bg]"),
        Input::Frames { .. } => format!("[0:v]scale={w}:{h},fps={FPS},setsar=1[bg]"),
        Input::Video { focus, .. } => format!(
            "[0:v]scale={w}:{h}:force_original_aspect_ratio=increase,crop={w}:{h}:x='(iw-ow)*{:.3}':y='(ih-oh)*{:.3}',fps={FPS},setsar=1[bg]",
            focus[0].clamp(0.0, 1.0),
            focus[1].clamp(0.0, 1.0)
        ),
    };
    let mut last = "bg";
    if spec.subs_track.is_some() {
        // Each subtitle image stays until the next one; the last one until the clip ends.
        graph.push_str(&format!(";[bg][2:v]overlay=x=0:y={}:eof_action=repeat[subs]", f.subs_y()));
        last = "subs";
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

    fn spec(subs: bool, pan: Pan) -> ClipSpec {
        ClipSpec {
            input: Input::Image { path: "scenes/s001/image_1080.png".into(), motion: Motion::Pan(pan) },
            audio: "scenes/s001/voice.mp3".into(),
            voice_ms: 6150,
            subs_track: subs.then(|| "scenes/s001/subs/track.ffconcat".to_owned()),
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
        let a = clip_args(&spec(false, Pan::LeftToRight));
        assert_eq!(a.iter().filter(|x| *x == "6.400").count(), 2, "input -t and output -t");
        assert!(graph(&a).contains("apad=pad_dur=0.250"));
    }

    #[test]
    fn pan_uses_time_based_crop_not_zoompan() {
        let a = clip_args(&spec(false, Pan::RightToLeft));
        let g = graph(&a);
        assert!(g.contains("crop=w=1920:h=1080:x='(iw-ow)*(1-t/6.400)':y='(ih-oh)/2'"), "{g}");
        assert!(!g.contains("zoompan"));
    }

    #[test]
    fn subtitle_track_is_one_concat_input_and_one_overlay() {
        let a = clip_args(&spec(true, Pan::TopToBottom));
        let g = graph(&a);
        assert!(g.contains("[bg][2:v]overlay=x=0:y=816:eof_action=repeat[subs]"), "{g}");
        assert!(g.contains("[subs]format=yuv420p[v]"), "{g}");
        let i = a.iter().position(|x| x == "scenes/s001/subs/track.ffconcat").unwrap();
        assert_eq!(&a[i - 5..i], ["-f", "concat", "-safe", "0", "-i"]);
        let inputs = a.iter().filter(|x| *x == "-i").count();
        assert_eq!(inputs, 3);
    }

    #[test]
    fn without_subtitles_the_background_goes_straight_to_the_encoder() {
        let a = clip_args(&spec(false, Pan::LeftToRight));
        assert!(graph(&a).contains("[bg]format=yuv420p[v]"));
        assert_eq!(a.iter().filter(|x| *x == "-i").count(), 2);
    }

    #[test]
    fn identical_stream_parameters_for_concat() {
        let a = clip_args(&spec(false, Pan::BottomToTop));
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
    fn typed_code_is_a_frame_track() {
        let mut s = spec(false, Pan::LeftToRight);
        s.input = Input::Frames { track: "scenes/s001/code/track.ffconcat".into() };
        let a = clip_args(&s);
        let i = a.iter().position(|x| x == "scenes/s001/code/track.ffconcat").unwrap();
        assert_eq!(&a[i - 5..i], ["-f", "concat", "-safe", "0", "-i"]);
        assert!(graph(&a).starts_with("[0:v]scale=1920:1080,fps=30,setsar=1[bg]"));
    }

    #[test]
    fn photos_zoom_with_zoompan_up_to_one_to_one() {
        let mut s = spec(false, Pan::LeftToRight);
        s.input = Input::Image { path: "scenes/s001/photo_1080p_ab.jpg".into(), motion: Motion::Zoom { to: 1.08, zoom_in: true } };
        let g = graph(&clip_args(&s)).to_owned();
        // 6.4 s at 30 fps = 192 frames → last frame 191.
        assert!(g.contains("zoompan=z='1.0000+(0.0800)*on/191'"), "{g}");
        assert!(g.contains(":d=1:s=1920x1080:fps=30[bg]"), "{g}");
        s.input = Input::Image { path: "p.jpg".into(), motion: Motion::Zoom { to: 1.05, zoom_in: false } };
        assert!(graph(&clip_args(&s)).contains("z='1.0500+(-0.0500)*on/191'"));
        assert_eq!(Motion::Zoom { to: 1.05, zoom_in: false }.as_str(), "zoom_out");
    }

    #[test]
    fn stills_and_clips() {
        let mut s = spec(true, Pan::LeftToRight);
        s.input = Input::Image { path: "scenes/s001/visual.png".into(), motion: Motion::Still };
        assert!(graph(&clip_args(&s)).starts_with("[0:v]scale=1920:1080,setsar=1[bg];[bg][2:v]overlay"));
        s.input = Input::Video { path: "assets/pixabay_1.mp4".into(), focus: [0.5, 0.25] };
        let a = clip_args(&s);
        let i = a.iter().position(|x| x == "assets/pixabay_1.mp4").unwrap();
        assert_eq!(&a[i - 5..i], ["-stream_loop", "-1", "-t", "6.400", "-i"]);
        let g = graph(&a);
        assert!(g.contains("force_original_aspect_ratio=increase,crop=1920:1080:x='(iw-ow)*0.500':y='(ih-oh)*0.250',fps=30"), "{g}");
        assert!(g.contains("[1:a]apad"), "the clip's own sound is not used: {g}");
    }

    /// Real FFmpeg on every input kind (zoom photo, still, short looped clip).
    /// `NITEAN_FFMPEG=src-tauri/binaries/ffmpeg-x86_64-pc-windows-gnu.exe`, `--ignored`.
    #[test]
    #[ignore = "needs FFmpeg"]
    fn real_ffmpeg_renders_every_input_kind() {
        let ffmpeg = std::env::var("NITEAN_FFMPEG").expect("set NITEAN_FFMPEG");
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        image::RgbImage::from_fn(2074, 1166, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, 120])).save(d.join("photo.jpg")).unwrap();
        image::RgbImage::from_pixel(1920, 1080, image::Rgb([240, 240, 240])).save(d.join("still.png")).unwrap();
        std::fs::write(d.join("voice.wav"), crate::tts::wav::to_wav(&crate::tts::wav::tests::tone(48_000, 100, 2_500, 0.3))).unwrap();
        let ok = std::process::Command::new(&ffmpeg)
            .current_dir(d)
            .args(["-y", "-v", "error", "-f", "lavfi", "-i", "testsrc=size=1280x720:rate=25:duration=1", "-c:v", "libx264", "clip.mp4"])
            .status()
            .unwrap()
            .success();
        assert!(ok, "test clip");
        for (name, input) in [
            ("zoom", Input::Image { path: "photo.jpg".into(), motion: Motion::Zoom { to: 1.08, zoom_in: true } }),
            ("still", Input::Image { path: "still.png".into(), motion: Motion::Still }),
            ("video", Input::Video { path: "clip.mp4".into(), focus: [0.5, 0.5] }),
            ("frames", Input::Frames { track: "frames.ffconcat".into() }),
        ] {
            std::fs::write(
                d.join("frames.ffconcat"),
                "ffconcat version 1.0\nfile 'still.png'\nduration 1.000\nfile 'photo.jpg'\nduration 1.950\nfile 'photo.jpg'\n",
            )
            .unwrap();
            let spec = ClipSpec {
                input,
                audio: "voice.wav".into(),
                voice_ms: 2_700,
                subs_track: None,
                frame: Frame::for_resolution("1080p"),
                encoder_args: vec!["-c:v".into(), "libx264".into(), "-preset".into(), "veryfast".into()],
                output: format!("{name}.mkv"),
            };
            let out = std::process::Command::new(&ffmpeg).current_dir(d).args(["-v", "error"]).args(clip_args(&spec)).output().unwrap();
            assert!(out.status.success(), "{name}: {}", String::from_utf8_lossy(&out.stderr));
            let probe = std::process::Command::new(&ffmpeg).current_dir(d).args(["-i", &format!("{name}.mkv")]).output().unwrap();
            let info = String::from_utf8_lossy(&probe.stderr).into_owned();
            assert!(info.contains("1920x1080") && info.contains("Duration: 00:00:02.9"), "{name}: {info}");
        }
    }

    #[test]
    fn subtitle_band_fits_the_frame() {
        let f = Frame::for_resolution("1080p");
        assert_eq!(f.subs_y(), 816);
        let f = Frame::for_resolution("720p");
        assert_eq!((f.width, f.height, f.subs_y()), (1280, 720, 544));
    }
}
