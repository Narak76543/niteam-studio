//! Khmer number-to-words fallback for TTS input (`docs/04-khmer.md` §4).
//! The script model should already write numbers as words in `spoken_text`; this converts any
//! digits it left behind (Arabic `0-9` or Khmer `០-៩`), so the voice never reads digit names.
//! **A native speaker must verify the output before release.**

const ONES: [&str; 10] = [
    "សូន្យ",
    "មួយ",
    "ពីរ",
    "បី",
    "បួន",
    "ប្រាំ",
    "ប្រាំមួយ",
    "ប្រាំពីរ",
    "ប្រាំបី",
    "ប្រាំបួន",
];
const TENS: [&str; 10] = [
    "",
    "ដប់",
    "ម្ភៃ",
    "សាមសិប",
    "សែសិប",
    "ហាសិប",
    "ហុកសិប",
    "ចិតសិប",
    "ប៉ែតសិប",
    "កៅសិប",
];
const HUNDRED: &str = "រយ";
const THOUSAND: &str = "ពាន់";
const TEN_THOUSAND: &str = "ម៉ឺន";
const HUNDRED_THOUSAND: &str = "សែន";
const MILLION: &str = "លាន";
const POINT: &str = "ចុច";
const PERCENT: &str = "ភាគរយ";

fn digit(i: u64) -> &'static str {
    ONES.get(usize::try_from(i).unwrap_or(0))
        .copied()
        .unwrap_or("")
}

/// 0..=99 without the zero word.
fn below_hundred(n: u64, out: &mut String) {
    let (t, u) = (n / 10, n % 10);
    if t > 0 {
        out.push_str(
            TENS.get(usize::try_from(t).unwrap_or(0))
                .copied()
                .unwrap_or(""),
        );
    }
    if u > 0 {
        out.push_str(digit(u));
    }
}

/// 1..=999_999 using the Khmer units សែន / ម៉ឺន / ពាន់ / រយ.
fn below_million(n: u64, out: &mut String) {
    for (value, unit) in [
        (100_000, HUNDRED_THOUSAND),
        (10_000, TEN_THOUSAND),
        (1_000, THOUSAND),
        (100, HUNDRED),
    ] {
        let d = (n / value) % 10;
        if d > 0 {
            out.push_str(digit(d));
            out.push_str(unit);
        }
    }
    below_hundred(n % 100, out);
}

/// Spoken Khmer words for `n`, e.g. 1863 → មួយពាន់ប្រាំបីរយហុកសិបបី.
pub fn to_words(n: u64) -> String {
    if n == 0 {
        return ONES[0].to_owned();
    }
    let mut out = String::new();
    let millions = n / 1_000_000;
    if millions > 0 {
        out.push_str(&to_words(millions));
        out.push_str(MILLION);
    }
    below_million(n % 1_000_000, &mut out);
    out
}

fn ascii_digit(c: char) -> Option<u32> {
    match c {
        '0'..='9' => c.to_digit(10),
        // Khmer digits ០ (U+17E0) … ៩ (U+17E9)
        '\u{17E0}'..='\u{17E9}' => Some(c as u32 - 0x17E0),
        _ => None,
    }
}

/// Reads a run of digits as an integer; long runs (> 15 digits) are read digit by digit.
fn digits_to_words(digits: &[u32]) -> String {
    if digits.len() > 15 {
        return digits.iter().map(|d| digit(u64::from(*d))).collect();
    }
    let n = digits.iter().fold(0u64, |acc, d| acc * 10 + u64::from(*d));
    to_words(n)
}

