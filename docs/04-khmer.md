# 04 — Khmer Rules

Khmer is the main language of this app and the easiest place to ship bugs.
Test every text feature with real Khmer, never only with English.

## 1. Text basics

- Khmer script has **no spaces between words**. Spaces mark phrase/sentence pauses.
- Sentence end: `។` (khan). Also `៕`. Treat them like `.`
- Characters stack: a consonant + subscript (`្` coeng + consonant) + vowels + signs
  form **one visible cluster** (grapheme). Never split inside a cluster.
- Always normalize text to **Unicode NFC** before hashing, measuring or rendering.
- Strip zero-width spaces (U+200B) the model sometimes inserts, *after* using them
  as break hints.

## 2. Line breaking for subtitles

Use the browser's segmenter — it has Khmer dictionary data:

```ts
const words = new Intl.Segmenter("km", { granularity: "word" });
const graphemes = new Intl.Segmenter("km", { granularity: "grapheme" });
```

Algorithm for one scene's narration:
1. Split into sentences on `។ ៕ ! ?` and on spaces (phrase boundaries).
2. Greedily pack **words** (from the word segmenter) into a line until the next word
   would exceed **~38 graphemes** (count with the grapheme segmenter, not `.length`).
3. Prefer breaking at a space/phrase boundary if one exists in the last ~30 % of the line.
4. Never end a line with a single short word; never start a line with `។`.
5. Max 2 lines on screen at once.

Unit-test with real sentences (`src/lib/subtitles.test.ts`), e.g.
`តើស្រូវកើតមកពីណា? នេះជារឿងចៃដន្យដ៏ធំបំផុតក្នុងប្រវត្តិសាស្ត្រ។`
Assert: no line breaks inside a word, each line ≤ 38 graphemes.

## 3. Fonts & rendering

- Bundle **Noto Sans Khmer** and **Kantumruy Pro** (both SIL OFL). **Subtitles use
  Kantumruy Pro** (owner's choice, 2026-10-15) for Khmer *and* Latin, registered as the family
  `"Kantumruy Pro Subtitle"` (Khmer + Latin + Latin-ext subsets) so the UI font stack is
  unchanged.
  Load with `@font-face` and wait for `document.fonts.load(...)` **before** drawing
  to canvas, otherwise the first PNGs render with a fallback font.
- Khmer needs more vertical space than Latin (subscripts below, vowels above):
  line height ≥ 1.6 × font size; canvas padding top/bottom ≥ 0.4 × font size.
- Subtitle size: ~56 px at 1080p, weight 600. White fill, ~6 px dark stroke
  (`ctx.lineJoin = "round"`), soft shadow.
- **Word tracker** (CapCut-style): the whole cue (≤ 2 lines) stays on screen and the word
  being spoken sits on a rounded box in the project's highlight colour (`render.subtitle_highlight`,
  default `#FFD60A`, `null` = off), dark or white text by contrast. The box height follows the
  line's real glyph extent so Khmer vowels above and subscripts below stay inside. Word times:
  Edge phrase timings, interpolated by grapheme count inside a phrase (Edge does not time
  individual Khmer words), or each word's share of the text without timings.
- **Never use FFmpeg `drawtext` or ASS/libass for Khmer** — shaping of subscripts
  breaks depending on the FFmpeg build.

## 4. Text for TTS (`spoken_text`)

The script model must produce `spoken_text` with everything written as it should be
**spoken in Khmer**:

| In narration | In spoken_text |
|---|---|
| Numbers `1,500` / `១៥០០` | Khmer words: មួយពាន់ប្រាំរយ |
| Years `1863` | ឆ្នាំមួយពាន់ប្រាំបីរយហុកសិបបី |
| Percent `30%` | សាមសិបភាគរយ |
| Units `5 km` | ប្រាំគីឡូម៉ែត្រ |
| Foreign names (e.g. "Mesopotamia") | Khmer transliteration: មេសូប៉ូតាមី |
| Abbreviations `BC`, `AD` | មុនគ្រិស្តសករាជ / គ្រិស្តសករាជ |

Also add a deterministic fallback in Rust: a Khmer number-to-words function
(`pipeline/khmer_numbers.rs`) applied to any digits (Arabic `0-9` or Khmer `០-៩`)
left in `spoken_text`. Unit-test it (0, 10, 11, 20, 100, 1,000, 10,000, 100,000,
1,000,000, years like 1863 and 2026).

Khmer number words reference: មួយ ពីរ បី បួន ប្រាំ ប្រាំមួយ ប្រាំពីរ ប្រាំបី ប្រាំបួន ·
ដប់ (10) · ម្ភៃ (20) · សាមសិប (30) · សែសិប (40) · ហាសិប (50) · ហុកសិប (60) ·
ចិតសិប (70) · ប៉ែតសិប (80) · កៅសិប (90) · រយ (100) · ពាន់ (1,000) · ម៉ឺន (10,000) ·
សែន (100,000) · លាន (1,000,000).
Implemented in `src-tauri/src/pipeline/khmer_numbers.rs` (also handles `1,500`, `3.5` → បីចុចប្រាំ
and `30%` → សាមសិបភាគរយ); applied to every Khmer scene right before TTS.
**Have a native speaker verify the converter output before release.**

## 4b. Voices

- **Edge TTS** (default, unofficial): `km-KH-SreymomNeural` (female, default) and
  `km-KH-PisethNeural` (male). Rate and pitch are adjustable in Settings.
- **Gemini TTS**: multilingual voices (`Kore`, …); Khmer is detected from the text.
- Edge's WordBoundary events for Khmer are **phrase-level**: one entry per space-separated chunk
  (it does not split Khmer words). Subtitle timing therefore snaps line breaks to phrase pauses
  when a break falls at a phrase end, and interpolates by grapheme count inside a phrase
  (`src/lib/subtitleTiming.ts`). This is one more reason to keep phrase spaces in `narration`.

## 5. Script quality

- Ask the model for natural spoken Khmer, not a literal translation of English.
- Prefer common words over formal/literary vocabulary unless the style asks for it.
- The UI must let the user edit `narration` and `spoken_text` separately.

## 6. UI

- All UI strings in `src/i18n/km.json` and `en.json`; Khmer UI is the default
  when the Windows display language is Khmer.
- Use the bundled Khmer font for the whole UI too (Windows' default Khmer font
  rendering is inconsistent).
