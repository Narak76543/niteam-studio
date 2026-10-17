import { describe, expect, it } from "vitest";

import { assetId, cropAt, licenseLabel } from "./visuals";
import type { StockCandidate } from "./types";

describe("cropAt", () => {
  it("matches the Rust focus crop", () => {
    expect(cropAt(1600, 2000, [0.5, 0.1])).toEqual({ x: 0, y: 0, w: 1600, h: 900 });
    expect(cropAt(1600, 2000, [0.5, 0.6])).toEqual({ x: 0, y: 750, w: 1600, h: 900 });
    expect(cropAt(4000, 900, [0.95, 0.5])).toEqual({ x: 2400, y: 0, w: 1600, h: 900 });
  });
});

describe("licence labels and asset ids", () => {
  it("mirror Rust", () => {
    expect(licenseLabel({ code: "by_sa", version: "2.0 fr", url: null })).toBe("CC BY-SA 2.0 fr");
    expect(licenseLabel({ code: "pdm", version: null, url: null })).toBe("Public domain");
    const c = { source: "openverse", source_id: "c025bb1a-e95b/../x" } as StockCandidate;
    expect(assetId(c)).toBe("openverse_c025bb1a-e95b____x");
  });
});