/// Replaces every number in `text` with Khmer words.
/// Handles thousands separators (`1,500`), decimals (`3.5` → បីចុចប្រាំ) and `%`.
pub fn replace_digits(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let Some(first) = chars.get(i).copied().and_then(ascii_digit) else {
            if let Some(c) = chars.get(i) {
                out.push(*c);
            }
            i += 1;
            continue;
        };
        // Integer part, allowing "," between groups of exactly three digits.
        let mut int = vec![first];
        i += 1;
        loop {
            match chars.get(i).copied() {
                Some(c) if ascii_digit(c).is_some() => {
                    int.extend(ascii_digit(c));
                    i += 1;
                }
                Some(',')
                    if (1..=3)
                        .all(|k| chars.get(i + k).copied().and_then(ascii_digit).is_some())
                        && chars.get(i + 4).copied().and_then(ascii_digit).is_none() =>
                {
                    i += 1;
                }
                _ => break,
            }
        }
        out.push_str(&digits_to_words(&int));
        // Decimal part.
        if chars.get(i) == Some(&'.')
            && let Some(d) = chars.get(i + 1).copied().and_then(ascii_digit)
        {
            let mut frac = vec![d];
            i += 2;
            while let Some(d) = chars.get(i).copied().and_then(ascii_digit) {
                frac.push(d);
                i += 1;
            }
            out.push_str(POINT);
            out.push_str(&digits_to_words(&frac));
        }
        if chars.get(i) == Some(&'%') {
            out.push_str(PERCENT);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_numbers() {
        assert_eq!(to_words(0), "សូន្យ");
        assert_eq!(to_words(7), "ប្រាំពីរ");
        assert_eq!(to_words(10), "ដប់");
        assert_eq!(to_words(11), "ដប់មួយ");
        assert_eq!(to_words(20), "ម្ភៃ");
        assert_eq!(to_words(21), "ម្ភៃមួយ");
        assert_eq!(to_words(45), "សែសិបប្រាំ");
        assert_eq!(to_words(100), "មួយរយ");
        assert_eq!(to_words(105), "មួយរយប្រាំ");
    }

    #[test]
    fn large_units() {
        assert_eq!(to_words(1_000), "មួយពាន់");
        assert_eq!(to_words(1_500), "មួយពាន់ប្រាំរយ");
        assert_eq!(to_words(10_000), "មួយម៉ឺន");
        assert_eq!(to_words(25_000), "ពីរម៉ឺនប្រាំពាន់");
        assert_eq!(to_words(100_000), "មួយសែន");
        assert_eq!(to_words(150_000), "មួយសែនប្រាំម៉ឺន");
        assert_eq!(to_words(1_000_000), "មួយលាន");
        assert_eq!(to_words(2_500_000), "ពីរលានប្រាំសែន");
        assert_eq!(to_words(1_000_000_000), "មួយពាន់លាន");
    }

    #[test]
    fn years() {
        assert_eq!(to_words(1863), "មួយពាន់ប្រាំបីរយហុកសិបបី");
        assert_eq!(to_words(2026), "ពីរពាន់ម្ភៃប្រាំមួយ");
        assert_eq!(to_words(1113), "មួយពាន់មួយរយដប់បី");
    }

    #[test]
    fn replaces_digits_in_text() {
        assert_eq!(replace_digits("ឆ្នាំ1863"), "ឆ្នាំមួយពាន់ប្រាំបីរយហុកសិបបី");
        assert_eq!(replace_digits("ឆ្នាំ១៨៦៣"), "ឆ្នាំមួយពាន់ប្រាំបីរយហុកសិបបី");
        assert_eq!(replace_digits("1,500 នាក់"), "មួយពាន់ប្រាំរយ នាក់");
        assert_eq!(replace_digits("30%"), "សាមសិបភាគរយ");
        assert_eq!(replace_digits("3.5 គីឡូ"), "បីចុចប្រាំ គីឡូ");
        assert_eq!(replace_digits("គ្មានលេខ"), "គ្មានលេខ");
        // A comma not followed by exactly three digits is punctuation.
        assert_eq!(replace_digits("1, 2"), "មួយ, ពីរ");
        assert_eq!(replace_digits("end."), "end.");
    }
}
