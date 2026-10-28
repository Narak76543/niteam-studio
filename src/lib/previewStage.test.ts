import { describe, expect, it } from "vitest";

import { LONG_EDGE, stageFor } from "./previewStage";

describe("stageFor", () => {
  it("16:9: 1280×720 preview, exact 16/9", () => {
    const s = stageFor("16:9");
    expect(s.w).toBe(LONG_EDGE);
    expect(s.h).toBe(Math.round((LONG_EDGE * 1080) / 1920));
    expect(s.vertical).toBe(false);
    expect(s.w / s.h).toBeCloseTo(16 / 9, 10);
  });

  it("9:16: preview 720×1280 crop of the 1080×1920 frame, exact 9/16", () => {
    const s = stageFor("9:16");
    expect(s.w).toBe(Math.round((LONG_EDGE * 1080) / 1920));
    expect(s.h).toBe(LONG_EDGE);
    expect(s.vertical).toBe(true);
    expect(s.w / s.h).toBeCloseTo(9 / 16, 10);
    expect(s.cropH).toBeCloseTo(1920 / 2150, 10);
    expect(s.cropW).toBeCloseTo(1080 / 1210, 10);
  });
});
