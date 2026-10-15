//! Audio helpers without FFmpeg: duration of WAV / MP3 files, PCM → WAV, and the
//! "suspicious length" check (`docs/03-pipeline.md` Step 3).

use unicode_segmentation::UnicodeSegmentation;

use super::AudioFormat;
use crate::pipeline::project::Language;

pub fn duration_ms(bytes: &[u8], format: AudioFormat) -> Option<u64> {
    match format {
        AudioFormat::Wav => wav_duration_ms(bytes),
        AudioFormat::Mp3 => mp3_duration_ms(bytes),
    }
}

fn u32_le(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes([
        *b.get(at)?,
        *b.get(at + 1)?,
        *b.get(at + 2)?,
        *b.get(at + 3)?,
    ]))
}

/// Duration from the RIFF `fmt ` byte rate and the `data` chunk size.
pub fn wav_duration_ms(b: &[u8]) -> Option<u64> {
    if b.get(0..4)? != b"RIFF" || b.get(8..12)? != b"WAVE" {
        return None;
    }
    let mut pos = 12;
    let mut byte_rate = None;
    while pos + 8 <= b.len() {
        let id = b.get(pos..pos + 4)?;
        let size = u32_le(b, pos + 4)? as usize;
        let body = pos + 8;
        if id == b"fmt " {
            byte_rate = u32_le(b, body + 8);
        } else if id == b"data" {
            // Streaming writers sometimes leave 0 / 0xFFFFFFFF; use what is actually there.
            let available = b.len().saturating_sub(body);
            let size = if size == 0 || size > available {
                available
            } else {
                size
            };
            let rate = u64::from(byte_rate.filter(|r| *r > 0)?);
            return Some(size as u64 * 1000 / rate);
        }
        pos = body + size + (size & 1);
    }
    None
}

