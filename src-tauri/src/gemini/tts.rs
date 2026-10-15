//! Gemini TTS through the Interactions API (`POST /v1beta/interactions`), as documented on
//! ai.google.dev/gemini-api/docs/speech-generation (checked 2026-10-15):
//! - request: `model`, `input[{type:"user_input", content:[{type:"text", text, annotations:
//!   [{type:"speech_metadata", style}]}]}]`, `response_format:{type:"audio"}`,
//!   `generation_config:{speech_config:[{voice}]}`, `store:false`
//! - response: `steps[{type:"model_output", content:[{type:"audio", data(base64), mime_type}]}]`
//! - unary default output: WAV (RIFF, 24 kHz, mono, 16-bit PCM).

use serde::Serialize;

use super::GeminiError;
use super::interactions::{Interaction, last_media};
use super::client::{EndpointKind, GeminiClient};
use super::retry::{MAX_ATTEMPTS, RetryHooks};
use crate::storage::ApiKey;

/// The 30 prebuilt studio voices (name, character) from the docs.
pub const PREBUILT_VOICES: &[(&str, &str)] = &[
    ("Zephyr", "Bright"),
    ("Puck", "Upbeat"),
    ("Charon", "Informative"),
    ("Kore", "Firm"),
    ("Fenrir", "Excitable"),
    ("Leda", "Youthful"),
    ("Orus", "Firm"),
    ("Aoede", "Breezy"),
    ("Callirrhoe", "Easy-going"),
    ("Autonoe", "Bright"),
    ("Enceladus", "Breathy"),
    ("Iapetus", "Clear"),
    ("Umbriel", "Easy-going"),
    ("Algieba", "Smooth"),
    ("Despina", "Smooth"),
    ("Erinome", "Clear"),
    ("Algenib", "Gravelly"),
    ("Rasalgethi", "Informative"),
    ("Laomedeia", "Upbeat"),
    ("Achernar", "Soft"),
    ("Alnilam", "Firm"),
    ("Schedar", "Even"),
    ("Gacrux", "Mature"),
    ("Pulcherrima", "Forward"),
    ("Achird", "Friendly"),
    ("Zubenelgenubi", "Casual"),
    ("Vindemiatrix", "Gentle"),
    ("Sadachbia", "Lively"),
    ("Sadaltager", "Knowledgeable"),
    ("Sulafat", "Warm"),
];

#[derive(Serialize)]
struct InteractionRequest<'a> {
    model: &'a str,
    input: [UserInput<'a>; 1],
    response_format: ResponseFormat,
    generation_config: GenerationConfig<'a>,
    /// Don't keep the request/response on Google's side for later retrieval.
    store: bool,
}

#[derive(Serialize)]
struct UserInput<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    content: [TextContent<'a>; 1],
}

#[derive(Serialize)]
struct TextContent<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    text: &'a str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    annotations: Vec<SpeechMetadata<'a>>,
}

#[derive(Serialize)]
struct SpeechMetadata<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    style: &'a str,
}

#[derive(Serialize)]
struct ResponseFormat {
    #[serde(rename = "type")]
    kind: &'static str,
}

#[derive(Serialize)]
struct GenerationConfig<'a> {
    speech_config: [VoiceChoice<'a>; 1],
}

#[derive(Serialize)]
struct VoiceChoice<'a> {
    voice: &'a str,
}

/// Audio returned by Gemini TTS.
pub struct SpeechAudio {
    pub bytes: Vec<u8>,
    /// `audio/wav` (default) or `audio/l16` (raw PCM, then `sample_rate` applies).
    pub mime_type: String,
    pub sample_rate: Option<u32>,
}

fn build_request<'a>(
    model: &'a str,
    text: &'a str,
    voice: &'a str,
    style: &'a str,
) -> InteractionRequest<'a> {
    let style = style.trim();
    InteractionRequest {
        model,
        input: [UserInput {
            kind: "user_input",
            content: [TextContent {
                kind: "text",
                text,
                annotations: if style.is_empty() {
                    vec![]
                } else {
                    vec![SpeechMetadata {
                        kind: "speech_metadata",
                        style,
                    }]
                },
            }],
        }],
        response_format: ResponseFormat { kind: "audio" },
        generation_config: GenerationConfig {
            speech_config: [VoiceChoice { voice }],
        },
        store: false,
    }
}

fn extract_audio(interaction: &Interaction) -> Result<SpeechAudio, GeminiError> {
    let media = last_media(interaction, "audio")?;
    Ok(SpeechAudio {
        bytes: media.bytes,
        mime_type: media.mime_type.unwrap_or_else(|| "audio/wav".into()),
        sample_rate: media.sample_rate,
    })
}

impl GeminiClient {
    /// One TTS request (5 attempts with the usual backoff).
    pub async fn synthesize_speech(
        &self,
        key: &ApiKey,
        model: &str,
        text: &str,
        voice: &str,
        style: &str,
        hooks: &RetryHooks,
    ) -> Result<SpeechAudio, GeminiError> {
        let body = build_request(model, text, voice, style);
        let interaction: Interaction = self
            .create_interaction(key, model, EndpointKind::Tts, &body, hooks, MAX_ATTEMPTS)
            .await?;
        extract_audio(&interaction)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_matches_documented_shape() {
        let v = serde_json::to_value(build_request(
            "gemini-3.8-flash-tts",
            "សួស្តី",
            "Kore",
            "calm storyteller",
        ))
        .unwrap();
        assert_eq!(
            v,
            json!({
                "model": "gemini-3.8-flash-tts",
                "input": [{
                    "type": "user_input",
                    "content": [{
                        "type": "text",
                        "text": "សួស្តី",
                        "annotations": [{"type": "speech_metadata", "style": "calm storyteller"}]
                    }]
                }],
                "response_format": {"type": "audio"},
                "generation_config": {"speech_config": [{"voice": "Kore"}]},
                "store": false
            })
        );
        let v = serde_json::to_value(build_request("m", "t", "Kore", "  ")).unwrap();
        assert!(v["input"][0]["content"][0].get("annotations").is_none());
    }

    #[test]
    fn extracts_last_audio_block() {
        let i: Interaction = serde_json::from_value(json!({
            "status": "completed",
            "steps": [{"type": "model_output", "content": [
                {"type": "audio", "data": "AAAA", "mime_type": "audio/wav"},
                {"type": "audio", "data": "AQID", "mime_type": "audio/wav"}
            ]}]
        }))
        .unwrap();
        let a = extract_audio(&i).unwrap();
        assert_eq!(a.bytes, vec![1, 2, 3]);
        assert_eq!(a.mime_type, "audio/wav");
    }

    #[test]
    fn missing_audio_or_failed_status_is_an_error() {
        let i: Interaction = serde_json::from_value(json!({"status": "completed", "steps": [
            {"type": "model_output", "content": [{"type": "text", "text": "hi"}]}
        ]}))
        .unwrap();
        assert!(extract_audio(&i).is_err());
        let i: Interaction =
            serde_json::from_value(json!({"status": "failed", "steps": []})).unwrap();
        assert!(extract_audio(&i).is_err());
    }
}
