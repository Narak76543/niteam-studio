//! Built-in visual style presets. `style_prompt` is prefixed to every image prompt.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct StylePreset {
    pub id: &'static str,
    /// English label (UI shows the i18n string `preset.<id>`).
    pub label: &'static str,
    pub style_prompt: &'static str,
    /// Added word for word near the end of every final image prompt (after the scene), so
    /// it holds even when the Visual Bible's `style` line paraphrases `style_prompt`.
    pub prompt_rules: &'static str,
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
        prompt_rules: "",
    },
    StylePreset {
        id: "storybook_watercolor",
        label: "Storybook watercolor",
        style_prompt: "warm watercolor storybook illustration, soft natural light, gentle textured paper, \
                       muted earthy palette, hand-painted details, calm and inviting",
        prompt_rules: "",
    },
    StylePreset {
        id: "cinematic_realistic",
        label: "Cinematic realistic",
        style_prompt: "cinematic realistic digital painting, dramatic natural lighting, shallow depth of field, \
                       rich color grading, wide establishing composition, highly detailed",
        prompt_rules: "",
    },
    StylePreset {
        id: "flat_2d",
        label: "Flat 2D",
        style_prompt: "clean flat 2D vector illustration, simple geometric shapes, limited harmonious palette, \
                       soft shadows, friendly modern explainer style",
        prompt_rules: "",
    },
    // Owner, 2026-10-17: calm explainer look, outlined round-headed figures on soft painted
    // backgrounds. Z-Image runs at cfg 1.0 (no negative prompt), so "no ..." rules stay in
    // the positive prompt.
    StylePreset {
        id: "soft_2d_explainer",
        label: "Soft 2D Explainer",
        style_prompt: "flat 2D cartoon illustration, simple calm explainer-video style, characters with round \
                       bald peach-coloured heads, dot eyes, small smiles, thin stick legs and clean thick black \
                       outlines, simple painterly background with soft brush shading and no line art, few \
                       background elements, distant layers fading into mist, muted green, sage and grey-blue \
                       palette with soft earth tones, gentle lighting, lots of empty space",
        prompt_rules: SOFT_2D_RULES,
    },
];

/// The one figure design of `soft_2d_explainer`, repeated in every prompt (with the
/// background and no-text rules).
const SOFT_2D_RULES: &str = "every person drawn with the same simple design: round bald peach-coloured head, \
                             dot eyes, small smile, thin stick legs, clean thick black outline; only the \
                             people have outlines, buildings, hills and plants are soft brush shading without \
                             line art; no text, no letters, no watermark";

pub fn find(id: &str) -> Option<&'static StylePreset> {
    PRESETS.iter().find(|p| p.id == id)
}

/// How people are drawn, which decides how the Visual Bible and scene prompts describe
/// them. Read from the style text, so an edited style keeps its rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Figures {
    /// Any look: characters get full descriptions.
    Free,
    /// Stick figures: only props and colour accents.
    StickFigure,
    /// One shared round-headed cartoon design (`soft_2d_explainer`): only clothing, one
    /// prop, headwear.
    SharedCartoon,
}

pub fn figures(style_prompt: &str) -> Figures {
    let s = style_prompt.to_ascii_lowercase();
    if s.contains("round bald") {
        Figures::SharedCartoon
    } else if s.contains("stick figure") || s.contains("stickman") || s.contains("stick-figure") {
        Figures::StickFigure
    } else {
        Figures::Free
    }
}

/// The preset's fixed prompt rules (empty for unknown ids and most presets).
pub fn prompt_rules(preset_id: &str) -> &'static str {
    find(preset_id).map_or("", |p| p.prompt_rules)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stickman_is_first_and_detected() {
        assert_eq!(PRESETS[0].id, "stickman");
        assert_eq!(figures(PRESETS[0].style_prompt), Figures::StickFigure);
        assert_eq!(figures(PRESETS[1].style_prompt), Figures::Free);
    }

    #[test]
    fn soft_2d_has_shared_figures_and_no_text_rules() {
        let p = find("soft_2d_explainer").unwrap();
        // "thin stick legs" must not make it a stick-figure style.
        assert_eq!(figures(p.style_prompt), Figures::SharedCartoon);
        assert!(p.prompt_rules.contains("no text, no letters, no watermark"));
        assert_eq!(prompt_rules("stickman"), "");
        assert_eq!(prompt_rules("nope"), "");
    }
}
