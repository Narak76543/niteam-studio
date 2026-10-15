import { describe, expect, it } from "vitest";

import { diffText } from "./textDiff";

const join = (parts: { text: string; kind: string }[], keep: string[]) =>
  parts.filter((p) => keep.includes(p.kind)).map((p) => p.text).join("");

describe("diffText", () => {
  it("rebuilds both texts and marks only the change", () => {
    const before = "វាល្អណាស";
    const after = "វាល្អណាស់";
    const d = diffText(before, after);
    expect(join(d, ["same", "del"])).toBe(before);
    expect(join(d, ["same", "ins"])).toBe(after);
    expect(d.filter((p) => p.kind !== "same").length).toBeGreaterThan(0);
  });

  it("keeps a Khmer cluster whole (no half-cluster changes)", () => {
    const d = diffText("ស្រូវ", "ស្រូ");
    for (const p of d) expect(p.text.startsWith("្")).toBe(false);
  });

  it("no change → one 'same' part", () => {
    expect(diffText("React Native", "React Native")).toEqual([{ text: "React Native", kind: "same" }]);
  });

  it("handles empty sides", () => {
    expect(diffText("", "ក")).toEqual([{ text: "ក", kind: "ins" }]);
    expect(diffText("ក", "")).toEqual([{ text: "ក", kind: "del" }]);
  });
});
