//! Gemini TTS engine (user's key). Returns WAV; no word timings.

use super::audio::pcm16_to_wav;
use super::{AudioFormat, TtsEngine, TtsError, TtsFuture, TtsOutput, TtsRequest};
use crate::gemini::GeminiClient;
use crate::gemini::retry::RetryHooks;
use crate::storage::ApiKey;

const DEFAULT_PCM_RATE: u32 = 24_000;

pub struct GeminiTts {
    client: GeminiClient,
    key: ApiKey,
}

impl GeminiTts {
    pub fn new(client: GeminiClient, key: ApiKey) -> Self {
        Self { client, key }
    }
}

impl TtsEngine for GeminiTts {
    fn synthesize<'a>(&'a self, request: &'a TtsRequest, hooks: &'a RetryHooks) -> TtsFuture<'a> {
        Box::pin(async move {
            let audio = self
                .client
                .synthesize_speech(
                    &self.key,
                    &request.model,
                    &request.text,
                    &request.voice,
                    &request.style,
                    hooks,
                )
                .await?;
            let wav = match audio.mime_type.as_str() {
                "audio/wav" | "audio/x-wav" | "audio/wave" => audio.bytes,
                "audio/l16" | "audio/pcm" => pcm16_to_wav(
                    &audio.bytes,
                    audio.sample_rate.unwrap_or(DEFAULT_PCM_RATE),
                    1,
                ),
                other => return Err(TtsError::BadAudio(format!("unexpected audio type {other}"))),
            };
            Ok(TtsOutput {
                audio: wav,
                format: AudioFormat::Wav,
                words: vec![],
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::secrets;
    use crate::storage::settings::Settings;

    /// Real Gemini TTS call with the saved key. Run with:
    /// `cargo test --manifest-path src-tauri/Cargo.toml live_gemini_tts -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "calls the real Gemini API"]
    async fn live_gemini_tts_khmer() {
        let key = secrets::get_api_key().unwrap().expect("no API key saved");
        let settings = Settings::default();
        let client = GeminiClient::new(&settings.concurrency).unwrap();
        let engine = GeminiTts::new(client, key);
        let req = TtsRequest {
            text: "សួស្តី! នេះជាការសាកល្បងសំឡេង។".into(),
            voice: "Kore".into(),
            rate: 0,
            pitch: 0,
            model: settings.models.tts.clone(),
            style: "calm, warm storyteller".into(),
        };
        let out = engine
            .synthesize(&req, &RetryHooks::default())
            .await
            .unwrap();
        let ms = super::super::audio::wav_duration_ms(&out.audio);
        eprintln!("gemini tts: {} bytes, {:?} ms", out.audio.len(), ms);
        assert!(ms.unwrap_or(0) > 500);
    }
}
