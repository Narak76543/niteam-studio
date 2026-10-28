// Draws subtitles (docs/04-khmer.md §3): Kantumruy Pro, white text with a dark outline, and
// the word being spoken on a coloured rounded box (the "word tracker"). The same `paintCue`
// draws the editor preview and the PNGs burned into the video, so the preview matches the
// export. Khmer is shaped by the WebView, never by FFmpeg drawtext/libass.
//
// The band sizes must match `render::clip::Frame` in Rust (1920×240 at 1080p 16:9,
// 1280×160 at 720p, 1080×240 at 9:16). Changing the look here? Bump `SUBS_VERSION` in
// src-tauri/src/pipeline/hashing.rs so existing subtitle PNGs are redrawn.

import { lineText, type Word } from "./subtitles";
import type { Aspect } from "./types";

export interface Band {
  width: number;
  height: number;
  fontPx: number;
}

/** Band and margin constants, from `render::clip::Frame` (one source of truth in Rust). */
export function bandFor(resolution: string, aspect: Aspect = "16:9", scale = 1): Band {
  const base =
    aspect === "9:16"
      ? { width: 1080, height: 240, fontPx: 44 }
      : resolution === "720p"
        ? { width: 1280, height: 160, fontPx: 29 }
        : { width: 1920, height: 240, fontPx: 44 };
  return { width: base.width * scale, height: base.height * scale, fontPx: base.fontPx * scale };
}

/** Kantumruy Pro for Khmer and Latin (see src/styles/fonts.css). */
export const SUBTITLE_FONT_FAMILY = "Kantumruy Pro Subtitle";
const FONT_STACK = `"${SUBTITLE_FONT_FAMILY}", "Noto Sans Khmer", sans-serif`;
/** Regular (owner, 2026-10-17: bold 56 px was too heavy and too big). */
const WEIGHT = 400;
/** Khmer stacks above and below the line: ≥ 1.6 × font size. */
const LINE_HEIGHT = 1.6;
/** Text never wider than this share of the frame (shrinks the font if needed). */
const MAX_TEXT_WIDTH = 0.9;

function font(px: number): string {
  return `${WEIGHT} ${px}px ${FONT_STACK}`;
}

/** Waits for the subtitle font; without this the first images use a fallback font. */
export async function ensureFont(sample: string, band: Band): Promise<void> {
  const f = font(band.fontPx);
  await document.fonts.load(f, sample);
  if (!document.fonts.check(f, sample)) {
    throw new Error("The subtitle font could not be loaded.");
  }
}

/** Dark text on light highlight colours, white on dark ones. */
function textOn(hex: string): string {
  const n = Number.parseInt(hex.slice(1), 16);
  const [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255];
  return 0.299 * r + 0.587 * g + 0.114 * b > 150 ? "#141414" : "#FFFFFF";
}

function roundRect(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  ctx.beginPath();
  ctx.roundRect(x, y, w, h, r);
  ctx.fill();
}

export interface CueStyle {
  /** `#RRGGBB` box behind the spoken word; null = no word tracker. */
  highlight: string | null;
}

/**
 * Paints one cue (1–2 lines) into `ctx`, bottom-aligned in a band of `band` size whose top
 * is at `top`. `active` = index of the spoken word over all lines of the cue, or null.
 */
export function paintCue(
  ctx: CanvasRenderingContext2D,
  lines: Word[][],
  active: number | null,
  style: CueStyle,
  band: Band,
  top = 0,
) {
  let px = band.fontPx;
  ctx.font = font(px);
  const widest = Math.max(...lines.map((l) => ctx.measureText(lineText(l)).width), 1);
  const limit = band.width * MAX_TEXT_WIDTH;
  if (widest > limit) {
    px = Math.floor((px * limit) / widest);
    ctx.font = font(px);
  }

  const lineH = px * LINE_HEIGHT;
  const padBottom = px * 0.4;
  const stroke = Math.max(2, Math.round(px * 0.09)); // ~4 px at 44 px
  const boxPadX = px * 0.16;
  const boxPadY = px * 0.1;
  /** Minimum box height (lines without tall stacks). */
  const boxH = px * 1.3;
  ctx.textAlign = "left";
  ctx.textBaseline = "middle";
  ctx.lineJoin = "round";
  ctx.miterLimit = 2;

  let wordIndex = 0;
  lines.forEach((words, li) => {
    const fromBottom = lines.length - 1 - li;
    const y = top + band.height - padBottom - lineH / 2 - fromBottom * lineH;
    const text = lineText(words);
    const x0 = (band.width - ctx.measureText(text).width) / 2;

    // Word positions from prefix widths (keeps the line's natural spacing).
    const spans = words.map((w, i) => {
      const before = lineText(words.slice(0, i)) + (i > 0 && w.spaced ? " " : "");
      const start = x0 + ctx.measureText(before).width;
      return { word: w, start, end: start + ctx.measureText(w.text).width, index: wordIndex + i };
    });
    wordIndex += words.length;
    const hit = style.highlight !== null && active !== null ? spans.find((s) => s.index === active) : undefined;

    if (hit && style.highlight) {
      // Box height from the whole line's real glyph extent (vowels above, subscripts below),
      // so it is the same for every word of the line and always wraps the Khmer stacks.
      const m = ctx.measureText(text);
      const up = Math.max(m.actualBoundingBoxAscent, boxH / 2 - boxPadY);
      const down = Math.max(m.actualBoundingBoxDescent, boxH / 2 - boxPadY);
      ctx.save();
      ctx.shadowColor = "rgba(0, 0, 0, 0.35)";
      ctx.shadowBlur = Math.round(px * 0.2);
      ctx.fillStyle = style.highlight;
      roundRect(
        ctx,
        hit.start - boxPadX,
        y - up - boxPadY,
        hit.end - hit.start + boxPadX * 2,
        up + down + boxPadY * 2,
        px * 0.22,
      );
      ctx.restore();
    }

    // Outline + soft shadow for every word not on the box, then white fill.
    ctx.save();
    ctx.shadowColor = "rgba(0, 0, 0, 0.55)";
    ctx.shadowBlur = Math.round(px * 0.18);
    ctx.shadowOffsetY = Math.round(px * 0.05);
    ctx.strokeStyle = "rgba(0, 0, 0, 0.9)";
    ctx.lineWidth = stroke * 2;
    for (const s of spans) if (s !== hit) ctx.strokeText(s.word.text, s.start, y);
    ctx.restore();
    for (const s of spans) {
      ctx.fillStyle = s === hit && style.highlight ? textOn(style.highlight) : "#FFFFFF";
      ctx.fillText(s.word.text, s.start, y);
    }
  });
}

/** PNG bytes of one subtitle state (transparent band). */
export async function drawCuePng(lines: Word[][], active: number | null, style: CueStyle, band: Band): Promise<Uint8Array> {
  const canvas = document.createElement("canvas");
  canvas.width = band.width;
  canvas.height = band.height;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("Canvas is not available.");
  paintCue(ctx, lines, active, style, band);
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
