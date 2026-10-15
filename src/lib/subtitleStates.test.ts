import { describe, expect, it } from "vitest";

import { stateAt, subtitleStates } from "./subtitleStates";

const LONG =
  "ប្រាសាទអង្គរវត្តត្រូវបានសាងសង់ឡើងនៅដើមសតវត្សរ៍ទីដប់ពីរ ក្រោមរជ្ជកាលព្រះបាទសូរ្យវរ្ម័នទីពីរ ដើម្បីឧទ្ទិសដល់ព្រះវិស្ណុ។ " +
  "កម្មករជាច្រើនម៉ឺននាក់បានដឹកថ្មភក់ពីភ្នំគូលេនតាមប្រឡាយទឹក។";

describe("subtitleStates", () => {
  it("without the tracker: one state per cue, ≤ 2 lines, covering the whole voice", () => {
    const st = subtitleStates({ narration: LONG, duration_ms: 9000, words: null }, "km", false);
    expect(st.length).toBeGreaterThan(1);
    expect(st[0]?.start_ms).toBe(0);
    expect(st.at(-1)?.end_ms).toBe(9000);
    for (const [i, s] of st.entries()) {
      expect(s.lines.length).toBeLessThanOrEqual(2);
      expect(s.active).toBeNull();
      if (i > 0) expect(s.start_ms).toBe(st[i - 1]?.end_ms);
    }
  });

  it("with the tracker: one state per word, consecutive, the active word walks through the cue", () => {
    const st = subtitleStates({ narration: LONG, duration_ms: 9000, words: null }, "km", true);
    const words = st[0]?.lines.flat().length ?? 0;
    expect(words).toBeGreaterThan(2);
    expect(st.slice(0, words).map((s) => s.active)).toEqual([...Array(words).keys()]);
    expect(st[words]?.active).toBe(0); // next cue starts again at its first word
    expect(st[0]?.start_ms).toBe(0);
    expect(st.at(-1)?.end_ms).toBe(9000);
    for (let i = 1; i < st.length; i++) expect(st[i]?.start_ms).toBe(st[i - 1]?.end_ms);
  });

  it("follows Edge phrase timings: the second phrase is highlighted only after its start", () => {
    const narration = "តើស្រូវកើតមកពីណា? នេះជារឿងចៃដន្យ។";
    const words = [
      { text: "តើស្រូវកើតមកពីណា?", start_ms: 100, end_ms: 1500 },
      { text: "នេះជារឿងចៃដន្យ។", start_ms: 1900, end_ms: 3000 },
    ];
    const st = subtitleStates({ narration, duration_ms: 3500, words }, "km", true);
    const firstOfPhrase2 = st.findIndex((s) => s.lines.flat()[s.active ?? 0]?.text.startsWith("នេះ"));
    expect(firstOfPhrase2).toBeGreaterThan(0);
    expect(st[firstOfPhrase2]?.start_ms).toBeGreaterThanOrEqual(1500);
    expect(st[firstOfPhrase2]?.start_ms).toBeLessThanOrEqual(1900);
  });

  it("stateAt finds the state on screen, the last one holds until the end", () => {
    const st = subtitleStates({ narration: LONG, duration_ms: 9000, words: null }, "km", true);
    expect(stateAt(st, 0)).toBe(st[0]);
    expect(stateAt(st, 8999)).toBe(st.at(-1));
    expect(stateAt(st, 20_000)).toBe(st.at(-1));
    const mid = st[3];
    if (mid) expect(stateAt(st, mid.start_ms)).toBe(mid);
    expect(stateAt([], 10)).toBeNull();
  });

  it("returns nothing for an empty narration or no voice", () => {
    expect(subtitleStates({ narration: " ", duration_ms: 1000, words: null }, "km", true)).toEqual([]);
    expect(subtitleStates({ narration: LONG, duration_ms: 0, words: null }, "km", true)).toEqual([]);
  });
});
