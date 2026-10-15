//! Prompt building (`docs/07-image-generation.md` §3). Pure functions, no I/O.
//!
//! `{style}, {shot phrase}, {subject with bible descriptions inlined}, {place},
//!  {culture notes}, {composition phrase (square providers)}, clean image without any lettering`

use sha2::{Digest, Sha256};

use crate::pipeline::project::Shot;

/// Flux Schnell prompt limit.
pub const MAX_PROMPT_CHARS: usize = 2048;
pub const NO_LETTERING: &str = "clean image without any lettering";
/// For square providers: the image is center-cropped to 16:9 afterwards.
pub const SQUARE_COMPOSITION: &str =
    "wide horizontal composition, main subject in the center band, empty space above and below";

pub fn shot_phrase(shot: Shot) -> &'static str {
    match shot {
        Shot::Wide => "wide establishing shot",
        Shot::Medium => "medium shot",
        Shot::Close => "close-up portrait",
        Shot::Detail => "close-up detail shot",
        Shot::Aerial => "aerial view",
    }
}

pub struct PromptParts<'a> {
    pub style: &'a str,
    pub shot: Shot,
    /// The scene's English `image_prompt`.
    pub subject: &'a str,
    /// (bible key, description) of the scene's characters.
    pub characters: Vec<(&'a str, &'a str)>,
    /// Description of the scene's place.
    pub place: Option<&'a str>,
    /// Only passed when the project is Cambodia-related.
    pub culture_notes: Option<&'a str>,
    pub square: bool,
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Byte index of `needle` in `hay` (ASCII case-insensitive) at word boundaries.
fn find_word(hay: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    let lower = hay.to_lowercase();
    let needle = needle.to_lowercase();
    // Only safe when lowercasing kept byte offsets (true for the ASCII prompts we build).
    if lower.len() != hay.len() {
        return None;
    }
    let mut from = 0;
    while let Some(rel) = lower.get(from..).and_then(|s| s.find(&needle)) {
        let start = from + rel;
        let end = start + needle.len();
        let before_ok = lower[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !is_word_char(c));
        let after_ok = lower[end..].chars().next().is_none_or(|c| !is_word_char(c));
        if before_ok && after_ok {
            return Some(end);
        }
        from = start + needle.len().max(1);
    }
    None
}

/// Inlines each character's bible description after its first mention in the subject
/// ("the farmer plants rice" → "the farmer (elderly Khmer rice farmer, …) plants rice").
/// Characters not mentioned are appended ("…, featuring elderly Khmer rice farmer, …").
pub fn inline_characters(subject: &str, characters: &[(&str, &str)]) -> String {
    let mut out = subject.trim().trim_end_matches(['.', ',']).to_owned();
    let mut missing = Vec::new();
    for (key, desc) in characters {
        let desc = desc.trim();
        if desc.is_empty() {
            continue;
        }
        let spaced = key.replace('_', " ");
        match find_word(&out, &spaced).or_else(|| find_word(&out, key)) {
            Some(end) => out.insert_str(end, &format!(" ({desc})")),
            None => missing.push(desc),
        }
    }
    if !missing.is_empty() {
        out.push_str(", featuring ");
        out.push_str(&missing.join(" and "));
    }
    out
}

