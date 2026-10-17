//! Is the text `lopdf` extracted from a chunk good enough to send instead of the pages
//! (`docs/10-document-source.md` §3)? Scanned pages have no text; many PDFs (Khmer from
//! browsers and Word especially) give text with lost subscripts or vowels in visual order.

/// A page with fewer visible characters counts as "no text" (scan, figure, blank page).
const MIN_PAGE_CHARS: usize = 80;
/// At least this share of the pages must have text.
const MIN_TEXT_PAGES: f64 = 0.7;
/// NUL, U+FFFD, control and private-use characters.
const MAX_BAD_RATIO: f64 = 0.005;
/// Khmer signs in impossible positions (vowel after a space, coeng without a consonant).
const MAX_KHMER_INVALID_RATIO: f64 = 0.01;
const MIN_KHMER_CHARS: usize = 20;
/// Letters among visible characters (formula-heavy pages are still mostly letters).
const MIN_LETTER_RATIO: f64 = 0.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Good,
    TooLittleText,
    BadCharacters,
    KhmerOutOfOrder,
    MostlySymbols,
}

impl Verdict {
    /// For the project log.
    pub fn reason(self) -> &'static str {
        match self {
            Self::Good => "text is clean",
            Self::TooLittleText => "too little text (scanned or image pages)",
            Self::BadCharacters => "unreadable characters in the text",
            Self::KhmerOutOfOrder => "Khmer text out of order (lost subscripts / vowels)",
            Self::MostlySymbols => "mostly symbols, few letters",
        }
    }
}

fn is_bad(c: char) -> bool {
    c == '\u{FFFD}'
        || ('\u{E000}'..='\u{F8FF}').contains(&c)
        || (c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
}

fn is_khmer(c: char) -> bool {
    ('\u{1780}'..='\u{17FF}').contains(&c)
}

fn is_khmer_base(c: char) -> bool {
    ('\u{1780}'..='\u{17B3}').contains(&c)
}

/// Dependent vowels and signs that must follow a base (or another sign) in logical order.
fn is_khmer_dependent(c: char) -> bool {
    ('\u{17B6}'..='\u{17D1}').contains(&c) || c == '\u{17D3}' || c == '\u{17DD}'
}

const COENG: char = '\u{17D2}';

/// Khmer characters in positions valid Unicode text never has.
fn khmer_invalid(text: &str) -> (usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    let mut khmer = 0;
    let mut invalid = 0;
    for (i, &c) in chars.iter().enumerate() {
        if !is_khmer(c) {
            continue;
        }
        khmer += 1;
        let prev = i.checked_sub(1).map(|j| chars[j]);
        if is_khmer_dependent(c) && !prev.is_some_and(|p| is_khmer_base(p) || is_khmer_dependent(p) || p == COENG) {
            invalid += 1;
        }
        if c == COENG && !chars.get(i + 1).is_some_and(|&n| is_khmer_base(n)) {
            invalid += 1;
        }
    }
    (khmer, invalid)
}

/// Checks the text of every page of one chunk.
pub fn check(pages: &[String]) -> Verdict {
    if pages.is_empty() {
        return Verdict::TooLittleText;
    }
    let visible = |p: &String| p.chars().filter(|c| !c.is_whitespace()).count();
    let text_pages = pages.iter().filter(|p| visible(p) >= MIN_PAGE_CHARS).count();
    if (text_pages as f64) < MIN_TEXT_PAGES * pages.len() as f64 {
        return Verdict::TooLittleText;
    }
    let all: String = pages.concat();
    let total = all.chars().filter(|c| !c.is_whitespace()).count().max(1);
    let bad = all.chars().filter(|&c| is_bad(c)).count();
    if bad as f64 > MAX_BAD_RATIO * total as f64 {
        return Verdict::BadCharacters;
    }
    let (khmer, invalid) = khmer_invalid(&all);
    if khmer >= MIN_KHMER_CHARS && invalid as f64 > MAX_KHMER_INVALID_RATIO * khmer as f64 {
        return Verdict::KhmerOutOfOrder;
    }
    let letters = all.chars().filter(|c| c.is_alphabetic() || is_khmer(*c)).count();
    if (letters as f64) < MIN_LETTER_RATIO * total as f64 {
        return Verdict::MostlySymbols;
    }
    Verdict::Good
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pages(text: &str, n: usize) -> Vec<String> {
        vec![text.to_owned(); n]
    }

    const KHMER_OK: &str = "ស្រូវគឺជាដំណាំសំខាន់បំផុតរបស់ប្រទេសកម្ពុជា។ ប្រជាកសិករប្រហែល ៧៥ ភាគរយ \
                            រស់នៅដោយពឹងផ្អែកលើការដាំស្រូវ។ ក្នុងឆ្នាំ ២០២៣ ផលិតកម្មស្រូវសរុបមានប្រមាណ ១២ លានតោន។";
    /// What lopdf returned for a Chromium-made Khmer PDF (2026-10-17): lost subscripts as NUL,
    /// pre-base vowels in visual order.
    const KHMER_GARBLED: &str = "\0ស\0វគឺជាដំ\0ំសំខាន់បំផុតរបស់\0បេទសកម\0\0ជា។ \0បជាកសិករ\0បែហល \
                                 ៧៥ ភាគរយ រស់េ\0េ\0យពឹងែផ\0កេល\0ការ\0ំ\0ស\0វ។ ១. េសចក\0ីេផ\0\0ម";

    #[test]
    fn clean_english_and_khmer_pass() {
        let en = "Recurrent neural networks, long short-term memory and gated recurrent neural networks \
                  have been firmly established as state of the art approaches in sequence modeling.";
        assert_eq!(check(&pages(en, 3)), Verdict::Good);
        assert_eq!(check(&pages(KHMER_OK, 3)), Verdict::Good);
    }

    #[test]
    fn scans_and_garbled_khmer_fail() {
        assert_eq!(check(&pages("", 5)), Verdict::TooLittleText);
        assert_eq!(check(&[KHMER_OK.to_owned(), String::new(), String::new()]), Verdict::TooLittleText);
        assert_eq!(check(&pages(KHMER_GARBLED, 2)), Verdict::BadCharacters);
        // Without the NULs the vowel order still gives it away.
        let reordered = KHMER_GARBLED.replace('\0', "");
        assert_eq!(check(&pages(&reordered.repeat(2), 2)), Verdict::KhmerOutOfOrder);
    }

    #[test]
    fn symbol_soup_fails() {
        let soup = "∑∫≈≠ ∂∆ 12345 ()[]{} +-*/ = < > ≤ ≥ ±".repeat(5);
        assert_eq!(check(&pages(&soup, 2)), Verdict::MostlySymbols);
    }
}
