//! Edge TTS (Microsoft Edge "Read aloud") through the `msedge-tts` crate.
//! **Unofficial service** (see CLAUDE.md): it can break or be blocked at any time, so every
//! failure becomes [`TtsError::EdgeUnavailable`] with a "switch to Gemini TTS" hint.
//!
//! Hardening around the crate:
//! - text is XML-escaped here (the crate inserts it into SSML verbatim);
//! - each attempt runs in its own task with a timeout, so a stuck socket or a panic inside
//!   the crate becomes an error instead of hanging or crashing the app.

use std::sync::Arc;
use std::time::Duration;

use msedge_tts::tts::SpeechConfig;
use msedge_tts::tts::client::tokio_runtime::connect_async;
use tokio::sync::Semaphore;

use super::{AudioFormat, TtsEngine, TtsError, TtsFuture, TtsOutput, TtsRequest, WordTiming};
use crate::gemini::retry::{RetryHooks, RetryNotice, RetryReason, backoff_delay};

const OUTPUT_FORMAT: &str = "audio-24khz-48kbitrate-mono-mp3";
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_ATTEMPTS: u32 = 3;
/// Word-boundary offsets/durations are in 100-ns ticks.
const TICKS_PER_MS: u64 = 10_000;

pub struct EdgeTts {
    limit: Arc<Semaphore>,
}

impl EdgeTts {
    pub fn new(concurrency: usize) -> Self {
        Self {
            limit: Arc::new(Semaphore::new(concurrency.max(1))),
        }
    }
}

/// Escapes text for an SSML element body.
pub fn escape_ssml(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // Control characters are invalid in XML 1.0.
            c if c.is_control() && c != '\n' && c != '\t' => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// One synthesis on a fresh connection, isolated in its own task.
async fn attempt(text: String, config: SpeechConfig) -> Result<(Vec<u8>, Vec<WordTiming>), String> {
    let task = tokio::spawn(async move {
        let mut client = connect_async().await.map_err(|e| e.to_string())?;
        let audio = client
            .synthesize(&text, &config)
            .await
            .map_err(|e| e.to_string())?;
        let words = audio
            .audio_metadata
            .into_iter()
            .filter(|m| m.metadata_type.as_deref() == Some("WordBoundary"))
            .filter_map(|m| {
                let text = m.text?;
                Some(WordTiming {
                    text,
                    start_ms: m.offset / TICKS_PER_MS,
                    end_ms: (m.offset + m.duration) / TICKS_PER_MS,
                })
            })
            .collect::<Vec<_>>();
        Ok::<_, String>((audio.audio_bytes, words))
    });
    let abort = task.abort_handle();
    match tokio::time::timeout(ATTEMPT_TIMEOUT, task).await {
        Ok(Ok(result)) => result,
        Ok(Err(join)) => Err(format!("Edge TTS task failed: {join}")),
        Err(_) => {
            abort.abort();
            Err(format!("no answer within {} s", ATTEMPT_TIMEOUT.as_secs()))
        }
    }
}

impl TtsEngine for EdgeTts {
    fn synthesize<'a>(&'a self, request: &'a TtsRequest, hooks: &'a RetryHooks) -> TtsFuture<'a> {
        Box::pin(async move {
            let _permit = self
                .limit
                .acquire()
                .await
                .map_err(|_| TtsError::Cancelled)?;
            let config = SpeechConfig {
                voice_name: request.voice.clone(),
                audio_format: OUTPUT_FORMAT.to_owned(),
                pitch: request.pitch.clamp(-50, 50),
                rate: request.rate.clamp(-50, 50),
                volume: 0,
            };
            let text = escape_ssml(&request.text);
            let mut last_error = String::new();
            for n in 1..=MAX_ATTEMPTS {
                if hooks.cancel.is_cancelled() {
                    return Err(TtsError::Cancelled);
                }
                let result = tokio::select! {
                    r = attempt(text.clone(), config.clone()) => r,
                    _ = hooks.cancel.cancelled() => return Err(TtsError::Cancelled),
                };
                match result {
                    Ok((audio, words)) if !audio.is_empty() => {
                        return Ok(TtsOutput {
                            audio,
                            format: AudioFormat::Mp3,
                            words,
                        });
                    }
                    Ok(_) => last_error = "Edge TTS returned no audio".into(),
                    Err(e) => last_error = e,
                }
                if n < MAX_ATTEMPTS {
                    let delay = backoff_delay(n, None, Duration::ZERO);
                    hooks.notify(&RetryNotice {
                        model: format!("Edge TTS ({})", request.voice),
                        previous_model: None,
                        reason: RetryReason::Network,
                        attempt: n + 1,
                        max_attempts: MAX_ATTEMPTS,
                        delay_ms: u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
                        status: None,
                        error: last_error.clone(),
                    });
                    tokio::select! {
                        _ = tokio::time::sleep(delay) => {}
                        _ = hooks.cancel.cancelled() => return Err(TtsError::Cancelled),
                    }
                }
            }
            Err(TtsError::EdgeUnavailable(last_error))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_xml() {
        assert_eq!(
            escape_ssml("a < b & \"c\" 'd' >"),
            "a &lt; b &amp; &quot;c&quot; &apos;d&apos; &gt;"
        );
        assert_eq!(escape_ssml("ស្រូវ\u{0007}"), "ស្រូវ ");
    }

    /// Real Edge TTS call (network). Run with:
    /// `cargo test --manifest-path src-tauri/Cargo.toml live_edge -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "calls the real Edge TTS service"]
    async fn live_edge_khmer() {
        let engine = EdgeTts::new(1);
        let req = TtsRequest {
            text: "តើប្រាសាទអង្គរវត្តត្រូវបានសាងសង់ឡើងដោយរបៀបណា? នេះជារឿងដ៏អស្ចារ្យ ដែលមានអាយុជាងប្រាំបួនរយឆ្នាំ។"
                .into(),
            voice: "km-KH-SreymomNeural".into(),
            rate: 0,
            pitch: 0,
            model: String::new(),
            style: String::new(),
        };
        let out = engine
            .synthesize(&req, &RetryHooks::default())
            .await
            .unwrap();
        let ms = super::super::audio::mp3_duration_ms(&out.audio).unwrap();
        eprintln!(
            "audio {} bytes, {ms} ms, {} words",
            out.audio.len(),
            out.words.len()
        );
        for w in &out.words {
            eprintln!("  {:>6}–{:<6} {}", w.start_ms, w.end_ms, w.text);
        }
        assert!(ms > 1_000);
        assert!(!out.words.is_empty(), "no WordBoundary events for Khmer");
    }
}
