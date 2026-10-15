//! Gemini image generation ("Nano Banana", paid only) through the Interactions API, as
//! documented on ai.google.dev/gemini-api/docs/image-generation (checked 2026-10-15):
//! `{ model, input: [{type:"text", text}, {type:"image", mime_type, data}…],
//!    response_format: {type:"image", aspect_ratio:"16:9", image_size:"1K"}, store:false }`
//! → last `steps[model_output].content[type=image]` (base64).
//! `image_size: "1K"` is the only size every model supports (3.1 Flash Lite Image is 1K only).

use base64::Engine as _;
use serde::Serialize;

use super::GeminiError;
use super::client::{EndpointKind, GeminiClient};
use super::interactions::{Interaction, Media, last_media, output_text};
use super::retry::RetryHooks;
use crate::storage::ApiKey;

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum InputBlock<'a> {
    Text { text: &'a str },
    Image { mime_type: &'a str, data: String },
}

#[derive(Serialize)]
struct ImageFormat {
    #[serde(rename = "type")]
    kind: &'static str,
    aspect_ratio: &'static str,
    image_size: &'static str,
}

#[derive(Serialize)]
struct ImageRequestBody<'a> {
    model: &'a str,
    input: Vec<InputBlock<'a>>,
    response_format: ImageFormat,
    store: bool,
}

/// A reference image passed to the model (style / character consistency).
pub struct Reference {
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

fn build_request<'a>(
    model: &'a str,
    prompt: &'a str,
    refs: &'a [Reference],
) -> ImageRequestBody<'a> {
    let mut input = vec![InputBlock::Text { text: prompt }];
    for r in refs {
        input.push(InputBlock::Image {
            mime_type: &r.mime_type,
            data: base64::engine::general_purpose::STANDARD.encode(&r.bytes),
        });
    }
    ImageRequestBody {
        model,
        input,
        response_format: ImageFormat {
            kind: "image",
            aspect_ratio: "16:9",
            image_size: "1K",
        },
        store: false,
    }
}

/// Generated image, or the model's text explaining why there is none.
pub enum ImageAnswer {
    Image(Media),
    Refused(String),
}

impl GeminiClient {
    /// One image request. `max_attempts` is 1 when the caller runs its own retry loop.
    pub async fn generate_image(
        &self,
        key: &ApiKey,
        model: &str,
        prompt: &str,
        refs: &[Reference],
        hooks: &RetryHooks,
        max_attempts: u32,
    ) -> Result<ImageAnswer, GeminiError> {
        let body = build_request(model, prompt, refs);
        let interaction: Interaction = self
            .create_interaction(key, model, EndpointKind::Image, &body, hooks, max_attempts)
            .await?;
        match last_media(&interaction, "image") {
            Ok(media) => Ok(ImageAnswer::Image(media)),
            Err(e) => {
                let text = output_text(&interaction);
                if text.trim().is_empty() {
                    Err(e)
                } else {
                    Ok(ImageAnswer::Refused(text))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_matches_documented_shape() {
        let refs = [Reference {
            mime_type: "image/jpeg".into(),
            bytes: vec![1, 2, 3],
        }];
        let v = serde_json::to_value(build_request(
            "gemini-3.1-flash-lite-image",
            "a leaf",
            &refs,
        ))
        .unwrap();
        assert_eq!(
            v,
            json!({
                "model": "gemini-3.1-flash-lite-image",
                "input": [
                    {"type": "text", "text": "a leaf"},
                    {"type": "image", "mime_type": "image/jpeg", "data": "AQID"}
                ],
                "response_format": {"type": "image", "aspect_ratio": "16:9", "image_size": "1K"},
                "store": false
            })
        );
    }
}
