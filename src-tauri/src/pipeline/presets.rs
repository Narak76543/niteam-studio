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
    // Default (owner's choice, 2026-10-15).
    StylePreset {
        id: "stickman",
        label: "Stickman",
        // Fuller scenes (owner, 2026-10-16): background, setting and props drawn in the same
        // line style instead of "lots of white space".
        style_prompt: "simple hand-drawn stick figure doodle, bold clean black marker lines, flat pure white \
                       background filling the whole frame, stick figures with round heads and simple dot eyes, \
                       complete scene drawn in the same doodle lines with a background setting and several \
                       topic props, flat colour accents, whiteboard explainer style",
    },
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

/// Stick-figure styles need simple, prop-based character descriptions (visual bible).
pub fn is_stick_figure(style_prompt: &str) -> bool {
    let s = style_prompt.to_ascii_lowercase();
    s.contains("stick figure") || s.contains("stickman") || s.contains("stick-figure")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stickman_is_first_and_detected() {
        assert_eq!(PRESETS[0].id, "stickman");
        assert!(is_stick_figure(PRESETS[0].style_prompt));
        assert!(!is_stick_figure(PRESETS[1].style_prompt));
    }
}
