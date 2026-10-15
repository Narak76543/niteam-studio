//! Joins the scene clips into one MP4 (03 Step 6b): concat demuxer, video stream-copied,
//! audio encoded once to AAC (clips carry PCM, so there are no gaps between scenes).

/// `concat.txt` content; paths relative to the list file (the project folder).
pub fn concat_list(clips: &[String]) -> String {
    clips
        .iter()
        .map(|c| format!("file '{}'\n", c.replace('\'', "'\\''")))
        .collect()
}

/// Arguments to join `list` (relative to the project folder) into `output`.
pub fn concat_args(list: &str, output: &str) -> Vec<String> {
    [
        "-y", "-f", "concat", "-safe", "0", "-i", list, "-map", "0:v", "-map", "0:a", "-c:v", "copy",
        "-c:a", "aac", "-b:a", "192k", "-ar", "48000", "-ac", "2", "-movflags", "+faststart", "-f",
        "mp4", output,
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_quotes_paths() {
        let l = concat_list(&["scenes/s001/clip.mkv".into(), "scenes/it's/clip.mkv".into()]);
        assert_eq!(l, "file 'scenes/s001/clip.mkv'\nfile 'scenes/it'\\''s/clip.mkv'\n");
    }

    #[test]
    fn video_is_copied_audio_encoded_once() {
        let a = concat_args("concat.txt", "output/final.tmp.mp4");
        assert!(a.windows(2).any(|w| w[0] == "-c:v" && w[1] == "copy"));
        assert!(a.windows(2).any(|w| w[0] == "-c:a" && w[1] == "aac"));
        assert!(a.windows(2).any(|w| w[0] == "-movflags" && w[1] == "+faststart"));
        assert_eq!(a.last().map(String::as_str), Some("output/final.tmp.mp4"));
    }
}
