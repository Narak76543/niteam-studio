// Real-world Tech Explainer helpers (docs/11-real-world-visuals.md); mirror the Rust rules.

import type { Scene, StockCandidate, StockLicense } from "./types";

/** Mirrors `Scene::needs_ai_image`: AI-image scenes and photo/clip scenes whose search failed. */
export function needsAiImage(s: Scene): boolean {
  const v = s.visual;
  switch (v.kind) {
    case "ai_image":
      return true;
    case "real_photo":
    case "stock_video":
      return v.asset === null && v.status === "failed";
    default:
      return false;
  }
}

/** The scene's picture is ready for the video (voice aside). */
export function visualReady(s: Scene): boolean {
  if (needsAiImage(s)) return s.image.status === "done";
  const v = s.visual;
  switch (v.kind) {
    case "real_photo":
    case "stock_video":
    case "screenshot":
      return v.status === "done" && v.asset !== null;
    default:
      return v.status === "done";
  }
}

/** Mirrors `License::label`. */
export function licenseLabel(l: StockLicense): string {
  const v = l.version ? ` ${l.version}` : "";
  switch (l.code) {
    case "pexels":
      return "Pexels License";
    case "pixabay":
      return "Pixabay Content License";
    case "cc0":
      return "CC0";
    case "pdm":
      return "Public domain";
    case "by":
      return `CC BY${v}`;
    case "by_sa":
      return `CC BY-SA${v}`;
    case "own":
      return "Own";
  }
}

/** Mirrors `Candidate::asset_id`. */
export function assetId(c: StockCandidate): string {
  const id = c.source_id.replace(/[^A-Za-z0-9-]/g, "_").slice(0, 64);
  return `${c.source}_${id}`;
}

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** Mirrors `postprocess::crop_16x9_at`: largest 16:9 rectangle centred on `focus` (0..1). */
export function cropAt(width: number, height: number, focus: [number, number]): Rect {
  const wide = width * 9 >= height * 16;
  const w = wide ? Math.min(width, Math.floor((height * 16) / 9)) : width;
  const h = wide ? height : Math.min(height, Math.floor((width * 9) / 16));
  const place = (size: number, crop: number, f: number) => {
    const centre = Math.round(Math.min(1, Math.max(0, f)) * size);
    return Math.min(size - crop, Math.max(0, centre - Math.floor(crop / 2)));
  };
  return { x: place(width, w, focus[0]), y: place(height, h, focus[1]), w, h };
}