fn join(parts: &[&str]) -> String {
    parts
        .iter()
        .map(|p| p.trim().trim_end_matches(['.', ',']).trim())
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

fn char_len(s: &str) -> usize {
    s.chars().count()
}

/// Builds the final prompt, at most [`MAX_PROMPT_CHARS`] characters: if too long, drop
/// culture notes first, then the place, then shorten the subject.
pub fn build_prompt(p: &PromptParts) -> String {
    let subject = inline_characters(p.subject, &p.characters);
    let composition = if p.square { SQUARE_COMPOSITION } else { "" };
    let culture = p.culture_notes.unwrap_or("");
    let place = p.place.unwrap_or("");
    let shot = shot_phrase(p.shot);

    for (with_culture, with_place) in [(true, true), (false, true), (false, false)] {
        let full = join(&[
            p.style,
            shot,
            &subject,
            if with_place { place } else { "" },
            if with_culture { culture } else { "" },
            composition,
            NO_LETTERING,
        ]);
        if char_len(&full) <= MAX_PROMPT_CHARS {
            return full;
        }
    }
    // Still too long: keep style/shot/composition/no-lettering, cut the subject.
    let fixed = join(&[p.style, shot, composition, NO_LETTERING]);
    let room = MAX_PROMPT_CHARS.saturating_sub(char_len(&fixed) + 2);
    let cut: String = subject.chars().take(room).collect();
    let out = join(&[p.style, shot, &cut, composition, NO_LETTERING]);
    out.chars().take(MAX_PROMPT_CHARS).collect()
}

/// Cambodia-related projects get the bible's culture notes (07 §3).
pub fn is_cambodia_related(khmer_project: bool, texts: &[&str]) -> bool {
    khmer_project
        || texts.iter().any(|t| {
            let t = t.to_lowercase();
            [
                "cambod",
                "khmer",
                "angkor",
                "phnom penh",
                "mekong",
                "tonle sap",
            ]
            .iter()
            .any(|k| t.contains(k))
        })
}

/// Scene number from its id (`s012` → 12). Stable when scenes are reordered.
pub fn scene_number(scene_id: &str) -> u32 {
    scene_id.trim_start_matches('s').parse().unwrap_or(0)
}

/// `visual_bible.seed + scene number`, unless the scene has a seed from "Regenerate".
pub fn scene_seed(bible_seed: u32, scene_id: &str, override_seed: Option<u32>) -> u32 {
    override_seed.unwrap_or_else(|| bible_seed.wrapping_add(scene_number(scene_id)))
}

/// `sha256(provider_id + model + final_prompt + seed + steps)` (07 §8).
pub fn cache_hash(provider_id: &str, model: &str, prompt: &str, seed: u32, steps: u8) -> String {
    let mut h = Sha256::new();
    for part in [
        provider_id,
        model,
        prompt,
        &seed.to_string(),
        &steps.to_string(),
    ] {
        h.update(part.as_bytes());
        h.update([0x1F]);
    }
    hex::encode(&h.finalize()[..16])
}

#[cfg(test)]
mod tests {
    use super::*;

    const FARMER: (&str, &str) = (
        "farmer",
        "elderly Khmer rice farmer, white checked krama scarf on head, faded blue shirt",
    );

    fn parts<'a>(subject: &'a str, chars: Vec<(&'a str, &'a str)>) -> PromptParts<'a> {
        PromptParts {
            style: "warm watercolor storybook illustration",
            shot: Shot::Wide,
            subject,
            characters: chars,
            place: Some("Cambodian village of wooden stilt houses"),
            culture_notes: Some("Khmer sampot clothing, Angkorian carvings"),
            square: true,
        }
    }

    #[test]
    fn inlines_bible_descriptions() {
        let s = inline_characters("The farmer plants rice at dawn.", &[FARMER]);
        assert_eq!(
            s,
            "The farmer (elderly Khmer rice farmer, white checked krama scarf on head, faded blue shirt) plants rice at dawn"
        );
        // Word boundary: "farmers" contains "farmer" but must still match only whole words.
        let s = inline_characters("Two farmers talk", &[FARMER]);
        assert!(s.ends_with(", featuring elderly Khmer rice farmer, white checked krama scarf on head, faded blue shirt"));
        // Keys with underscores match their spaced form.
        let s = inline_characters("the old monk prays", &[("old_monk", "saffron robe")]);
        assert_eq!(s, "the old monk (saffron robe) prays");
    }

    #[test]
    fn full_prompt_order_and_phrases() {
        let p = build_prompt(&parts("The farmer plants rice", vec![FARMER]));
        assert!(p.starts_with(
            "warm watercolor storybook illustration, wide establishing shot, The farmer ("
        ));
        assert!(p.contains("Cambodian village of wooden stilt houses, Khmer sampot clothing"));
        assert!(p.contains(SQUARE_COMPOSITION));
        assert!(p.ends_with(NO_LETTERING));
        let mut wide = parts("x", vec![]);
        wide.square = false;
        assert!(!build_prompt(&wide).contains("center band"));
    }

    #[test]
    fn shot_phrases() {
        assert_eq!(shot_phrase(Shot::Wide), "wide establishing shot");
        assert_eq!(shot_phrase(Shot::Medium), "medium shot");
        assert_eq!(shot_phrase(Shot::Close), "close-up portrait");
        assert_eq!(shot_phrase(Shot::Detail), "close-up detail shot");
        assert_eq!(shot_phrase(Shot::Aerial), "aerial view");
    }

    #[test]
    fn truncation_drops_culture_then_place_then_cuts_subject() {
        let long_culture = "c".repeat(1900);
        let long_place = "p".repeat(700);
        let mut p = parts("The farmer plants rice", vec![FARMER]);
        p.culture_notes = Some(&long_culture);
        let out = build_prompt(&p);
        assert!(char_len(&out) <= MAX_PROMPT_CHARS);
        assert!(!out.contains("ccc"), "culture notes dropped first");
        assert!(out.contains("Cambodian village"), "place kept");

        p.place = Some(&long_place);
        p.subject = "The farmer plants rice";
        let long_subject = "s".repeat(1300);
        p.subject = &long_subject;
        let out = build_prompt(&p);
        assert!(char_len(&out) <= MAX_PROMPT_CHARS);
        assert!(!out.contains("ppp"), "then the place");
        assert!(out.contains("sss"), "subject kept");

        let huge = "word ".repeat(1000);
        p.subject = &huge;
        let out = build_prompt(&p);
        assert_eq!(char_len(&out), char_len(&out).min(MAX_PROMPT_CHARS));
        assert!(out.ends_with(NO_LETTERING), "suffix survives truncation");
    }

    #[test]
    fn seeds_and_hash() {
        assert_eq!(scene_seed(1847, "s001", None), 1848);
        assert_eq!(scene_seed(1847, "s012", None), 1859);
        assert_eq!(scene_seed(1847, "s012", Some(5)), 5);
        let a = cache_hash("cloudflare_flux", "m", "p", 1, 4);
        assert_eq!(a, cache_hash("cloudflare_flux", "m", "p", 1, 4));
        assert_ne!(a, cache_hash("cloudflare_flux", "m", "p", 2, 4));
        assert_ne!(a, cache_hash("gemini_nano_banana", "m", "p", 1, 4));
    }

    #[test]
    fn cambodia_detection() {
        assert!(is_cambodia_related(true, &[]));
        assert!(is_cambodia_related(false, &["How was Angkor Wat built?"]));
        assert!(!is_cambodia_related(false, &["Why is the sky blue?"]));
    }
}
