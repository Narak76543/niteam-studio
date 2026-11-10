//! Khmer text → the punctuation VoxCPM2 understands (`docs/09-voice-profiles.md`,
//! `docs/04-khmer.md` §4b). VoxCPM2 takes its prosody cues from Latin punctuation: `,` gives a
//! breathing pause with a sustained / rising clause tone, `.` `?` `!` give the sentence-final
//! cadence. Khmer writes sentence ends as `។` / `៕` and phrase pauses as plain spaces, so the
//! model reads a Khmer sentence as one flat run. This rewrite is applied **only** to the text
//! sent to the audio server: `spoken_text`, subtitle timings and the UI keep the original.

/// Khmer letters, signs and digits (the punctuation `។ ៕ ៖` is rewritten before this is asked).
fn is_khmer(c: char) -> bool {
    matches!(c, '\u{1780}'..='\u{17FF}' | '\u{19E0}'..='\u{19FF}')
}

/// Punctuation that already carries a pause: no space before it, never a comma next to it.
fn is_punct(c: char) -> bool {
    matches!(c, '.' | ',' | '?' | '!' | ':' | ';' | '…')
}

/// Closing quotes / brackets that may follow a sentence's end mark.
fn is_closer(c: char) -> bool {
    matches!(c, '"' | '\'' | ')' | ']' | '»' | '”' | '’')
}

fn is_terminal(c: char) -> bool {
    matches!(c, '.' | '?' | '!' | '…')
}

/// Khmer / full-width punctuation → the Latin mark with the same role.
fn map_punct(c: char) -> Option<char> {
    match c {
        '។' | '៕' => Some('.'),
        '៖' => Some(','),
        '？' => Some('?'),
        '！' => Some('!'),
        '，' | '、' => Some(','),
        _ => None,
    }
}

/// The text VoxCPM2 should read for one Khmer piece.
///
/// - `។` / `៕` → `.`, `៖` → `,` (`?` and `!` stay);
/// - whitespace runs collapse to one space; no space before punctuation;
/// - a phrase space between two Khmer words → `, ` (a breathing pause with clause intonation);
///   spaces next to Latin words (names such as `React Native`, `Apollo 11`) stay plain spaces;
/// - a piece without end punctuation gets `.` when it ends a sentence, else `,` so a piece cut
///   from the middle of a long sentence does not fall to a sentence-final tone.
pub fn for_voxcpm(text: &str, ends_sentence: bool) -> String {
    let mut out = String::with_capacity(text.len() + text.len() / 4);
    let mut pending_space = false;
    // Set after a mapped Khmer sentence end: Khmer often writes the next sentence right after `។`.
    let mut space_after_mark = false;
    for raw in text.chars() {
        if raw.is_whitespace() {
            pending_space = !out.is_empty();
            continue;
        }
        let mapped = map_punct(raw);
        let c = mapped.unwrap_or(raw);
        if let Some(prev) = out.chars().next_back() {
            if is_punct(c) {
                // "ថា ។" → "ថា." — drop the space; "., " never doubles up.
                if prev == ',' && c != ',' {
                    out.pop();
                } else if prev == ',' && c == ',' {
                    pending_space = false;
                    continue;
                }
            } else if pending_space || (space_after_mark && !is_closer(c)) {
                if is_khmer(prev) && is_khmer(c) && pending_space {
                    out.push_str(", ");
                } else {
                    out.push(' ');
                }
            }
        }
        pending_space = false;
        space_after_mark = mapped.is_some_and(is_terminal);
        out.push(c);
    }
    if out.is_empty() || out.chars().rev().find(|c| !is_closer(*c)).is_some_and(is_terminal) {
        return out;
    }
    if out.ends_with([',', ':', ';']) {
        out.pop();
    }
    out.push(if ends_sentence { '.' } else { ',' });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn khmer_sentence_ends_become_periods() {
        assert_eq!(for_voxcpm("សួស្តី។", true), "សួស្តី.");
        assert_eq!(for_voxcpm("ចប់ហើយ៕", true), "ចប់ហើយ.");
        assert_eq!(for_voxcpm("តើអ្នកដឹងទេ?", true), "តើអ្នកដឹងទេ?");
        assert_eq!(for_voxcpm("អស្ចារ្យណាស់!", true), "អស្ចារ្យណាស់!");
    }

    #[test]
    fn phrase_spaces_become_breathing_commas() {
        assert_eq!(
            for_voxcpm("ដើម្បីបានថ្មភក់រាប់លានដុំមកសាងសង់ ដំណើរការដ៏ធំសម្បើមមួយត្រូវចាប់ផ្តើម។", true),
            "ដើម្បីបានថ្មភក់រាប់លានដុំមកសាងសង់, ដំណើរការដ៏ធំសម្បើមមួយត្រូវចាប់ផ្តើម."
        );
        assert_eq!(for_voxcpm("ក   ខ\n\nគ។", true), "ក, ខ, គ.");
    }

    #[test]
    fn spaces_around_punctuation_and_latin_names_stay_clean() {
        assert_eq!(for_voxcpm("  នេះជាការសាកល្បង ។  ", true), "នេះជាការសាកល្បង.");
        assert_eq!(for_voxcpm("តើវាជាអ្វី ? វាជាថ្ម។", true), "តើវាជាអ្វី? វាជាថ្ម.");
        assert_eq!(for_voxcpm("ទីមួយ។ទីពីរ។", true), "ទីមួយ. ទីពីរ.");
        assert_eq!(for_voxcpm("យាន Apollo 11 បានចុះចត។", true), "យាន Apollo 11 បានចុះចត.");
        assert_eq!(for_voxcpm("ប្រើ React Native ដើម្បីបង្កើតកម្មវិធី។", true), "ប្រើ React Native ដើម្បីបង្កើតកម្មវិធី.");
        assert_eq!(for_voxcpm("ហេតុផលមានពីរ៖ ទីមួយ ទីពីរ។", true), "ហេតុផលមានពីរ, ទីមួយ, ទីពីរ.");
        assert_eq!(for_voxcpm("ពីរៗ នាក់", true), "ពីរៗ, នាក់.");
        assert_eq!(for_voxcpm("គាត់ថា \"សួស្តី។\"", true), "គាត់ថា \"សួស្តី.\"");
        assert_eq!(for_voxcpm("ក ,។", true), "ក.");
    }

    #[test]
    fn pieces_without_an_end_mark_get_one_by_position() {
        assert_eq!(for_voxcpm("ដើម្បីបានថ្មភក់", false), "ដើម្បីបានថ្មភក់,");
        assert_eq!(for_voxcpm("ដើម្បីបានថ្មភក់", true), "ដើម្បីបានថ្មភក់.");
        assert_eq!(for_voxcpm("ដើម្បីបានថ្មភក់ ", true), "ដើម្បីបានថ្មភក់.");
        assert_eq!(for_voxcpm("", true), "");
    }
}
