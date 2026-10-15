//! Voice catalogue shown in Settings.

use serde::Serialize;

use crate::gemini::tts::PREBUILT_VOICES;
use crate::pipeline::project::Language;

#[derive(Debug, Clone, Serialize)]
pub struct VoiceInfo {
    pub id: String,
    pub label: String,
    pub language: Language,
}

/// Edge neural voices we offer. Khmer has exactly two.
pub const EDGE_VOICES: &[(&str, &str, Language)] = &[
    ("km-KH-SreymomNeural", "Sreymom (female)", Language::Km),
    ("km-KH-PisethNeural", "Piseth (male)", Language::Km),
    ("en-US-AndrewNeural", "Andrew (male)", Language::En),
    ("en-US-AvaNeural", "Ava (female)", Language::En),
    ("en-US-BrianNeural", "Brian (male)", Language::En),
    ("en-US-EmmaNeural", "Emma (female)", Language::En),
    ("en-US-GuyNeural", "Guy (male)", Language::En),
    ("en-US-JennyNeural", "Jenny (female)", Language::En),
    ("en-US-AriaNeural", "Aria (female)", Language::En),
    (
        "en-US-ChristopherNeural",
        "Christopher (male)",
        Language::En,
    ),
];

#[derive(Debug, Clone, Serialize)]
pub struct VoiceCatalog {
    pub edge: Vec<VoiceInfo>,
    /// Gemini voices are multilingual (language detected from the text).
    pub gemini: Vec<(String, String)>,
}

pub fn catalog() -> VoiceCatalog {
    VoiceCatalog {
        edge: EDGE_VOICES
            .iter()
            .map(|(id, label, language)| VoiceInfo {
                id: (*id).to_owned(),
                label: (*label).to_owned(),
                language: *language,
            })
            .collect(),
        gemini: PREBUILT_VOICES
            .iter()
            .map(|(name, character)| ((*name).to_owned(), (*character).to_owned()))
            .collect(),
    }
}

pub fn is_edge_voice(id: &str) -> bool {
    EDGE_VOICES.iter().any(|v| v.0 == id)
}

pub fn is_gemini_voice(id: &str) -> bool {
    PREBUILT_VOICES.iter().any(|v| v.0 == id)
}