/// Wraps raw 16-bit little-endian PCM in a WAV header.
pub fn pcm16_to_wav(pcm: &[u8], sample_rate: u32, channels: u16) -> Vec<u8> {
    let block_align = channels * 2;
    let byte_rate = sample_rate * u32::from(block_align);
    let data_len = u32::try_from(pcm.len()).unwrap_or(u32::MAX);
    let mut out = Vec::with_capacity(44 + pcm.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.extend_from_slice(pcm);
    out
}

/// Counts MPEG audio Layer III frames. Edge sends CBR `audio-24khz-48kbitrate-mono-mp3`
/// (MPEG-2, 576 samples per frame), but any MPEG-1/2/2.5 Layer III stream works.
pub fn mp3_duration_ms(b: &[u8]) -> Option<u64> {
    let mut pos = skip_id3(b);
    let mut samples: u64 = 0;
    let mut rate: u64 = 0;
    while pos + 4 <= b.len() {
        let Some((len, frame_samples, sr)) = parse_frame_header(b.get(pos..pos + 4)?) else {
            pos += 1; // resync
            continue;
        };
        samples += frame_samples;
        rate = sr;
        pos += len;
    }
    (rate > 0).then(|| samples * 1000 / rate)
}

fn skip_id3(b: &[u8]) -> usize {
    if b.get(0..3) == Some(b"ID3") {
        let size = b.get(6..10).map_or(0, |s| {
            s.iter()
                .fold(0usize, |acc, x| (acc << 7) | usize::from(x & 0x7F))
        });
        10 + size
    } else {
        0
    }
}

/// Returns (frame length in bytes, samples per frame, sample rate) for a Layer III header.
fn parse_frame_header(h: &[u8]) -> Option<(usize, u64, u64)> {
    let (b1, b2, b3) = (*h.first()?, *h.get(1)?, *h.get(2)?);
    if b1 != 0xFF || (b2 & 0xE0) != 0xE0 {
        return None;
    }
    let version = (b2 >> 3) & 0b11; // 0 = 2.5, 2 = 2, 3 = 1
    let layer = (b2 >> 1) & 0b11; // 1 = Layer III
    if version == 1 || layer != 1 {
        return None;
    }
    let bitrate_idx = usize::from(b3 >> 4);
    let sr_idx = usize::from((b3 >> 2) & 0b11);
    let padding = u64::from((b3 >> 1) & 1);
    const BR_V1: [u64; 15] = [
        0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
    ];
    const BR_V2: [u64; 15] = [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];
    const SR_V1: [u64; 3] = [44_100, 48_000, 32_000];
    let mpeg1 = version == 3;
    let bitrate = *if mpeg1 {
        BR_V1.get(bitrate_idx)?
    } else {
        BR_V2.get(bitrate_idx)?
    } * 1000;
    let base_sr = *SR_V1.get(sr_idx)?;
    let sample_rate = match version {
        3 => base_sr,
        2 => base_sr / 2,
        _ => base_sr / 4,
    };
    if bitrate == 0 {
        return None;
    }
    let samples: u64 = if mpeg1 { 1152 } else { 576 };
    let len = samples / 8 * bitrate / sample_rate + padding;
    Some((
        usize::try_from(len).ok().filter(|l| *l > 4)?,
        samples,
        sample_rate,
    ))
}

/// Rough expected speech length. Khmer ≈ 11 grapheme clusters/s, English ≈ 14 chars/s,
/// at rate 0 %. Only used for the suspicious-length check, so it can be coarse.
pub fn expected_ms(text: &str, language: Language, rate_percent: i32) -> u64 {
    let units = text
        .graphemes(true)
        .filter(|g| !g.trim().is_empty())
        .count() as u64;
    let per_unit = match language {
        Language::Km => 90,
        Language::En => 70,
    };
    let speed = (100 + rate_percent.clamp(-90, 200)) as u64;
    units * per_unit * 100 / speed.max(10)
}

/// TTS sometimes truncates or loops: flag audio far outside 40 %–250 % of the expectation.
pub fn suspicious_length(actual_ms: u64, expected_ms: u64) -> bool {
    expected_ms > 0 && (actual_ms * 100 < expected_ms * 40 || actual_ms * 100 > expected_ms * 250)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_duration_from_header() {
        let pcm = vec![0u8; 24_000 * 2 * 3 / 2]; // 1.5 s of 24 kHz mono 16-bit
        let wav = pcm16_to_wav(&pcm, 24_000, 1);
        assert_eq!(wav_duration_ms(&wav), Some(1_500));
        assert_eq!(duration_ms(&wav, AudioFormat::Wav), Some(1_500));
        assert_eq!(wav_duration_ms(b"not a wav"), None);
    }

    fn mpeg2_frame_24khz_48kbps() -> Vec<u8> {
        // MPEG-2 Layer III, 48 kbps (idx 6), 24 kHz (idx 1), no padding.
        let header = [0xFF, 0xF3, 0x64, 0xC4];
        let len = 576 / 8 * 48_000 / 24_000; // 144 bytes
        let mut f = header.to_vec();
        f.resize(len, 0);
        f
    }

    #[test]
    fn mp3_duration_counts_frames() {
        // 1 s at 24 kHz = 24000 / 576 ≈ 41.67 frames → 125 frames = 3.0 s
        let mut data = b"ID3\x04\x00\x00\x00\x00\x00\x05abcde".to_vec();
        for _ in 0..125 {
            data.extend(mpeg2_frame_24khz_48kbps());
        }
        assert_eq!(mp3_duration_ms(&data), Some(3_000));
        assert_eq!(mp3_duration_ms(&[0u8; 100]), None);
    }

    #[test]
    fn suspicious_length_window() {
        assert!(!suspicious_length(5_000, 5_000));
        assert!(suspicious_length(1_000, 5_000));
        assert!(suspicious_length(13_000, 5_000));
        assert!(!suspicious_length(12_000, 5_000));
        assert!(!suspicious_length(0, 0));
    }

    #[test]
    fn expected_length_scales_with_rate() {
        let t = "តើស្រូវកើតមកពីណា?";
        let normal = expected_ms(t, Language::Km, 0);
        assert!(normal > 500);
        assert!(expected_ms(t, Language::Km, 50) < normal);
    }
}
