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
   would exceed **~38 graphemes** (count with the grapheme segmenter, not `.length`;
   vertical 9:16 projects: ~24 km / 26 en — `MAX_GRAPHEMES_VERTICAL` in
   `src/lib/subtitles.ts`, provisional until the vertical layout is tuned, 03 Step 6).
3. Prefer breaking at a space/phrase boundary if one exists in the last ~30 % of the line.
4. Never end a line with a single short word; never start a line with `។`.
5. Max 2 lines on screen at once.

Unit-test with real sentences (`src/lib/subtitles.test.ts`), e.g.
`តើស្រូវកើតមកពីណា? នេះជារឿងចៃដន្យដ៏ធំបំផុតក្នុងប្រវត្តិសាស្ត្រ។`
Assert: no line breaks inside a word, each line ≤ 38 graphemes (16:9).

## 3. Fonts & rendering

- Bundle **Noto Sans Khmer** and **Kantumruy Pro** (both SIL OFL). **Subtitles use
  Kantumruy Pro** (owner's choice, 2026-10-15) for Khmer *and* Latin, registered as the family
  `"Kantumruy Pro Subtitle"` (Khmer + Latin + Latin-ext subsets) so the UI font stack is
  unchanged.
  Load with `@font-face` and wait for `document.fonts.load(...)` **before** drawing
  to canvas, otherwise the first PNGs render with a fallback font.
- Khmer needs more vertical space than Latin (subscripts below, vowels above):
  line height ≥ 1.6 × font size; canvas padding top/bottom ≥ 0.4 × font size.
- Subtitle size: 44 px at 1080p (29 px at 720p), weight 400 (regular; owner 2026-10-17, was
  56 px / 600). White fill, ~4 px dark stroke
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

The script model must produce `spoken_text` with numbers left as **digits**, written as they should be
**spoken in Khmer**. The app converts the digits itself (`khmer_numbers.rs`); the model must never
spell numbers out in Khmer words — in practice it hallucinates the rare tens words (observed in real
projects: ចៅសិប for ចិតសិប, សែសាប់ for សែសិប, ប្រាំបាំ, dropped hundred-groups like ប្រាំរយ for
ប្រាំបួនរយ, an English-calque និង inside the year, leftover digits such as ដប់ប្រាំ៦).

| In narration | In spoken_text |
|---|---|
| Numbers `1,500` / `១៥០០` | digits stay `1,500` → app voices មួយពាន់ប្រាំរយ |
| Years `1863` | `ឆ្នាំ 1863` (digits) → app voices ឆ្នាំមួយពាន់ប្រាំបីរយហុកសិបបី |
| Percent `30%` | `30%` → app voices សាមសិបភាគរយ |
| Units `5 km` | `5 គីឡូម៉ែត្រ` (unit words stay Khmer) |
| Foreign names (e.g. "Mesopotamia") | Khmer transliteration: មេសូប៉ូតាមី |
| Abbreviations `BC`, `AD` | មុនគ្រិស្តសករាជ / គ្រិស្តសករាជ (words: digits alone are not enough) |

The deterministic converter in Rust (`pipeline/khmer_numbers.rs`) is therefore the **primary**
number-to-speech path, not just a fallback: it is applied to every Khmer scene right before TTS
(`voice_job.rs`) and reads any digits (Arabic `0-9` or Khmer `០-៩`) left in `spoken_text` —
`1,500`, `3.5` → បីចុចប្រាំ, `30%` → សាមសិបភាគរយ. Unit-test it (0, 10, 11, 20, 100, 1,000, 10,000,
100,000, 1,000,000, years like 1863 and 2026).

Khmer number words reference: មួយ ពីរ បី បួន ប្រាំ ប្រាំមួយ ប្រាំពីរ ប្រាំបី ប្រាំបួន ·
ដប់ (10) · ម្ភៃ (20) · សាមសិប (30) · សែសិប (40) · ហាសិប (50) · ហុកសិប (60) ·
ចិតសិប (70) · ប៉ែតសិប (80) · កៅសិប (90) · រយ (100) · ពាន់ (1,000) · ម៉ឺន (10,000) ·
សែន (100,000) · លាន (1,000,000).
Implemented in `src-tauri/src/pipeline/khmer_numbers.rs`.
**Have a native speaker verify the converter output before release.**

## 4b. Voices

- **Edge TTS** (default, unofficial): `km-KH-SreymomNeural` (female, default) and
  `km-KH-PisethNeural` (male). Rate and pitch are adjustable in Settings.
- **Gemini TTS**: multilingual voices (`Kore`, …); Khmer is detected from the text.
- Edge's WordBoundary events for Khmer are **phrase-level**: one entry per space-separated chunk
  (it does not split Khmer words). Subtitle timing therefore snaps line breaks to phrase pauses
  when a break falls at a phrase end, and interpolates by grapheme count inside a phrase
  (`src/lib/subtitleTiming.ts`). This is one more reason to keep phrase spaces in `narration`.

## 4c. Names stay as written

Names of products, brands, companies, people (e.g. Neil Armstrong, Buzz Aldrin, Michael Collins),
spacecraft and missions (e.g. Apollo 11, Saturn V, Eagle, Columbia), apps, software, devices and
technical terms (React Native, Android, iPhone, Google, YouTube, JavaScript, Wi-Fi, AI …) are
**never translated or transliterated**: they stay in Latin letters with their usual capitalization
in **BOTH `narration` (subtitles) AND `spoken_text` (TTS input)**. This allows speech synthesis
engines (like VoxCPM2 and multilingual TTS) to speak them directly in clean, authentic English.
The project's **Names** list (`project.keep_terms`, Script page → Names) adds terms that must
appear exactly as written. Rule text: `gemini::script::names_rule` (script prompts + proofreader).
Latin words inside Khmer subtitles use Kantumruy Pro's Latin glyphs.

ASCII digits belonging to English names, designations or models (e.g. `Apollo 11`, `Saturn 5`,
`Falcon 9`, `MP4`, `F-1`) are preserved by `khmer_numbers::replace_digits` so TTS speaks them
in English rather than converting them to Khmer words. Khmer numbers and years (like `ឆ្នាំ 1863`,
`1,500 នាក់`, `30%`) continue to be converted to Khmer words as usual.

## 5. Script quality

- Ask the model for natural spoken Khmer, not a literal translation of English.
- Prefer common words over formal/literary vocabulary unless the style asks for it.
- The UI must let the user edit `narration` and `spoken_text` separately.

## 6. UI

- All UI strings in `src/i18n/km.json` and `en.json`; Khmer UI is the default
  when the Windows display language is Khmer.
- Use the bundled Khmer font for the whole UI too (Windows' default Khmer font
  rendering is inconsistent).
