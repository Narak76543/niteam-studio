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
    /// "Real-world Tech Explainer": scenes get a visual type (real photo, stock video,
    /// diagram, code…) and the image providers are only the fallback
    /// (`docs/11-real-world-visuals.md`).
    pub real_world: bool,
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
        real_world: false,
    },
    StylePreset {
        id: "storybook_watercolor",
        label: "Storybook watercolor",
        style_prompt: "warm watercolor storybook illustration, soft natural light, gentle textured paper, \
                       muted earthy palette, hand-painted details, calm and inviting",
        prompt_rules: "",
        real_world: false,
    },
    StylePreset {
        id: "cinematic_realistic",
        label: "Cinematic realistic",
        style_prompt: "cinematic realistic digital painting, dramatic natural lighting, shallow depth of field, \
                       rich color grading, wide establishing composition, highly detailed",
        prompt_rules: "",
        real_world: false,
    },
    StylePreset {
        id: "flat_2d",
        label: "Flat 2D",
        style_prompt: "clean flat 2D vector illustration, simple geometric shapes, limited harmonious palette, \
                       soft shadows, friendly modern explainer style",
        prompt_rules: "",
        real_world: false,
    },
    // Owner, 2026-10-17: calm explainer look, outlined round-headed figures on soft painted
    // backgrounds. Z-Image runs at cfg 1.0 (no negative prompt), so "no ..." rules stay in
    // the positive prompt.
    StylePreset {
        id: "soft_2d_explainer",
        label: "Soft 2D Explainer",
        style_prompt: "polished flat 2D educational cartoon; recurring friendly stickman guide with round peach head, \
                       dot eyes, small smile, thin black stick limbs and small teal scarf; crisp silhouettes and \
                       thick clean outlines; soft painterly backgrounds without outlines; muted sage green, grey-blue \
                       and earth tones, gentle light, subtle texture, one clear focal subject",
        prompt_rules: SOFT_2D_RULES,
        real_world: false,
    },
    // Owner, 2026-10-17: real photos, stock clips, diagrams and code; this style prompt is
    // only used for scenes that fall back to an AI image.
    StylePreset {
        id: "tech_explainer",
        label: "Real-world Tech Explainer",
        style_prompt: "clean realistic technology photo illustration, modern equipment and places, natural soft \
                       light, neutral colours, sharp focus, documentary feel",
        prompt_rules: "no text, no letters, no logos, no watermark",
        real_world: true,
    },
];

/// The one figure design of `soft_2d_explainer`, repeated in every prompt (with the
/// background and no-text rules).
const SOFT_2D_RULES: &str = "Faithfully show the exact named subject and action in this scene as the main focal \
                             point; never replace it with a generic person, group, landscape or symbol. For historical \
                             or biographical stories, depict named historical figures with authentic likeness and period clothing; \
                             otherwise a friendly explainer guide demonstrates the subject. Use only props, people and settings that \
                             fit the topic, narration and scene's actual time period; do not add unrelated modern technology, \
                             historical objects or scenery. Vary setting, composition, props and pose; no generic \
                             meadow, classroom or crowd unless the scene needs one. Keep backgrounds soft and unoutlined. no text, no \
                             letters, no watermark; no words, no Chinese characters, no Hanzi, no pseudo-text, no numbers, no signs, no labels, no posters, no banners, no title cards, no speech balloons, no logos \
                             or badges; interpret topic wording as meaning, never as text to copy";

/// Topic-sensitive visual guidance for prompts and final image requests. Check history first so a
/// historical technology topic does not automatically become a modern computer scene.
pub fn topic_visual_rules(texts: &[&str]) -> &'static str {
    if is_prehistoric_topic(texts) {
        "accurately depict the named prehistoric creature, plants, terrain and period-appropriate setting; \
         show no modern tools, computers, buildings or modern clothing unless explicitly needed for a historical comparison."
    } else if is_historical_topic(texts) {
        "accurately depict the authentic historical era, location, period-accurate clothing, tools, workshop equipment \
         and architecture; all people and objects must faithfully match the historical period."
    } else if is_technology_topic(texts) {
        "accurately depict the specific machinery, physical components, laboratory instruments and practical tools \
         named in this scene; focus on concrete physical objects without modern computer clutter or unrelated screens."
    } else {
        "choose concrete visual details directly from this scene's narration; do not invent unrelated props or scenery."
    }
}

pub fn is_technology_topic(texts: &[&str]) -> bool {
    const TERMS: &[&str] = &[
        "ai", "artificial intelligence", "agentic", "computer", "technology",
        "software", "programming", "coding", "developer", "code", "algorithm", "machine learning",
        "robotics", "robot", "internet", "api", "cpu", "gpu", "digital", "cyber",
        "បច្ចេកវិទ្យា", "កុំព្យូទ័រ",
    ];
    texts.iter().any(|text| contains_topic_term(text, TERMS))
}

pub fn is_historical_topic(texts: &[&str]) -> bool {
    const TERMS: &[&str] = &[
        "history", "historical", "ancient", "prehistoric", "medieval", "civilization", "dynasty",
        "empire", "world war", "wwii", "ww2", "archaeology", "archaeological", "ដាយណូស័រ",
        "បុរាណ", "ប្រវត្តិសាស្ត្រ",
    ];
    texts.iter().any(|text| contains_topic_term(text, TERMS))
}

pub fn is_prehistoric_topic(texts: &[&str]) -> bool {
    const TERMS: &[&str] = &["dinosaur", "dinosaurs", "prehistoric", "ដាយណូស័រ"];
    texts.iter().any(|text| contains_topic_term(text, TERMS))
}

fn contains_topic_term(text: &str, terms: &[&str]) -> bool {
    let lower = text.to_lowercase();
    terms.iter().any(|term| {
        if term.chars().all(|ch| ch.is_ascii_alphanumeric()) {
            lower
                .split(|ch: char| !ch.is_ascii_alphanumeric())
                .any(|word| word == *term)
        } else {
            lower.contains(term)
        }
    })
}

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
    if s.contains("round bald") || s.contains("round peach") || s.contains("soft 2d") {
        Figures::SharedCartoon
    } else if s.contains("stick figure") || s.contains("stickman") || s.contains("stick-figure") {
        Figures::StickFigure
    } else {
        Figures::Free
    }
}

/// Scenes of this preset get visual types (`tech_explainer`).
pub fn is_real_world(preset_id: &str) -> bool {
    find(preset_id).is_some_and(|p| p.real_world)
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

    #[test]
    fn only_the_tech_explainer_is_real_world() {
        assert!(is_real_world("tech_explainer"));
        assert!(!is_real_world("stickman") && !is_real_world("nope"));
        assert_eq!(figures(find("tech_explainer").unwrap().style_prompt), Figures::Free);
    }
}
