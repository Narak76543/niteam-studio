import { describe, expect, it } from "vitest";

import { planCues } from "./prepareSubtitles";

const LONG =
  "ប្រាសាទអង្គរវត្តត្រូវបានសាងសង់ឡើងនៅដើមសតវត្សរ៍ទីដប់ពីរ ក្រោមរជ្ជកាលព្រះបាទសូរ្យវរ្ម័នទីពីរ ដើម្បីឧទ្ទិសដល់ព្រះវិស្ណុ។ " +
  "កម្មករជាច្រើនម៉ឺននាក់បានដឹកថ្មភក់ពីភ្នំគូលេនតាមប្រឡាយទឹក។";

describe("planCues", () => {
  it("covers the whole voice with at most two lines per cue, in order", () => {
    const cues = planCues(
      { scene_id: "s001", narration: LONG, duration_ms: 9000, words: null, hash: "h" },
      "km",
    );
    expect(cues.length).toBeGreaterThan(1);
    expect(cues[0]?.start_ms).toBe(0);
    expect(cues.at(-1)?.end_ms).toBe(9000);
    for (const [i, c] of cues.entries()) {
      expect(c.lines.length).toBeLessThanOrEqual(2);
      expect(c.end_ms).toBeGreaterThanOrEqual(c.start_ms);
      if (i > 0) expect(c.start_ms).toBe(cues[i - 1]?.end_ms);
    }
  });

  it("uses Edge phrase timings when present", () => {
    const narration = "តើស្រូវកើតមកពីណា? នេះជារឿងចៃដន្យ។";
    const words = [
      { text: "តើស្រូវកើតមកពីណា?", start_ms: 100, end_ms: 1500 },
      { text: "នេះជារឿងចៃដន្យ។", start_ms: 1900, end_ms: 3000 },
    ];
    const cues = planCues({ scene_id: "s001", narration, duration_ms: 3500, words, hash: "h" }, "km");
    expect(cues).toHaveLength(1);
    expect(cues[0]).toMatchObject({ start_ms: 0, end_ms: 3500 });
  });

  it("returns nothing for an empty narration", () => {
    expect(planCues({ scene_id: "s", narration: " ", duration_ms: 1000, words: null, hash: "h" }, "km")).toEqual([]);
  });
});
