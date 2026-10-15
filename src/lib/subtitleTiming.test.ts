import { describe, expect, it } from "vitest";

import { spokenLength, timeLines } from "./subtitleTiming";

// Real Edge TTS output for km-KH-SreymomNeural (phrase-level boundaries).
const WORDS = [
  { text: "តើប្រាសាទអង្គរវត្តត្រូវបានសាងសង់ឡើងដោយរបៀបណា", start_ms: 112, end_ms: 3262 },
  { text: "នេះជារឿងដ៏អស្ចារ្យ", start_ms: 4250, end_ms: 5412 },
  { text: "ដែលមានអាយុជាងប្រាំបួនរយឆ្នាំ។", start_ms: 5462, end_ms: 7325 },
];
const DURATION = 8256;

describe("spokenLength", () => {
  it("counts Khmer clusters, not code units, and ignores spaces/punctuation", () => {
    expect(spokenLength("ស្រូវ")).toBe(2); // ស្រូ + វ
    expect(spokenLength("ស្រូវ ។?")).toBe(2);
    expect(spokenLength("Hi, you!")).toBe(5);
  });
});

describe("timeLines", () => {
  it("falls back to proportional timing without words", () => {
    const t = timeLines(["aaaa", "bbbb"], null, 1000);
    expect(t).toEqual([
      { start_ms: 0, end_ms: 500 },
      { start_ms: 500, end_ms: 1000 },
    ]);
  });

  it("puts the line break in the pause between phrases", () => {
    const lines = [
      "តើប្រាសាទអង្គរវត្តត្រូវបានសាងសង់ឡើងដោយរបៀបណា?",
      "នេះជារឿងដ៏អស្ចារ្យ ដែលមានអាយុជាងប្រាំបួនរយឆ្នាំ។",
    ];
    const t = timeLines(lines, WORDS, DURATION);
    expect(t[0]?.start_ms).toBe(0);
    // Boundary between phrase 1 (ends 3262) and phrase 2 (starts 4250).
    expect(t[0]?.end_ms).toBeGreaterThanOrEqual(3262);
    expect(t[0]?.end_ms).toBeLessThanOrEqual(4250);
    expect(t[1]).toEqual({ start_ms: t[0]?.end_ms, end_ms: DURATION });
  });

  it("interpolates inside a phrase and stays monotonic", () => {
    const lines = ["តើប្រាសាទអង្គរវត្ត", "ត្រូវបានសាងសង់ឡើងដោយរបៀបណា?", "នេះជារឿងដ៏អស្ចារ្យ", "ដែលមានអាយុជាងប្រាំបួនរយឆ្នាំ។"];
    const t = timeLines(lines, WORDS, DURATION);
    expect(t).toHaveLength(4);
    t.forEach((line, i) => {
      if (i > 0) expect(line.start_ms).toBe(t[i - 1]?.end_ms);
      expect(line.end_ms).toBeGreaterThanOrEqual(line.start_ms);
    });
    // First break falls inside phrase 1 (112..3262), not at a proportional guess of the scene.
    expect(t[0]?.end_ms).toBeGreaterThan(112);
    expect(t[0]?.end_ms).toBeLessThan(3262);
  });

  it("tolerates narration that differs from the spoken text (digits vs words)", () => {
    const t = timeLines(["ឆ្នាំ 1863", "គេបានសាងសង់"], [
      { text: "ឆ្នាំមួយពាន់ប្រាំបីរយហុកសិបបី", start_ms: 0, end_ms: 2000 },
      { text: "គេបានសាងសង់", start_ms: 2300, end_ms: 3000 },
    ], 3200);
    expect(t[0]?.end_ms).toBeGreaterThan(0);
    expect(t[1]?.end_ms).toBe(3200);
  });
});
