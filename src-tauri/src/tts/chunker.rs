//! Splits a scene's spoken text into sentence-sized requests for VoxCPM2
//! (`docs/09-voice-profiles.md`): long single requests are unstable and, on a 6 GB GPU with
//! Vulkan, run out of memory in the audio decoder (measured: a 24 s Khmer paragraph fails,
//! sentences of ~7 s work). Over-long sentences are split as evenly as possible at clause
//! punctuation or phrase spaces, else right before a Khmer connector word or syllable onset —
//! never inside a stacked cluster.

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

/// Where a piece may end, worst to best. Khmer has no spaces between words, so after clause
/// punctuation and phrase spaces the next best places are right before a connector word
/// (`ដែល`, `ដើម្បី`, …) and then before a syllable onset; a bare cluster cut is the last resort.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Break {
    Cluster,
    Syllable,
    Connector,
    Space,
    Clause,
}

/// Khmer sign that stacks the next consonant below the previous one (`ក្ស`).
const COENG: char = '\u{17D2}';

/// Mid-sentence punctuation; a piece may end right after it.
fn is_clause(g: &str) -> bool {
    matches!(g, "," | ";" | ":" | "៖" | "，" | "、" | "،")
}

/// Khmer words that usually open a new clause ("which", "and", "but", "in order to",
/// "because", "and then", "so", "as", "when", "although", "while", "if", "after that").
const KM_CONNECTORS: &[&str] = &[
    "ដែល",
    "និង",
    "ប៉ុន្តែ",
    "ដើម្បី",
    "ពីព្រោះ",
    "ព្រោះ",
    "ហើយ",
    "រួចហើយ",
    "ដូច្នេះ",
    "ដូចដែល",
    "នៅពេល",
    "ទោះបី",
    "ខណៈ",
    "ប្រសិនបើ",
    "បន្ទាប់មក",
];

/// Endings that make a connector the tail of a longer word (`ពី|ព្រោះ`, `រួច|ហើយ`, `ដូច|ដែល`).
const NOT_AFTER: &[&str] = &["ពី", "រួច", "ដូច", "ទោះ"];

/// Khmer dependent vowel signs: a cluster carrying one starts a syllable.
fn has_vowel_sign(g: &str) -> bool {
    g.chars().any(|c| matches!(c, '\u{17B6}'..='\u{17C5}'))
}

/// How good a cut right before `gs[i]` is (`None` = never cut here).
fn break_at(gs: &[&str], offsets: &[usize], text: &str, i: usize) -> Option<Break> {
    let (prev, cur) = (gs[i - 1], gs[i]);
    if prev.ends_with(COENG) || cur == "ៗ" || is_clause(cur) || is_end(cur) {
        return None; // inside a stacked cluster, or before a mark that belongs to the left
    }
    if is_clause(prev) {
        return Some(Break::Clause);
    }
    if cur.trim().is_empty() || prev.trim().is_empty() {
        return Some(Break::Space);
    }
    let (before, rest) = text.split_at(offsets[i]);
    if KM_CONNECTORS.iter().any(|c| rest.starts_with(c)) && !NOT_AFTER.iter().any(|n| before.ends_with(n)) {
        return Some(Break::Connector);
    }
    if cur.ends_with(COENG) || has_vowel_sign(cur) {
        return Some(Break::Syllable);
    }
    Some(Break::Cluster)
}

/// Grapheme index to cut `gs` at: the best kind of break whose left side has `lo..=hi` units,
/// nearest `target` among equals. Falls back to any break up to `hi`, then to a hard cut.
fn cut_index(gs: &[&str], lo: usize, hi: usize, target: usize) -> usize {
    let text = gs.concat();
    let mut offsets = Vec::with_capacity(gs.len());
    let mut prefix = Vec::with_capacity(gs.len() + 1);
    let (mut off, mut n) = (0, 0);
    prefix.push(0);
    for g in gs {
        offsets.push(off);
        off += g.len();
        n += usize::from(!g.trim().is_empty());
        prefix.push(n);
    }
    let total = n;
    let candidates: Vec<(usize, Break)> = (1..gs.len())
        .filter(|i| prefix[*i] > 0 && prefix[*i] < total && prefix[*i] <= hi)
        .filter_map(|i| break_at(gs, &offsets, &text, i).map(|b| (i, b)))
        .collect();
    let best = |min: usize| {
        candidates
            .iter()
            .filter(|(i, _)| prefix[*i] >= min)
            .max_by_key(|(i, b)| (*b, std::cmp::Reverse(prefix[*i].abs_diff(target))))
            .map(|(i, _)| *i)
    };
    best(lo.max(1))
        .or_else(|| best(1))
        .unwrap_or_else(|| (1..gs.len()).find(|i| prefix[*i] >= hi).unwrap_or(gs.len() / 2).max(1))
}

