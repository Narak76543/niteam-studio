import { describe, expect, it } from "vitest";

import { settingsPatch } from "./settingsPatch";

describe("settingsPatch", () => {
  const base = {
    ui_language: "km",
    output_dir: "D:/videos",
    models: { text_chain: ["a", "b"] },
    local: { cooldown_every: 0, offer_dismissed: false },
  };

  it("is empty when nothing changed", () => {
    expect(settingsPatch(base, structuredClone(base))).toEqual({});
  });

  it("keeps only the changed nested fields", () => {
    const next = { ...base, local: { ...base.local, cooldown_every: 5 } };
    expect(settingsPatch(base, next)).toEqual({ local: { cooldown_every: 5 } });
  });

  it("replaces arrays whole and sends null for cleared values", () => {
    const next = { ...base, output_dir: null, models: { text_chain: ["b"] } };
    expect(settingsPatch(base, next)).toEqual({ output_dir: null, models: { text_chain: ["b"] } });
  });
});
