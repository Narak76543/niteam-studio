import { describe, expect, it } from "vitest";

import { followScroll } from "./timeline";

// Timeline 1152 px wide, sticky label column 44 px (as measured at 1280×720).
const W = 1152;
const L = 44;

describe("followScroll", () => {
  it("leaves the timeline alone while the playhead is visible", () => {
    expect(followScroll(L + 300, 0, W, L)).toBeNull();
    expect(followScroll(L + 1500, 900, W, L)).toBeNull();
  });

  it("scrolls forward when the playhead runs off the right edge (playback / seek ahead)", () => {
    const next = followScroll(L + 2400, 0, W, L);
    expect(next).not.toBeNull();
    const s = next ?? 0;
    // Afterwards the playhead sits inside the visible track area, a third of the way in.
    expect(L + 2400).toBeGreaterThanOrEqual(s + L);
    expect(L + 2400).toBeLessThanOrEqual(s + W - 40);
  });

  it("scrolls back when seeking to an earlier time, never below 0", () => {
    expect(followScroll(L + 100, 1200, W, L)).toBe(0);
    const s = followScroll(L + 900, 1500, W, L) ?? -1;
    expect(L + 900).toBeGreaterThanOrEqual(s + L);
    expect(L + 900).toBeLessThanOrEqual(s + W - 40);
  });

  it("a playhead hidden under the sticky label column counts as off-screen", () => {
    expect(followScroll(500 + 10, 500, W, L)).not.toBeNull();
  });
});