/// Splits `text` into pieces of at most `max` units, as evenly as the break points allow.
fn split_long(text: &str, max: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text.trim().to_owned();
    while units(&rest) > max {
        let gs: Vec<&str> = rest.graphemes(true).collect();
        let total = units(&rest);
        let target = total.div_ceil(total.div_ceil(max));
        let at = cut_index(&gs, target / 2, max, target);
        let (a, b) = gs.split_at(at);
        let (a, b) = (a.concat().trim().to_owned(), b.concat().trim().to_owned());
        if !a.is_empty() {
            out.push(a);
        }
        rest = b;
    }
    if !rest.is_empty() {
        out.push(rest);
    }
    out
}

/// One request's worth of text. `ends_sentence` is false for a piece cut from the middle of a
/// long sentence (the voice engine then keeps a continuing, non-final tone).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub text: String,
    pub ends_sentence: bool,
}

/// Request-sized pieces of a scene's spoken text, in order. Very short sentences are merged
/// with the next one (single words sound clipped on their own).
pub fn chunk(text: &str, language: Language) -> Vec<Chunk> {
    let max = max_units(language);
    let pieces = sentences(text).into_iter().flat_map(|s| {
        let parts = split_long(&s, max);
        let last = parts.len().saturating_sub(1);
        parts.into_iter().enumerate().map(move |(i, text)| Chunk { text, ends_sentence: i == last })
    });
    let mut out: Vec<Chunk> = Vec::new();
    for p in pieces {
        match out.last_mut() {
            Some(prev) if units(&prev.text) < 12 && units(&prev.text) + units(&p.text) <= max => {
                prev.text.push(' ');
                prev.text.push_str(&p.text);
                prev.ends_sentence = p.ends_sentence;
            }
            _ => out.push(p),
        }
    }
    out
}

