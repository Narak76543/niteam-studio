//! 16-bit PCM WAV helpers for local voices (`docs/09-voice-profiles.md`): read the engine's
//! output, trim silence, join sentences, normalise loudness, write the scene's `voice.wav`.

use super::audio::pcm16_to_wav;

/// Mono 16-bit PCM.
#[derive(Debug, Clone, PartialEq)]
pub struct Pcm {
    pub rate: u32,
    pub samples: Vec<i16>,
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*b.get(at)?, *b.get(at + 1)?]))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes([*b.get(at)?, *b.get(at + 1)?, *b.get(at + 2)?, *b.get(at + 3)?]))
}

/// Parses a 16-bit PCM WAV (mono or stereo; stereo is averaged to mono).
pub fn parse(b: &[u8]) -> Result<Pcm, String> {
    if b.get(0..4) != Some(b"RIFF") || b.get(8..12) != Some(b"WAVE") {
        return Err("not a WAV file".into());
    }
    let mut pos = 12;
    let mut fmt: Option<(u16, u16, u32, u16)> = None;
    while pos + 8 <= b.len() {
        let id = &b[pos..pos + 4];
        let size = u32_at(b, pos + 4).unwrap_or(0) as usize;
        let body = pos + 8;
        if id == b"fmt " {
            let tag = u16_at(b, body).ok_or("short fmt chunk")?;
            let channels = u16_at(b, body + 2).ok_or("short fmt chunk")?;
            let rate = u32_at(b, body + 4).ok_or("short fmt chunk")?;
            let bits = u16_at(b, body + 14).ok_or("short fmt chunk")?;
            fmt = Some((tag, channels, rate, bits));
        } else if id == b"data" {
            let (tag, channels, rate, bits) = fmt.ok_or("data before fmt")?;
            // 1 = PCM, 0xFFFE = extensible (still integer PCM here).
            if !(tag == 1 || tag == 0xFFFE) || bits != 16 || channels == 0 || rate == 0 {
                return Err(format!("unsupported WAV (format {tag}, {bits}-bit, {channels} ch)"));
            }
            let available = b.len().saturating_sub(body);
            let size = if size == 0 || size > available { available } else { size };
            let frame = usize::from(channels) * 2;
            let frames = size / frame;
            let mut samples = Vec::with_capacity(frames);
            for f in 0..frames {
                let at = body + f * frame;
                let sum: i32 = (0..usize::from(channels))
                    .map(|c| i32::from(i16::from_le_bytes([b[at + 2 * c], b[at + 2 * c + 1]])))
                    .sum();
                samples.push((sum / i32::from(channels)) as i16);
            }
            return Ok(Pcm { rate, samples });
        }
        pos = body + size + (size & 1);
    }
    Err("no audio data in the WAV file".into())
}

pub fn to_wav(p: &Pcm) -> Vec<u8> {
    let bytes: Vec<u8> = p.samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    pcm16_to_wav(&bytes, p.rate, 1)
}

pub fn ms(p: &Pcm) -> u64 {
    if p.rate == 0 { 0 } else { p.samples.len() as u64 * 1000 / u64::from(p.rate) }
}

pub fn silence(rate: u32, ms: u64) -> Vec<i16> {
    vec![0; (u64::from(rate) * ms / 1000) as usize]
}

/// Level (dBFS) under which a 10 ms frame counts as silence.
const SILENCE_DBFS: f64 = -45.0;

fn frame_dbfs(frame: &[i16]) -> f64 {
    if frame.is_empty() {
        return -120.0;
    }
    let sum: f64 = frame.iter().map(|s| f64::from(*s).powi(2)).sum();
    let rms = (sum / frame.len() as f64).sqrt() / 32768.0;
    if rms <= 0.0 { -120.0 } else { 20.0 * rms.log10() }
}

