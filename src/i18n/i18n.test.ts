import { describe, expect, it } from "vitest";

import en from "./en.json";
import km from "./km.json";
import { translate } from "./context";
import { estimateScenes } from "../lib/format";

describe("i18n", () => {
  it("km.json and en.json have the same keys", () => {
    expect(Object.keys(km).sort()).toEqual(Object.keys(en).sort());
  });

  it("no translation is empty", () => {
    for (const [key, value] of Object.entries(km)) {
      expect(value.trim(), key).not.toBe("");
    }
  });

  it("keeps the same placeholders in both languages", () => {
    const vars = (s: string) => (s.match(/\{\w+\}/g) ?? []).sort();
    for (const key of Object.keys(en) as (keyof typeof en)[]) {
      expect(vars(km[key]), key).toEqual(vars(en[key]));
    }
  });

  it("interpolates variables", () => {
    expect(translate("en", "script.progress.chapter", { n: 2, total: 5 })).toBe("Writing chapter 2 of 5…");
    expect(translate("km", "script.chapter", { n: 3 })).toBe("ជំពូកទី 3");
  });
});

describe("estimateScenes", () => {
  it("matches the Rust plan (3 min ≈ 26 scenes, 20 min = 171)", () => {
    expect(estimateScenes(3)).toBe(26);
    expect(estimateScenes(20)).toBe(171);
    expect(estimateScenes(1)).toBe(9);
  });
});
