//! A scene's subtitle images as one timed image sequence (`subs/track.ffconcat`), read by
//! FFmpeg's concat demuxer and drawn with a single `overlay` (03 Step 6a). Each image is one
//! subtitle *state*: a cue with one word highlighted (or the plain cue when highlighting is
//! off). Each image stays on screen until the next one starts.

/// Track file name inside `scenes/<id>/subs/`.
pub const TRACK_FILE: &str = "track.ffconcat";

/// One image of the track: file name (relative to the track file) and its start/end.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackImage<'a> {
    pub file: &'a str,
    pub start_ms: u64,
    pub end_ms: u64,
}

fn secs(ms: u64) -> String {
    format!("{}.{:03}", ms / 1000, ms % 1000)
}

/// `ffconcat` text. Durations run from one start to the next (the last image to its end);
/// zero-length images are skipped. The last file is listed twice: the concat demuxer only
/// applies the final `duration` when another entry follows.
pub fn ffconcat(images: &[TrackImage<'_>]) -> String {
    let mut out = String::from("ffconcat version 1.0\n");
    let mut last: Option<&str> = None;
    for (i, img) in images.iter().enumerate() {
        let until = images.get(i + 1).map_or(img.end_ms, |n| n.start_ms);
        if until <= img.start_ms {
            continue;
        }
        out.push_str(&format!("file '{}'\nduration {}\n", img.file, secs(until - img.start_ms)));
        last = Some(img.file);
    }
    if let Some(f) = last {
        out.push_str(&format!("file '{f}'\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_run_to_the_next_start_and_last_is_repeated() {
        let t = ffconcat(&[
            TrackImage { file: "000.png", start_ms: 0, end_ms: 1400 },
            TrackImage { file: "001.png", start_ms: 1500, end_ms: 3500 },
            TrackImage { file: "002.png", start_ms: 3500, end_ms: 6000 },
        ]);
        assert_eq!(
            t,
            "ffconcat version 1.0\nfile '000.png'\nduration 1.500\nfile '001.png'\nduration 2.000\n\
             file '002.png'\nduration 2.500\nfile '002.png'\n"
        );
    }

    #[test]
    fn zero_length_states_are_skipped() {
        let t = ffconcat(&[
            TrackImage { file: "000.png", start_ms: 0, end_ms: 0 },
            TrackImage { file: "001.png", start_ms: 0, end_ms: 800 },
        ]);
        assert_eq!(t, "ffconcat version 1.0\nfile '001.png'\nduration 0.800\nfile '001.png'\n");
    }

    #[test]
    fn empty_track_has_only_the_header() {
        assert_eq!(ffconcat(&[]), "ffconcat version 1.0\n");
    }
}
