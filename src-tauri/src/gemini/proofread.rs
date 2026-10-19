//! Grammar / phrasing / spelling check of the narration with the Gemini text model
//! (Script page → "Check grammar"). Uses the same structured-JSON request as the script
//! (`script::request_json`: model chain, retries, one re-ask on invalid JSON).

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::script::{Validate, foreign_script, names_rule};
use crate::pipeline::project::Language;

/// One scene sent to the proofreader.
#[derive(Debug, Clone, Serialize)]
pub struct ProofInput<'a> {
    pub scene_id: &'a str,
    pub narration: &'a str,
    pub spoken_text: &'a str,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProofReply {
    pub fixes: Vec<ProofFix>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProofFix {
    pub scene_id: String,
    pub narration: String,
    pub spoken_text: String,
    pub note: String,
}

impl Validate for ProofReply {
    fn validate(&self) -> Result<(), String> {
        for f in &self.fixes {
            if f.narration.trim().is_empty() || f.spoken_text.trim().is_empty() {
                return Err(format!("fix for {} has an empty narration or spoken_text", f.scene_id));
            }
            for (field, text) in [("narration", &f.narration), ("spoken_text", &f.spoken_text)] {
                if let Some(c) = foreign_script(text) {
                    return Err(format!(
                        "fix for {} {field} contains a Thai/Lao character '{c}'; use Khmer script only",
                        f.scene_id
                    ));
                }
            }
        }
        Ok(())
    }
}

pub fn schema(scene_ids: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": {
            "fixes": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "scene_id": { "type": "string", "enum": scene_ids },
                        "narration": { "type": "string" },
                        "spoken_text": { "type": "string" },
                        "note": { "type": "string" }
                    },
                    "required": ["scene_id", "narration", "spoken_text", "note"]
                }
            }
        },
        "required": ["fixes"]
    })
}

pub fn prompt(language: Language, topic: &str, keep_terms: &[String], scenes: &[ProofInput<'_>]) -> String {
    let (lang, spoken_rule, note_lang) = match language {
        Language::Km => (
            "Khmer",
            "`spoken_text` must say exactly what `narration` says, written for the Khmer text-to-speech voice: \
numbers, years, percentages, units and abbreviations written out in Khmer words, no digits.",
            "Khmer",
        ),
        Language::En => (
            "English",
            "`spoken_text` must say exactly what `narration` says, written for text-to-speech: numbers, years, \
units and abbreviations written out as words, no digits.",
            "English",
        ),
    };
    let scenes_json = serde_json::to_string_pretty(scenes).unwrap_or_default();
    format!(
        "You are a careful {lang} copy editor. Proofread the narration of a narrated explainer video.\n\
TOPIC: {topic}\n\n\
Check every scene for:\n\
- spelling mistakes and typos (for Khmer: wrong subscript consonants / coeng, wrong vowels or signs, misspelled words)\n\
- grammar mistakes\n\
- awkward or unnatural phrasing — it must sound like natural spoken {lang}, the way a good storyteller talks, \
not a literal translation\n\
- `narration` and `spoken_text` that do not say the same thing\n\
- repeated sentences or scenes that explain the same point: compare scenes in the order provided. Keep the first clear version and suggest a fix for the later scene only when it repeats a full sentence or substantially repeats the same explanation. Repeated technical terms are fine; preserve distinct examples, useful callbacks and facts.\n\n\
Rules:\n\
- Make the SMALLEST change that fixes the problem. For repetition, change only the later scene so it advances the explanation with a new detail or removes the repeated sentence. Keep the meaning, facts, tone and length (one or two short sentences).\n\
- Do not rewrite a scene that is already correct and natural.\n\
{names}\n\
- {spoken_rule}\n\
- Return in `fixes` ONLY the scenes that need a change, each with the full corrected `narration` and `spoken_text` \
and a short `note` in {note_lang} that says what you changed. If every scene is correct, return \"fixes\": [].\n\n\
SCENES:\n{scenes_json}",
        names = names_rule(language, keep_terms),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_lists_scenes_and_keeps_names() {
        let scenes = [ProofInput { scene_id: "s001", narration: "React Native ជាអ្វី?", spoken_text: "x" }];
        let p = prompt(Language::Km, "React Native", &["React Native".into()], &scenes);
        assert!(p.contains("\"scene_id\": \"s001\""));
        assert!(p.contains("must appear EXACTLY as written"));
        assert!(p.contains("SMALLEST change"));
    }

    #[test]
    fn schema_restricts_scene_ids() {
        let s = schema(&["s001", "s002"]);
        assert_eq!(s["properties"]["fixes"]["items"]["properties"]["scene_id"]["enum"], json!(["s001", "s002"]));
    }

    #[test]
    fn replies_with_thai_letters_or_empty_text_are_rejected() {
        let fix = |n: &str| ProofReply {
            fixes: vec![ProofFix { scene_id: "s001".into(), narration: n.into(), spoken_text: "ក".into(), note: "".into() }],
        };
        assert!(fix("ញើសឈាមរបស់บรรพ").validate().is_err());
        assert!(fix(" ").validate().is_err());
        assert!(fix("ត្រឹមត្រូវ").validate().is_ok());
        assert!(ProofReply { fixes: vec![] }.validate().is_ok());
    }
}
