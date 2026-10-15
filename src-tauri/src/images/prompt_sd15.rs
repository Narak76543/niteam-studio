//! Prompt adapter for SD 1.5 (`docs/08-local-gpu.md` §7). Its CLIP text encoder only reads
//! ~77 tokens, so the prompt is short comma-separated phrases, most important first:
//! style → subject (with short character descriptions) → shot → place. Lettering is kept out
//! with the negative prompt instead of words in the prompt.

use super::prompt::{PromptParts, inline_characters, shot_phrase};

/// Fixed negative prompt (small effect at LCM's low CFG, but cheap).
pub const NEGATIVE: &str = "text, watermark, letters, blurry, deformed hands, extra fingers";
/// Token budget for the positive prompt (77 minus start/end tokens).
pub const MAX_TOKENS: usize = 75;
/// Character / place descriptions are cut to this many words (08 §7 fallback).
const SHORT_WORDS: usize = 12;

/// Rough CLIP token estimate: words × 1.3 (08 §7).
pub fn estimate_tokens(text: &str) -> usize {
    let words = text.split(|c: char| c.is_whitespace() || c == ',').filter(|w| !w.is_empty()).count();
    (words * 13).div_ceil(10)
}

fn first_words(text: &str, n: usize) -> String {
    text.split_whitespace().take(n).collect::<Vec<_>>().join(" ").trim_end_matches(',').to_owned()
}

fn join(parts: &[&str]) -> String {
    parts.iter().map(|p| p.trim()).filter(|p| !p.is_empty()).collect::<Vec<_>>().join(", ")
}

/// SD 1.5 prompt within `MAX_TOKENS`. `short_style` is the preset's short style text.
pub fn build_prompt_sd15(p: &PromptParts, short_style: &str) -> String {
    let characters: Vec<(String, String)> = p
        .characters
        .iter()
        .map(|(k, d)| ((*k).to_owned(), first_words(d, SHORT_WORDS)))
        .collect();
    let refs: Vec<(&str, &str)> = characters.iter().map(|(k, d)| (k.as_str(), d.as_str())).collect();
    let subject = inline_characters(p.subject, &refs);
    let place = p.place.map(|pl| first_words(pl, SHORT_WORDS)).unwrap_or_default();
    let shot = shot_phrase(p.shot);

    // Drop the least important parts first: place, then shot; then cut the subject.
    for (with_shot, with_place) in [(true, true), (true, false), (false, false)] {
        let prompt = join(&[short_style, &subject, if with_shot { shot } else { "" }, if with_place { &place } else { "" }]);
        if estimate_tokens(&prompt) <= MAX_TOKENS {
            return prompt;
        }
    }
    let room_words = (MAX_TOKENS * 10 / 13).saturating_sub(short_style.split_whitespace().count());
    join(&[short_style, &first_words(&subject, room_words)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::project::Shot;

    const SHORT: &str = "stick figure doodle, black marker lines, white background, flat colour accents";

    fn parts<'a>(subject: &'a str, characters: Vec<(&'a str, &'a str)>, place: Option<&'a str>) -> PromptParts<'a> {
        PromptParts { style: "unused full style", shot: Shot::Wide, subject, characters, place, culture_notes: None, square: false }
    }

    #[test]
    fn order_is_style_subject_shot_place() {
        let p = build_prompt_sd15(
            &parts("the farmer points at the temple", vec![("farmer", "stick figure with a straw hat")], Some("temple with three towers")),
            SHORT,
        );
        assert_eq!(
            p,
            "stick figure doodle, black marker lines, white background, flat colour accents, \
             the farmer (stick figure with a straw hat) points at the temple, wide establishing shot, \
             temple with three towers"
        );
    }

    #[test]
    fn long_descriptions_are_shortened_and_budget_is_kept() {
        let long = "stick figure with a red and white checked krama scarf, a wide straw hat, a small bag, sandals and a walking stick";
        let place = "simple line drawing of a huge temple with three lotus shaped towers, moat, causeway, palm trees, rice fields and mountains";
        let subject = "the farmer walks along the causeway towards the temple while monks carry offerings and children play near the water at sunrise";
        let p = build_prompt_sd15(&parts(subject, vec![("farmer", long)], Some(place)), SHORT);
        assert!(estimate_tokens(&p) <= MAX_TOKENS, "{} tokens: {p}", estimate_tokens(&p));
        assert!(p.starts_with(SHORT));
        assert!(!p.contains("walking stick"), "character description cut to 12 words");
    }

    #[test]
    fn huge_subject_is_cut_but_style_stays() {
        let subject = "word ".repeat(200);
        let p = build_prompt_sd15(&parts(&subject, vec![], None), SHORT);
        assert!(p.starts_with(SHORT));
        assert!(estimate_tokens(&p) <= MAX_TOKENS);
    }

    #[test]
    fn negative_prompt_keeps_lettering_out() {
        assert!(NEGATIVE.contains("text") && NEGATIVE.contains("letters"));
        assert_eq!(estimate_tokens("a, b c"), 4);
    }
}
