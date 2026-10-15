import { describe, expect, it } from "vitest";

import { MAX_GRAPHEMES, graphemeCount, groupCues, splitLines } from "./subtitles";

const words = new Intl.Segmenter("km", { granularity: "word" });

/** Word boundaries of the text with spaces removed (character offsets). */
function wordBoundaries(text: string): Set<number> {
  const flat = text.replace(/\s+/gu, "");
  const set = new Set<number>([0, flat.length]);
  let pos = 0;
  for (const { segment } of words.segment(flat)) {
    pos += segment.length;
    set.add(pos);
  }
  return set;
}

/** Every line break falls on a word boundary of the original text. */
function expectBreaksOnWords(text: string, lines: string[]) {
  const ok = wordBoundaries(text.normalize("NFC"));
  let pos = 0;
  for (const line of lines.slice(0, -1)) {
    pos += line.replace(/\s+/gu, "").length;
    expect(ok.has(pos), `break after "${line}"`).toBe(true);
  }
  expect(lines.join("").replace(/\s+/gu, "")).toBe(text.normalize("NFC").replace(/\s+/gu, ""));
}

const KM_SAMPLE = "តើស្រូវកើតមកពីណា? នេះជារឿងចៃដន្យដ៏ធំបំផុតក្នុងប្រវត្តិសាស្ត្រ។";
const KM_LONG =
  "ប្រាសាទអង្គរវត្តត្រូវបានសាងសង់ឡើងនៅដើមសតវត្សរ៍ទីដប់ពីរ ក្រោមរជ្ជកាលព្រះបាទសូរ្យវរ្ម័នទីពីរ ដើម្បីឧទ្ទិសដល់ព្រះវិស្ណុ។ " +
  "កម្មករជាច្រើនម៉ឺននាក់បានដឹកថ្មភក់ពីភ្នំគូលេនតាមប្រឡាយទឹក។";

describe("splitLines (Khmer)", () => {
  it("keeps every line within 38 graphemes and breaks only between words", () => {
    // KM_SAMPLE is 62 code points but only 31 graphemes: it fits on one line.
    expect(splitLines(KM_SAMPLE, "km")).toHaveLength(1);
    expect(splitLines(KM_LONG, "km").length).toBeGreaterThan(1);
    for (const text of [KM_SAMPLE, KM_LONG]) {
      const lines = splitLines(text, "km");
      for (const l of lines) expect(graphemeCount(l), l).toBeLessThanOrEqual(MAX_GRAPHEMES.km);
      expectBreaksOnWords(text, lines);
    }
  });

  it("prefers the phrase boundary (space) near the end of a line", () => {
    // The first phrase is 10 graphemes: inside the last 30 % of a 13-grapheme line.
    const lines = splitLines(KM_SAMPLE, "km", 13);
    expect(lines[0]).toBe("តើស្រូវកើតមកពីណា?");
  });

  it("never starts a line with ។ or other punctuation", () => {
    for (const l of splitLines(KM_LONG, "km")) expect(l).not.toMatch(/^[។៕!?,.]/u);
  });

  it("never splits a grapheme cluster (coeng subscripts stay together)", () => {
    // One long run without spaces forces word-level breaks.
    const text = "ស្ត្រីក្មេងៗស្រឡាញ់ស្រូវក្រអូបព្រោះវាឆ្ងាញ់ហើយក្លិនក្រអូបខ្លាំងណាស់";
    const lines = splitLines(text, "km");
    for (const l of lines) {
      expect(l.normalize("NFC")).toBe(l);
      expect(l.startsWith("្")).toBe(false);
      expect(graphemeCount(l)).toBeLessThanOrEqual(MAX_GRAPHEMES.km);
    }
    expectBreaksOnWords(text, lines);
  });

  it("uses zero-width spaces as break hints and removes them", () => {
    const lines = splitLines("ប្រាសាទ​អង្គរវត្ត", "km", 5);
    expect(lines.join("|")).toBe("ប្រាសាទ|អង្គរវត្ត");
    for (const l of lines) expect(l).not.toContain("​");
  });

  it("does not leave a short single word alone on the last line", () => {
    const lines = splitLines(KM_LONG, "km");
    const last = lines.at(-1) ?? "";
    expect(graphemeCount(last)).toBeGreaterThan(3);
  });

  it("returns nothing for empty text", () => {
    expect(splitLines("   ", "km")).toEqual([]);
  });
});

describe("splitLines (English)", () => {
  it("keeps lines within 42 characters and words whole", () => {
    const text =
      "Angkor Wat was built in the early twelfth century, under King Suryavarman the Second, to honour the god Vishnu.";
    const lines = splitLines(text, "en");
    for (const l of lines) {
      expect(l.length).toBeLessThanOrEqual(42);
      expect(l).toBe(l.trim());
    }
    expect(lines.join(" ")).toBe(text);
  });
});

describe("groupCues", () => {
  it("puts at most two lines on screen", () => {
    expect(groupCues(["a", "b", "c"])).toEqual([["a", "b"], ["c"]]);
    expect(groupCues([])).toEqual([]);
  });
});