/// Two halves for an out-of-memory retry, cut at the best break near the middle (phrase
/// space first, never inside a stacked Khmer cluster). `None` when the text is too short.
pub fn halves(text: &str) -> Option<(String, String)> {
    let gs: Vec<&str> = text.graphemes(true).collect();
    if gs.len() < 8 {
        return None;
    }
    let total = units(text);
    let quarter = (total / 4).max(1);
    let at = cut_index(&gs, quarter, total.saturating_sub(quarter), total / 2);
    let (a, b) = gs.split_at(at);
    let (a, b) = (a.concat().trim().to_owned(), b.concat().trim().to_owned());
    (!a.is_empty() && !b.is_empty()).then_some((a, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    const KM: &str = "តើអ្នកដឹងទេថា ដើម្បីសាងសង់ប្រាសាទអង្គរវត្តដ៏អស្ចារ្យនេះ បុព្វបុរសខ្មែរយើងត្រូវការពេលតិចជាងសាមសិបឆ្នាំទៅទៀត? ដើម្បីបានថ្មភក់រាប់លានដុំមកសាងសង់ ដំណើរការដ៏ធំសម្បើមមួយត្រូវចាប់ផ្តើមឡើងនៅឯភ្នំគូលែន។";

    fn texts(c: &[Chunk]) -> Vec<&str> {
        c.iter().map(|p| p.text.as_str()).collect()
    }

    #[test]
    fn khmer_paragraph_becomes_sentences_within_the_cap() {
        let c = chunk(KM, Language::Km);
        assert_eq!(c.len(), 2, "{c:?}");
        assert!(c[0].text.ends_with('?') && c[1].text.ends_with('។'));
        assert!(c.iter().all(|p| p.ends_sentence));
        assert!(c.iter().all(|p| units(&p.text) <= max_units(Language::Km)));
        assert_eq!(texts(&c).concat().replace(' ', ""), KM.replace(' ', ""));
    }

    #[test]
    fn over_long_sentences_split_at_spaces_then_graphemes() {
        let long = "word ".repeat(80) + "end.";
        let c = chunk(&long, Language::En);
        assert!(c.len() >= 2 && c.iter().all(|p| units(&p.text) <= 200), "{c:?}");
        assert!(!c[0].ends_sentence && c[c.len() - 1].ends_sentence);
        assert!(c.iter().all(|p| p.text.split(' ').all(|w| w == "word" || w == "end.")), "never inside a word");
        let run = "ស្រូវ".repeat(60); // no spaces
        let c = chunk(&run, Language::Km);
        assert!(c.len() >= 2 && c.iter().all(|p| units(&p.text) <= 110));
        assert!(c.iter().all(|p| !p.text.starts_with('្')), "never split inside a cluster");
        assert!(c.iter().all(|p| p.text.replace("ស្រូវ", "").is_empty()), "cut at syllable onsets: {c:?}");
    }

    #[test]
    fn long_khmer_sentences_split_evenly_at_phrase_spaces() {
        let phrase = "បុព្វបុរសខ្មែរយើងត្រូវការពេលតិចជាងសាមសិបឆ្នាំ";
        let sentence = [phrase; 5].join(" ") + "។";
        let c = chunk(&sentence, Language::Km);
        assert_eq!(c.len(), 2, "{c:?}");
        assert!(c.iter().all(|p| p.text.trim_end_matches('។').split(' ').all(|w| w == phrase)), "{c:?}");
        let (a, b) = (units(&c[0].text), units(&c[1].text));
        assert!(a.abs_diff(b) <= 40, "balanced: {a} / {b}");
        assert_eq!((c[0].ends_sentence, c[1].ends_sentence), (false, true));
    }

    #[test]
    fn spaceless_khmer_splits_before_a_connector_word() {
        let left = "ប្រាសាទអង្គរវត្តត្រូវបានសាងសង់ឡើងនៅសតវត្សទីដប់ពីរដោយព្រះបាទសូរ្យវរ្ម័នទីពីរ";
        let right = "ដើម្បីឧទ្ទិសថ្វាយដល់ព្រះវិស្ណុនិងជាទីបញ្ចុះសពរបស់ព្រះអង្គផ្ទាល់ក្នុងរាជធានីយសោធរបុរៈដែលមានទំហំធំបំផុតនៅលើពិភពលោកក្នុងតំបន់អាស៊ីអាគ្នេយ៍ទាំងមូលនាសម័យកាលនោះ";
        let text = format!("{left}{right}។");
        assert!(units(&text) > max_units(Language::Km));
        let c = chunk(&text, Language::Km);
        assert_eq!(c.len(), 2, "{c:?}");
        assert!(c[1].text.starts_with("ដើម្បី") || c[1].text.starts_with("និង"), "{c:?}");
    }

    #[test]
    fn connectors_inside_longer_words_are_not_split() {
        let gs: Vec<&str> = "ពីព្រោះ".graphemes(true).collect();
        let text = gs.concat();
        let offsets: Vec<usize> = gs.iter().scan(0, |o, g| {
            let at = *o;
            *o += g.len();
            Some(at)
        }).collect();
        let at = gs.iter().position(|g| g.starts_with('ព') && g.contains('្')).unwrap();
        assert_ne!(break_at(&gs, &offsets, &text, at), Some(Break::Connector));
    }

    #[test]
    fn tiny_sentences_are_merged_forward() {
        let c = chunk("Yes. Then the river rose and the whole village moved to higher ground.", Language::En);
        assert_eq!(c.len(), 1, "{c:?}");
        assert!(c[0].ends_sentence);
    }

    #[test]
    fn halves_split_near_the_middle_at_a_space() {
        let (a, b) = halves("one two three four five six").unwrap();
        assert_eq!((a.as_str(), b.as_str()), ("one two three", "four five six"));
        let (a, b) = halves(&"ក".repeat(20)).unwrap();
        assert_eq!((a.chars().count(), b.chars().count()), (10, 10));
        assert!(halves("short").is_none());
        let (a, b) = halves(&"ស្រូវ".repeat(8)).unwrap();
        assert!(a.replace("ស្រូវ", "").is_empty() && b.replace("ស្រូវ", "").is_empty(), "{a} | {b}");
    }
}
