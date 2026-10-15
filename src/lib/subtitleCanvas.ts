// Draws one subtitle cue (1–2 lines) as a transparent PNG (docs/04-khmer.md §3).
// Khmer is shaped by the WebView, never by FFmpeg drawtext/libass. The band size must match
// `render::clip::Frame` in Rust (1920×240 at 1080p, 1280×160 at 720p).
//
// Changing the look here? Bump `SUBS_VERSION` in src-tauri/src/pipeline/hashing.rs so
// existing subtitle PNGs are redrawn.

import type { Language } from "./types";

export interface Band {
  width: number;
  height: number;
  fontPx: number;
}

export function bandFor(resolution: string): Band {
  return resolution === "720p" ? { width: 1280, height: 160, fontPx: 37 } : { width: 1920, height: 240, fontPx: 56 };
}

const FONT: Record<Language, string> = {
  km: '"Noto Sans Khmer", "Kantumruy Pro", sans-serif',
  en: '"Manrope", "Noto Sans Khmer", sans-serif',
};
const WEIGHT = 600;
/** Khmer stacks above and below the line: ≥ 1.6 × font size. */
const LINE_HEIGHT = 1.6;
/** Text never wider than this share of the frame (shrinks the font if needed). */
const MAX_TEXT_WIDTH = 0.9;

function font(lang: Language, px: number): string {
  return `${WEIGHT} ${px}px ${FONT[lang]}`;
}

/**
 * Waits for the bundled subtitle font. Without this the first PNGs are drawn with a
 * fallback font (docs/04-khmer.md §3).
 */
export async function ensureFont(lang: Language, sample: string, band: Band): Promise<void> {
  const f = font(lang, band.fontPx);
  await document.fonts.load(f, sample);
  if (!document.fonts.check(f, sample)) {
    throw new Error("The subtitle font could not be loaded.");
  }
}

/** PNG bytes of one cue, lines centered, bottom-aligned in the band. */
export async function drawCue(lines: string[], lang: Language, band: Band): Promise<Uint8Array> {
  const canvas = document.createElement("canvas");
  canvas.width = band.width;
  canvas.height = band.height;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("Canvas is not available.");

  // Shrink only if a line would not fit (very long Latin words).
  let px = band.fontPx;
  ctx.font = font(lang, px);
  const widest = Math.max(...lines.map((l) => ctx.measureText(l).width), 1);
  const limit = band.width * MAX_TEXT_WIDTH;
  if (widest > limit) {
    px = Math.floor((px * limit) / widest);
    ctx.font = font(lang, px);
  }

  const lineH = px * LINE_HEIGHT;
  const padBottom = px * 0.4;
  const stroke = Math.max(2, Math.round(px * 0.11)); // ~6 px at 56 px
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.lineJoin = "round";
  ctx.miterLimit = 2;

  lines.forEach((line, i) => {
    const fromBottom = lines.length - 1 - i;
    const y = band.height - padBottom - lineH / 2 - fromBottom * lineH;
    const x = band.width / 2;
    // Dark outline with a soft shadow, then white fill on top.
    ctx.save();
    ctx.shadowColor = "rgba(0, 0, 0, 0.55)";
    ctx.shadowBlur = Math.round(px * 0.18);
    ctx.shadowOffsetY = Math.round(px * 0.05);
    ctx.strokeStyle = "rgba(0, 0, 0, 0.9)";
    ctx.lineWidth = stroke * 2;
    ctx.strokeText(line, x, y);
    ctx.restore();
    ctx.fillStyle = "#ffffff";
    ctx.fillText(line, x, y);
  });

  const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/png"));
  if (!blob) throw new Error("Could not create the subtitle image.");
  return new Uint8Array(await blob.arrayBuffer());
}

export function toBase64(bytes: Uint8Array): string {
  let bin = "";
  const CHUNK = 0x8000;
  for (let i = 0; i < bytes.length; i += CHUNK) {
    bin += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  }
  return btoa(bin);
}
