//! Serde types for the Gemini REST API (camelCase on the wire).
//! Only the fields we use are modeled; unknown response fields are ignored.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::GeminiError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Content {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default)]
    pub parts: Vec<Part>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Part {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thought: Option<bool>,
}

impl Content {
    pub fn user(text: impl Into<String>) -> Self {
        Self::with_role("user", text)
    }

    pub fn model(text: impl Into<String>) -> Self {
        Self::with_role("model", text)
    }

    fn with_role(role: &str, text: impl Into<String>) -> Self {
        Self {
            role: Some(role.into()),
            parts: vec![Part {
                text: Some(text.into()),
                thought: None,
            }],
        }
    }

    pub fn system(text: impl Into<String>) -> Self {
        Self {
            role: None,
            parts: vec![Part {
                text: Some(text.into()),
                thought: None,
            }],
        }
    }
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GenerateContentRequest {
    pub contents: Vec<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GenerationConfig>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GenerationConfig {
    /// Current (non-deprecated) way to request structured JSON output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormatConfig>,
    /// Fallback pair for models that don't accept `responseFormat` yet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_json_schema: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseFormatConfig {
    pub text: TextResponseFormat,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextResponseFormat {
    pub mime_type: String,
    pub schema: Value,
}

impl GenerationConfig {
    /// JSON output constrained by a JSON Schema.
    pub fn json(schema: Value, legacy_fields: bool) -> Self {
        if legacy_fields {
            Self {
                response_mime_type: Some("application/json".into()),
                response_json_schema: Some(schema),
                ..Default::default()
            }
        } else {
            Self {
                response_format: Some(ResponseFormatConfig {
                    text: TextResponseFormat {
                        mime_type: "APPLICATION_JSON".into(),
                        schema,
                    },
                }),
                ..Default::default()
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GenerateContentResponse {
    #[serde(default)]
    pub candidates: Vec<Candidate>,
    pub prompt_feedback: Option<PromptFeedback>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub content: Option<Content>,
    pub finish_reason: Option<String>,
    pub finish_message: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptFeedback {
    pub block_reason: Option<String>,
}

const BLOCK_REASONS: &[&str] = &[
    "SAFETY",
    "RECITATION",
    "BLOCKLIST",
    "PROHIBITED_CONTENT",
    "SPII",
    "IMAGE_SAFETY",
    "IMAGE_PROHIBITED_CONTENT",
];

impl GenerateContentResponse {
    /// Concatenated non-thought text of the first candidate.
    pub fn text(&self) -> Result<String, GeminiError> {
        if let Some(reason) = self
            .prompt_feedback
            .as_ref()
            .and_then(|f| f.block_reason.as_ref())
        {
            return Err(GeminiError::Blocked(reason.clone()));
        }
        let cand = self
            .candidates
            .first()
            .ok_or_else(|| GeminiError::BadResponse("no candidates".into()))?;
        let reason = cand.finish_reason.as_deref().unwrap_or("STOP");
        if BLOCK_REASONS.contains(&reason) {
            let msg = cand
                .finish_message
                .clone()
                .unwrap_or_else(|| reason.to_owned());
            return Err(GeminiError::Blocked(msg));
        }
        if reason == "MAX_TOKENS" {
            return Err(GeminiError::BadResponse(
                "the answer was cut off (too long)".into(),
            ));
        }
        let text: String = cand
            .content
            .iter()
            .flat_map(|c| c.parts.iter())
            .filter(|p| p.thought != Some(true))
            .filter_map(|p| p.text.as_deref())
            .collect();
        if text.trim().is_empty() {
            return Err(GeminiError::BadResponse(format!("empty answer ({reason})")));
        }
        Ok(text)
    }
}

/// Subset of the `Model` resource returned by `models.get`.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub name: String,
    pub display_name: Option<String>,
    pub input_token_limit: Option<u64>,
    pub output_token_limit: Option<u64>,
}

/// Error body. `code` is an int in the classic shape and a string in the newer one,
/// so only `message` is relied on.
#[derive(Debug, Deserialize)]
pub struct ErrorEnvelope {
    pub error: ErrorBody,
}

#[derive(Debug, Deserialize)]
pub struct ErrorBody {
    #[serde(default)]
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_uses_camel_case_and_response_format() {
        let req = GenerateContentRequest {
            contents: vec![Content::user("hi")],
            system_instruction: Some(Content::system("be brief")),
            generation_config: Some(GenerationConfig::json(json!({"type":"object"}), false)),
        };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["contents"][0]["role"], "user");
        assert_eq!(v["contents"][0]["parts"][0]["text"], "hi");
        assert_eq!(v["systemInstruction"]["parts"][0]["text"], "be brief");
        assert_eq!(
            v["generationConfig"]["responseFormat"]["text"]["mimeType"],
            "APPLICATION_JSON"
        );
        assert!(v["generationConfig"].get("responseMimeType").is_none());
    }

    #[test]
    fn legacy_json_fields() {
        let v = serde_json::to_value(GenerationConfig::json(json!({}), true)).unwrap();
        assert_eq!(v["responseMimeType"], "application/json");
        assert!(v.get("responseJsonSchema").is_some());
    }

    #[test]
    fn extracts_text_skipping_thoughts() {
        let r: GenerateContentResponse = serde_json::from_value(json!({
            "candidates": [{
                "content": {"role": "model", "parts": [
                    {"text": "thinking...", "thought": true},
                    {"text": "{\"a\":"}, {"text": "1}"}
                ]},
                "finishReason": "STOP"
            }],
            "usageMetadata": {"totalTokenCount": 10}
        }))
        .unwrap();
        assert_eq!(r.text().unwrap(), "{\"a\":1}");
    }

    #[test]
    fn blocked_prompt_is_reported() {
        let r: GenerateContentResponse =
            serde_json::from_value(json!({"promptFeedback": {"blockReason": "SAFETY"}})).unwrap();
        assert!(matches!(r.text(), Err(GeminiError::Blocked(_))));
    }

    #[test]
    fn error_envelope_accepts_both_code_shapes() {
        let a: ErrorEnvelope = serde_json::from_str(
            r#"{"error":{"code":400,"message":"bad","status":"INVALID_ARGUMENT"}}"#,
        )
        .unwrap();
        let b: ErrorEnvelope =
            serde_json::from_str(r#"{"error":{"code":"invalid_request","message":"bad"}}"#)
                .unwrap();
        assert_eq!(a.error.message, "bad");
        assert_eq!(b.error.message, "bad");
    }
}
