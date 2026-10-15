//! Built-in visual style presets. `style_prompt` is prefixed to every image prompt.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct StylePreset {
    pub id: &'static str,
    /// English label (UI shows the i18n string `preset.<id>`).
    pub label: &'static str,
    pub style_prompt: &'static str,
}

pub const PRESETS: &[StylePreset] = &[
    StylePreset {
        id: "storybook_watercolor",
        label: "Storybook watercolor",
        style_prompt: "warm watercolor storybook illustration, soft natural light, gentle textured paper, \
                       muted earthy palette, hand-painted details, calm and inviting",
    },
    StylePreset {
        id: "cinematic_realistic",
        label: "Cinematic realistic",
        style_prompt: "cinematic realistic digital painting, dramatic natural lighting, shallow depth of field, \
                       rich color grading, wide establishing composition, highly detailed",
    },
    StylePreset {
        id: "flat_2d",
        label: "Flat 2D",
        style_prompt: "clean flat 2D vector illustration, simple geometric shapes, limited harmonious palette, \
                       soft shadows, friendly modern explainer style",
    },
];

pub fn find(id: &str) -> Option<&'static StylePreset> {
    PRESETS.iter().find(|p| p.id == id)
}
