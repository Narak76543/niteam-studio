//! "Fix with Gemini" for a diagram whose Mermaid doesn't parse (`docs/11-real-world-visuals.md`
//! §10): one text request with the parser's error; the reply must start with an allowed header.

use serde::Deserialize;
use serde_json::{Value, json};

use super::client::ModelChain;
use super::models::Content;
use super::retry::RetryHooks;
use super::script::{Validate, mermaid_template, request_json_with};
use super::{GeminiClient, GeminiError};
use crate::pipeline::project::Language;
use crate::storage::ApiKey;

const SYSTEM: &str = "You repair Mermaid diagrams for an explainer video. Keep the meaning, the node \
order and the label language. Reply with JSON only, matching the given schema.";

#[derive(Debug, Deserialize)]
pub struct FixReply {
    pub mermaid: String,
}

impl Validate for FixReply {
    fn validate(&self) -> Result<(), String> {
        mermaid_template(&self.mermaid)
            .map(|_| ())
            .ok_or_else(|| "`mermaid` must start with `flowchart LR`, `flowchart TD`, `sequenceDiagram` or `timeline`".into())
    }
}

pub fn schema() -> Value {
    json!({ "type": "object", "properties": { "mermaid": { "type": "string" } }, "required": ["mermaid"] })
}

pub fn prompt(mermaid: &str, error: &str, language: Language) -> String {
    let labels = match language {
        Language::Km => "Khmer (product and technology names stay in Latin letters)",
        Language::En => "English",
    };
    format!(
        "This Mermaid diagram does not parse with Mermaid 11.\n\
ERROR: {error}\n\
DIAGRAM:\n{mermaid}\n\n\
Return the corrected diagram in `mermaid`. Rules: start with `flowchart LR`, `flowchart TD`, `sequenceDiagram` or \
`timeline`; at most 8 nodes or steps; node ids plain ASCII letters (A, B, C); every label in double quotes, at most 5 \
words, in {labels}; one label per line; no styling, classDef, click, links, emoji, HTML, markdown or comments."
    )
}

pub async fn fix(
    client: &GeminiClient,
    key: &ApiKey,
    chain: &ModelChain,
    mermaid: &str,
    error: &str,
    language: Language,
    hooks: &RetryHooks,
) -> Result<String, GeminiError> {
    let (reply, _model): (FixReply, String) =
        request_json_with(client, key, chain, SYSTEM, Content::user(prompt(mermaid, error, language)), schema(), hooks).await?;
    Ok(reply.mermaid.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_has_the_error_and_rules_and_replies_are_checked() {
        let p = prompt("flowchart LR\n A --> ", "Parse error on line 2", Language::Km);
        assert!(p.contains("Parse error on line 2") && p.contains("in Khmer"));
        assert!(FixReply { mermaid: "flowchart LR\n A[\"x\"] --> B[\"y\"]".into() }.validate().is_ok());
        assert!(FixReply { mermaid: "A --> B".into() }.validate().is_err());
    }
}