/// Sample range [start, end) between the first and last non-silent 10 ms frame.
/// `None` when the whole clip is silent.
pub fn speech_bounds(p: &Pcm) -> Option<(usize, usize)> {
    let frame = (p.rate as usize / 100).max(1);
    let frames: Vec<&[i16]> = p.samples.chunks(frame).collect();
    let loud = |f: &&[i16]| frame_dbfs(f) > SILENCE_DBFS;
    let first = frames.iter().position(loud)?;
    let last = frames.iter().rposition(loud)?;
    Some((first * frame, ((last + 1) * frame).min(p.samples.len())))
}

/// Speech plus up to `margin_ms` of the original silence on each side.
pub fn trim(p: &Pcm, margin_ms: u64) -> Pcm {
    let Some((s, e)) = speech_bounds(p) else {
        return Pcm { rate: p.rate, samples: Vec::new() };
    };
    let m = (u64::from(p.rate) * margin_ms / 1000) as usize;
    Pcm { rate: p.rate, samples: p.samples[s.saturating_sub(m)..(e + m).min(p.samples.len())].to_vec() }
}

/// Scales to `target_dbfs` RMS (over speech), never letting the peak exceed `peak_dbfs`.
pub fn normalize(p: &mut Pcm, target_dbfs: f64, peak_dbfs: f64) {
    let Some((s, e)) = speech_bounds(p) else { return };
    let speech = &p.samples[s..e];
    let rms_db = frame_dbfs(speech);
    let peak = speech.iter().map(|v| i32::from(*v).unsigned_abs()).max().unwrap_or(0);
    if peak == 0 {
        return;
    }
    let peak_db = 20.0 * (f64::from(peak) / 32768.0).log10();
    let gain_db = (target_dbfs - rms_db).min(peak_dbfs - peak_db);
    let gain = 10f64.powf(gain_db / 20.0);
    for v in &mut p.samples {
        *v = (f64::from(*v) * gain).round().clamp(-32768.0, 32767.0) as i16;
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// `ms` of a 440 Hz tone at `amp`, surrounded by `pad_ms` of silence.
    pub fn tone(rate: u32, pad_ms: u64, ms: u64, amp: f64) -> Pcm {
        let mut samples = silence(rate, pad_ms);
        let n = (u64::from(rate) * ms / 1000) as usize;
        samples.extend((0..n).map(|i| {
            (amp * 32767.0 * (2.0 * std::f64::consts::PI * 440.0 * i as f64 / f64::from(rate)).sin()) as i16
        }));
        samples.extend(silence(rate, pad_ms));
        Pcm { rate, samples }
    }

    #[test]
    fn round_trips_and_downmixes_stereo() {
        let p = tone(48_000, 100, 500, 0.5);
        assert_eq!(parse(&to_wav(&p)).unwrap(), p);
        let stereo: Vec<u8> = [1000i16, 3000, -2000, -4000].iter().flat_map(|s| s.to_le_bytes()).collect();
        let w = pcm16_to_wav(&stereo, 16_000, 2);
        assert_eq!(parse(&w).unwrap().samples, vec![2000, -3000]);
        assert!(parse(b"not a wav").is_err());
    }

    #[test]
    fn trims_to_speech_with_a_margin() {
        let p = tone(48_000, 400, 1000, 0.3);
        assert_eq!(ms(&p), 1800);
        let t = trim(&p, 50);
        assert!((ms(&t) as i64 - 1100).abs() <= 20, "{}", ms(&t));
        assert!(trim(&Pcm { rate: 48_000, samples: silence(48_000, 300) }, 50).samples.is_empty());
    }

    #[test]
    fn normalizes_quiet_speech_without_clipping() {
        let mut quiet = tone(48_000, 0, 1000, 0.02);
        normalize(&mut quiet, -18.0, -1.0);
        let peak = quiet.samples.iter().map(|v| i32::from(*v).abs()).max().unwrap();
        assert!(peak > 3000 && peak <= 29_300, "{peak}");
        let mut loud = tone(48_000, 0, 1000, 0.99);
        normalize(&mut loud, -6.0, -1.0);
        let peak = loud.samples.iter().map(|v| i32::from(*v).abs()).max().unwrap();
        assert!(peak <= 29_300, "peak limited to -1 dBFS: {peak}");
    }
}
