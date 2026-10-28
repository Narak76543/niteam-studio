import { describe, expect, it } from "vitest";

import type { Outline } from "./types";
import { parseScriptDraft } from "./scriptImport";
import { scriptReviewText } from "./scriptReview";

const OUTLINE: Outline = {
  title: "Angkor",
  hook: "A temple in the jungle",
  chapters: [
    { title: "The beginning", summary: "", scene_count: 2, done: true },
    { title: "The fall", summary: "", scene_count: 1, done: true },
  ],
};

const scene = (order: number, chapter: number, narration: string, spoken = narration, fact: string | null = null) => ({
  order,
  chapter,
  narration,
  spoken_text: spoken,
  fact_note: fact,
});

describe("scriptReviewText", () => {
  it("writes the header and every scene under its chapter", () => {
    const text = scriptReviewText(
      { title: "Angkor Wat", language: "km", target_minutes: 5, outline: OUTLINE },
      [
        scene(1, 0, "សួស្តី។"),
        scene(2, 0, "ប្រាសាទនេះធំណាស់។", "ប្រាសាទនេះ ធំ ណាស់។", "Built in the 12th century"),
        scene(3, 1, "បន្ទាប់មក វាត្រូវបានបោះបង់។"),
      ],
    );

    expect(text).toBe(
      [
        "Angkor Wat",
        "Khmer · 5 min · 3 scenes",
        "",
        "Chapter 1 — The beginning",
        "Scene 1",
        "Narration: សួស្តី។",
        "Spoken: សួស្តី។",
        "",
        "Scene 2",
        "Narration: ប្រាសាទនេះធំណាស់។",
        "Spoken: ប្រាសាទនេះ ធំ ណាស់។",
        "Fact check: Built in the 12th century",
        "",
        "Chapter 2 — The fall",
        "Scene 3",
        "Narration: បន្ទាប់មក វាត្រូវបានបោះបង់។",
        "Spoken: បន្ទាប់មក វាត្រូវបានបោះបង់។",
        "",
      ].join("\n"),
    );
  });

  it("falls back to a bare chapter number without an outline", () => {
    const text = scriptReviewText(
      { title: "T", language: "en", target_minutes: 1, outline: null },
      [scene(1, 0, "Hello.")],
    );

    expect(text).toContain("Chapter 1\nScene 1");
  });

  it("omits an empty fact note", () => {
    const text = scriptReviewText(
      { title: "T", language: "en", target_minutes: 1, outline: null },
      [scene(1, 0, "Hello.", "Hello.", "   ")],
    );

    expect(text).not.toContain("Fact check:");
  });

  it("round-trips through the paste parser", () => {
    const text = scriptReviewText({ title: "T", language: "km", target_minutes: 1, outline: OUTLINE }, [
      scene(1, 0, "សួស្តី។"),
      scene(2, 1, "លា។", "លា ។", "1953"),
    ]);

    expect(parseScriptDraft(text)).toEqual({
      scenes: [
        { order: 1, narration: "សួស្តី។", spoken_text: "សួស្តី។" },
        { order: 2, narration: "លា។", spoken_text: "លា ។" },
      ],
      errors: [],
    });
  });
});
