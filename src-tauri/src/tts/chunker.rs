//! Splits a scene's spoken text into sentence-sized requests for VoxCPM2
//! (`docs/09-voice-profiles.md`): long single requests are unstable and, on a 6 GB GPU with
//! Vulkan, run out of memory in the audio decoder (measured: a 24 s Khmer paragraph fails,
//! sentences of ~7 s work). Over-long sentences are split at spaces, then by grapheme count.

use unicode_segmentation::UnicodeSegmentation;

use crate::pipeline::project::Language;

/// Upper bound per request: ~10 s of Khmer (≈ 11 graphemes/s) or ~14 s of English.
pub fn max_units(language: Language) -> usize {
    match language {
        Language::Km => 110,
        Language::En => 200,
    }
}

fn units(text: &str) -> usize {
    text.graphemes(true).filter(|g| !g.trim().is_empty()).count()
}

fn is_end(g: &str) -> bool {
    matches!(g, "។" | "៕" | "?" | "!" | "." | "？" | "！" | "\n")
}

/// Sentences, keeping their end punctuation.
fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for g in text.graphemes(true) {
        cur.push_str(g);
        if is_end(g) {
            let s = cur.trim().to_owned();
            if !s.is_empty() {
                out.push(s);
            }
            cur.clear();
        }
    }
    let s = cur.trim().to_owned();
    if !s.is_empty() {
        out.push(s);
    }
    out
}

/// Splits `text` into pieces of at most `max` units: at spaces where possible, else hard.
fn split_long(text: &str, max: usize) -> Vec<String> {
    if units(text) <= max {
        return vec![text.to_owned()];
    }
    let words: Vec<&str> = text.split(' ').filter(|w| !w.is_empty()).collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    for w in words {
        if units(w) > max {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            // No space to split at (Khmer runs words together): cut by graphemes.
            let gs: Vec<&str> = w.graphemes(true).collect();
            for piece in gs.chunks(max) {
                out.push(piece.concat());
            }
            continue;
        }
        let candidate = if cur.is_empty() { w.to_owned() } else { format!("{cur} {w}") };
        if units(&candidate) > max {
            out.push(std::mem::replace(&mut cur, w.to_owned()));
        } else {
            cur = candidate;
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Request-sized pieces of a scene's spoken text, in order. Very short sentences are merged
/// with the next one (single words sound clipped on their own).
pub fn chunk(text: &str, language: Language) -> Vec<String> {
    let max = max_units(language);
    let pieces: Vec<String> = sentences(text).into_iter().flat_map(|s| split_long(&s, max)).collect();
    let mut out: Vec<String> = Vec::new();
    for p in pieces {
        match out.last_mut() {
            Some(prev) if units(prev) < 12 && units(prev) + units(&p) <= max => {
                prev.push(' ');
                prev.push_str(&p);
            }
            _ => out.push(p),
        }
    }
    out
}

/// Two halves for an out-of-memory retry: at the space nearest the middle, else at the
/// middle grapheme. `None` when the text is too short to split.
pub fn halves(text: &str) -> Option<(String, String)> {
    let gs: Vec<&str> = text.graphemes(true).collect();
    if gs.len() < 8 {
        return None;
    }
    let mid = gs.len() / 2;
    let at = (0..gs.len())
        .filter(|i| gs[*i] == " ")
        .min_by_key(|i| i.abs_diff(mid))
        .filter(|i| i.abs_diff(mid) < gs.len() / 3)
        .unwrap_or(mid);
    let (a, b) = gs.split_at(at);
    let (a, b) = (a.concat().trim().to_owned(), b.concat().trim().to_owned());
    (!a.is_empty() && !b.is_empty()).then_some((a, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    const KM: &str = "តើអ្នកដឹងទេថា ដើម្បីសាងសង់ប្រាសាទអង្គរវត្តដ៏អស្ចារ្យនេះ បុព្វបុរសខ្មែរយើងត្រូវការពេលតិចជាងសាមសិបឆ្នាំទៅទៀត? ដើម្បីបានថ្មភក់រាប់លានដុំមកសាងសង់ ដំណើរការដ៏ធំសម្បើមមួយត្រូវចាប់ផ្តើមឡើងនៅឯភ្នំគូលែន។";

    #[test]
    fn khmer_paragraph_becomes_sentences_within_the_cap() {
        let c = chunk(KM, Language::Km);
        assert_eq!(c.len(), 2, "{c:?}");
        assert!(c[0].ends_with('?') && c[1].ends_with('។'));
        assert!(c.iter().all(|p| units(p) <= max_units(Language::Km)));
        assert_eq!(c.concat().replace(' ', ""), KM.replace(' ', ""));
    }

    #[test]
    fn over_long_sentences_split_at_spaces_then_graphemes() {
        let long = "word ".repeat(80) + "end.";
        let c = chunk(&long, Language::En);
        assert!(c.len() >= 2 && c.iter().all(|p| units(p) <= 200), "{c:?}");
        let run = "ស្រូវ".repeat(60); // no spaces
        let c = chunk(&run, Language::Km);
        assert!(c.len() >= 2 && c.iter().all(|p| units(p) <= 110));
        assert!(c.iter().all(|p| !p.starts_with('្')), "never split inside a cluster");
    }

    #[test]
    fn tiny_sentences_are_merged_forward() {
        let c = chunk("Yes. Then the river rose and the whole village moved to higher ground.", Language::En);
        assert_eq!(c.len(), 1, "{c:?}");
    }

    #[test]
    fn halves_split_near_the_middle_at_a_space() {
        let (a, b) = halves("one two three four five six").unwrap();
        assert_eq!((a.as_str(), b.as_str()), ("one two three", "four five six"));
        let (a, b) = halves(&"ក".repeat(20)).unwrap();
        assert_eq!((a.chars().count(), b.chars().count()), (10, 10));
        assert!(halves("short").is_none());
    }
}
