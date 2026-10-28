import type { Aspect } from "./types";

export const LONG_EDGE = 1280;

/** Preview canvas size for the aspect (one source of truth: `render::clip::Frame`). */
export function stageFor(aspect: Aspect) {
  // Crop share of the pan-size image (frame over pan size, e.g. 1920 of 2150 px, 1080 of 1210).
  if (aspect === "9:16") {
    const w = Math.round((LONG_EDGE * 1080) / 1920);
    return { w, h: LONG_EDGE, scale: w / 1080, cropW: 1080 / 1210, cropH: 1920 / 2150, vertical: true };
  }
  const scale = LONG_EDGE / 1920;
  return { w: LONG_EDGE, h: Math.round((LONG_EDGE * 1080) / 1920), scale, cropW: 1920 / 2150, cropH: 1080 / 1210, vertical: false };
}
