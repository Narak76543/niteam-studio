import { describe, expect, it } from "vitest";

import { formatParsedScript, parseScriptDraft } from "./scriptImport";

describe("parseScriptDraft", () => {
  it("reads scene blocks, ignores chapters and fact notes", () => {
    const text = [
      "Python",
      "Khmer · 5 min · 2 scenes",
      "",
      "Chapter 1 — Start",
      "Scene 1",
      "Narration: សួស្តី។",
      "Spoken: សួស្តី។",
      "",
      "Chapter 2 — End",
      "Scene 2",
      "Narration: លា។",
      "Spoken: លា។",
      "Fact check: released in 1991.",
    ].join("\n");

    expect(parseScriptDraft(text)).toEqual({
      scenes: [
        { order: 1, narration: "សួស្តី។", spoken_text: "សួស្តី។" },
        { order: 2, narration: "លា។", spoken_text: "លា។" },
      ],
      errors: [],
    });
  });

  it("tolerates Markdown labels and joins wrapped lines", () => {
    const text = [
      "```text",
      "**Chapter 1**",
      "**Scene 1**",
      "**Narration:** first line",
      "continues here.",
      "**Spoken:** first line continues here.",
      "```",
    ].join("\n");

    expect(parseScriptDraft(text)).toEqual({
      scenes: [{ order: 1, narration: "first line continues here.", spoken_text: "first line continues here." }],
      errors: [],
    });
  });

  it("reports duplicate scene numbers and missing fields", () => {
    const text = [
      "Scene 1",
      "Narration: A",
      "Spoken: A",
      "Scene 1",
      "Narration: B",
      "Scene 2",
      "Narration: C",
      "Spoken: C",
    ].join("\n");

    const result = parseScriptDraft(text);

    expect(result.scenes).toHaveLength(3);
    expect(result.errors).toEqual([
      { kind: "duplicate", order: 1 },
      { kind: "missingFields", order: 1 },
    ]);
  });

  it("returns nothing for empty text", () => {
    expect(parseScriptDraft("  \n\n")).toEqual({ scenes: [], errors: [] });
  });

  it("accepts Khmer labels, Khmer digits and the Khmer colon", () => {
    const text = [
      "ជំពូកទី ១ — ការចាប់ផ្តើម",
      "ឈុតទី ១",
      "អត្ថបទនិទាន៖ សួស្តី។",
      "អត្ថបទសម្រាប់អាន៖ សួស្តី។",
    ].join("\n");

    expect(parseScriptDraft(text)).toEqual({
      scenes: [{ order: 1, narration: "សួស្តី។", spoken_text: "សួស្តី។" }],
      errors: [],
    });
  });

  it("accepts alternate labels, labels on their own line, and normalizes spacing", () => {
    const text = [
      "- Scene #2",
      "**Narration**",
      "first   line",
      "continues.",
      "**Voice-over**: first line continues.",
      "Fact-check: ignored.",
    ].join("\n");

    expect(parseScriptDraft(text)).toEqual({
      scenes: [{ order: 2, narration: "first line continues.", spoken_text: "first line continues." }],
      errors: [],
    });
  });

  it("formats scenes in the plain-text format the parser accepts", () => {
    const parsed = parseScriptDraft(
      ["Scene 1", "Narration: សួស្តី។", "Spoken: សួស្តី។", "Scene 2", "Narration: លា។", "Spoken: លា។"].join("\n"),
    );

    expect(formatParsedScript(parsed.scenes)).toBe(
      "Scene 1\nNarration: សួស្តី។\nSpoken: សួស្តី។\n\nScene 2\nNarration: លា។\nSpoken: លា។",
    );
    expect(parseScriptDraft(formatParsedScript(parsed.scenes))).toEqual(parsed);
  });
});
