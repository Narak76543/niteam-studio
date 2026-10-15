//! Response types of the Gemini Interactions API (`POST /v1beta/interactions`), shared by
//! TTS (`gemini/tts.rs`) and image generation (`gemini/image.rs`).
//! Shape (ai.google.dev/api/interactions-api, checked 2026-10-15):
//! `{ status, steps: [{ type: "model_output", content: [{ type: "audio"|"image"|"text",
//!   data (base64), mime_type, sample_rate, text }] }] }`

use base64::Engine as _;
use serde::Deserialize;

use super::GeminiError;

#[derive(Debug, Deserialize)]
pub struct Interaction {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub steps: Vec<Step>,
}

#[derive(Debug, Deserialize)]
pub struct Step {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub content: Vec<Content>,
}

#[derive(Debug, Deserialize)]
pub struct Content {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub data: Option<String>,
    #[serde(default)]
    pub mime_type: Option<String>,
    #[serde(default)]
    pub sample_rate: Option<u32>,
    #[serde(default)]
    pub text: Option<String>,
}

/// Decoded media block.
pub struct Media {
    pub bytes: Vec<u8>,
    pub mime_type: Option<String>,
    pub sample_rate: Option<u32>,
}

fn model_outputs(i: &Interaction) -> impl Iterator<Item = &Content> {
    i.steps
        .iter()
        .filter(|s| s.kind == "model_output")
        .flat_map(|s| s.content.iter())
}

/// Text the model wrote instead of / next to the media (used to explain refusals).
pub fn output_text(i: &Interaction) -> String {
    model_outputs(i)
        .filter(|c| c.kind == "text")
        .filter_map(|c| c.text.as_deref())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The last `kind` ("audio" / "image") block of the model output, base64-decoded.
pub fn last_media(i: &Interaction, kind: &str) -> Result<Media, GeminiError> {
    if let Some(status) = &i.status
        && status != "completed"
    {
        return Err(GeminiError::BadResponse(format!(
            "interaction status '{status}'"
        )));
    }
    let block = model_outputs(i)
        .filter(|c| c.kind == kind)
        .last()
        .ok_or_else(|| GeminiError::BadResponse(format!("the answer contained no {kind}")))?;
    let data = block
        .data
        .as_deref()
        .ok_or_else(|| GeminiError::BadResponse(format!("the {kind} had no data")))?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.trim())
        .map_err(|e| GeminiError::BadResponse(format!("the {kind} is not valid base64: {e}")))?;
    if bytes.is_empty() {
        return Err(GeminiError::BadResponse(format!("the {kind} is empty")));
    }
    Ok(Media {
        bytes,
        mime_type: block.mime_type.clone(),
        sample_rate: block.sample_rate,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_last_block_of_kind() {
        let i: Interaction = serde_json::from_value(json!({
            "status": "completed",
            "steps": [{"type": "model_output", "content": [
                {"type": "text", "text": "Here you go"},
                {"type": "image", "data": "AAAA", "mime_type": "image/png"},
                {"type": "image", "data": "AQID", "mime_type": "image/jpeg"}
            ]}]
        }))
        .unwrap();
        let m = last_media(&i, "image").unwrap();
        assert_eq!(m.bytes, vec![1, 2, 3]);
        assert_eq!(m.mime_type.as_deref(), Some("image/jpeg"));
        assert_eq!(output_text(&i), "Here you go");
        assert!(last_media(&i, "audio").is_err());
    }

    #[test]
    fn failed_status_is_an_error() {
        let i: Interaction =
            serde_json::from_value(json!({"status": "failed", "steps": []})).unwrap();
        assert!(last_media(&i, "image").is_err());
    }
}
