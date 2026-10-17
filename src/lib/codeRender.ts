// Code / terminal scenes (docs/11-real-world-visuals.md §11): a dark editor or terminal card
// drawn on a canvas, coloured with Shiki (JavaScript regex engine, no WASM; loaded only when a
// code scene is drawn). JetBrains Mono, with Kantumruy Pro for Khmer comments and strings.
// Typed-out scenes also get one picture per revealed line (with a cursor).

import type { ThemedToken } from "shiki/core";

import type { CodeSpec } from "./types";

export interface CodePictures {
  /** The finished card (the clip's last frame, thumbnail, preview). */
  still: Uint8Array<ArrayBuffer>;
  /** Typed out: lines 1…k for k = 1…n; empty otherwise. */
  frames: Uint8Array<ArrayBuffer>[];
}

const C = {
  background: "#F7F4EF",
  card: "#1B1D23",
  header: "#252830",
  text: "#D6DAE0",
  muted: "#7D8590",
  prompt: "#3FB950",
  cursor: "#E8553E",
  dots: ["#FF5F57", "#FEBC2E", "#28C840"],
} as const;
const MONO = '"JetBrains Mono Variable", "Kantumruy Pro", Consolas, monospace';
const UI = '"Kantumruy Pro", sans-serif';
const MAX_LINES = 18;
const MAX_FONT = 38;
const MIN_FONT = 18;
const THEME = "github-dark";

/** Languages we load (`gemini::script::CODE_LANGUAGES` minus "text"). */
const LANGS = {
  bash: () => import("@shikijs/langs/bash"),
  powershell: () => import("@shikijs/langs/powershell"),
  python: () => import("@shikijs/langs/python"),
  javascript: () => import("@shikijs/langs/javascript"),
  typescript: () => import("@shikijs/langs/typescript"),
  json: () => import("@shikijs/langs/json"),
  yaml: () => import("@shikijs/langs/yaml"),
  rust: () => import("@shikijs/langs/rust"),
  go: () => import("@shikijs/langs/go"),
  java: () => import("@shikijs/langs/java"),
  c: () => import("@shikijs/langs/c"),
  cpp: () => import("@shikijs/langs/cpp"),
  sql: () => import("@shikijs/langs/sql"),
  html: () => import("@shikijs/langs/html"),
  css: () => import("@shikijs/langs/css"),
} as const;
type Lang = keyof typeof LANGS;

function isLang(l: string): l is Lang {
  return l in LANGS;
}

type Highlighter = Awaited<ReturnType<(typeof import("shiki/core"))["createHighlighterCore"]>>;
let highlighter: Promise<Highlighter> | null = null;
const loaded = new Set<string>();

async function tokensFor(code: string, lang: string): Promise<ThemedToken[][]> {
  if (!isLang(lang)) return code.split("\n").map((l) => [{ content: l, offset: 0, color: C.text }]);
  highlighter ??= (async () => {
    const [{ createHighlighterCore }, { createJavaScriptRegexEngine }] = await Promise.all([
      import("shiki/core"),
      import("shiki/engine/javascript"),
    ]);
    return createHighlighterCore({ themes: [import("@shikijs/themes/github-dark")], langs: [], engine: createJavaScriptRegexEngine() });
  })();
  const hl = await highlighter;
  if (!loaded.has(lang)) {
    await hl.loadLanguage(LANGS[lang]());
    loaded.add(lang);
  }
  return hl.codeToTokensBase(code, { lang, theme: THEME });
}

/** A terminal line: "$ cmd" / "PS> cmd" is a command, anything else is output. */
function promptOf(line: string): string | null {
  const m = /^(\$ |PS> |> )/.exec(line);
  return m ? (m[1] ?? null) : null;
}

interface Line {
  tokens: { text: string; color: string }[];
  /** Terminal prompt drawn in green before the command. */
  prompt: string | null;
}

async function layoutLines(spec: CodeSpec): Promise<Line[]> {
  let raw = spec.text.replace(/\t/g, "  ").replace(/\s+$/, "").split("\n");
  if (raw.length > MAX_LINES) raw = [...raw.slice(0, MAX_LINES - 1), "…"];
  if (!spec.terminal) {
    const tokens = await tokensFor(raw.join("\n"), spec.language);
    return tokens.map((line) => ({ prompt: null, tokens: line.map((t) => ({ text: t.content, color: t.color ?? C.text })) }));
  }
  // Terminal: only commands are highlighted (shell grammar); output stays plain.
  const shell: Lang = spec.language === "powershell" ? "powershell" : "bash";
  const out: Line[] = [];
  for (const line of raw) {
    const prompt = promptOf(line);
    if (prompt === null) {
      out.push({ prompt: null, tokens: [{ text: line, color: C.muted }] });
      continue;
    }
    const [tokens] = await tokensFor(line.slice(prompt.length), shell);
    out.push({ prompt, tokens: (tokens ?? []).map((t) => ({ text: t.content, color: t.color ?? C.text })) });
  }
  return out;
}

function roundRect(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  ctx.beginPath();
  ctx.roundRect(x, y, w, h, r);
}

async function toPng(canvas: HTMLCanvasElement): Promise<Uint8Array<ArrayBuffer>> {
  const blob = await new Promise<Blob | null>((ok) => canvas.toBlob(ok, "image/png"));
  if (!blob) throw new Error("the code picture could not be saved");
  return new Uint8Array(await blob.arrayBuffer());
}

/** Draws the card with the first `shown` lines (all when undefined); `cursor` after the last one. */
function drawCard(
  canvas: HTMLCanvasElement,
  spec: CodeSpec,
  lines: Line[],
  shown: number,
  cursor: boolean,
  bandTop: number,
  fontPx: number,
) {
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("no canvas");
  const W = canvas.width;
  const H = canvas.height;
  const margin = Math.round(H * 0.06);
  ctx.fillStyle = C.background;
  ctx.fillRect(0, 0, W, H);

  const header = Math.round(H * 0.052);
  const pad = Math.round(fontPx * 1.1);
  const lineH = Math.round(fontPx * 1.5);
  const gutter = spec.terminal ? 0 : Math.round(fontPx * 2.6);
  const cardH = Math.min(bandTop - 2 * margin, header + 2 * pad + lines.length * lineH);
  const cardW = W - 2 * Math.round(W * 0.05);
  const x0 = (W - cardW) / 2;
  const y0 = margin + (bandTop - 2 * margin - cardH) / 2;

  ctx.save();
  ctx.shadowColor = "rgba(0,0,0,0.25)";
  ctx.shadowBlur = 40;
  ctx.shadowOffsetY = 12;
  roundRect(ctx, x0, y0, cardW, cardH, 22);
  ctx.fillStyle = C.card;
  ctx.fill();
  ctx.restore();

  // Title bar: three dots + language / "Terminal".
  ctx.save();
  roundRect(ctx, x0, y0, cardW, cardH, 22);
  ctx.clip();
  ctx.fillStyle = C.header;
  ctx.fillRect(x0, y0, cardW, header);
  C.dots.forEach((c, i) => {
    ctx.beginPath();
    ctx.arc(x0 + header * 0.6 + i * header * 0.45, y0 + header / 2, header * 0.14, 0, Math.PI * 2);
    ctx.fillStyle = c;
    ctx.fill();
  });
  ctx.fillStyle = C.muted;
  ctx.font = `500 ${Math.round(header * 0.38)}px ${UI}`;
  ctx.textBaseline = "middle";
  ctx.textAlign = "center";
  ctx.fillText(spec.terminal ? "Terminal" : spec.language, x0 + cardW / 2, y0 + header / 2);

  ctx.textAlign = "left";
  ctx.textBaseline = "alphabetic";
  ctx.font = `400 ${fontPx}px ${MONO}`;
  const textX = x0 + pad + gutter;
  const maxX = x0 + cardW - pad;
  let endX = textX;
  let endY = y0 + header + pad + fontPx;
  lines.slice(0, shown).forEach((line, i) => {
    const y = y0 + header + pad + fontPx + i * lineH;
    if (!spec.terminal) {
      ctx.fillStyle = C.muted;
      ctx.textAlign = "right";
      ctx.fillText(String(i + 1), x0 + pad + gutter - fontPx * 0.9, y);
      ctx.textAlign = "left";
    }
    let x = textX;
    if (line.prompt) {
      ctx.fillStyle = C.prompt;
      ctx.fillText(line.prompt, x, y);
      x += ctx.measureText(line.prompt).width;
    }
    for (const t of line.tokens) {
      if (x >= maxX) break;
      ctx.fillStyle = t.color;
      ctx.fillText(t.text, x, y);
      x += ctx.measureText(t.text).width;
    }
    endX = Math.min(x, maxX);
    endY = y;
  });
  if (cursor) {
    ctx.fillStyle = C.cursor;
    ctx.fillRect(endX + fontPx * 0.15, endY - fontPx * 0.85, fontPx * 0.55, fontPx * 1.05);
  }
  ctx.restore();
}

/** Font size that fits the longest line and every line above the subtitle band. */
function fitFont(ctx: CanvasRenderingContext2D, spec: CodeSpec, lines: Line[], W: number, H: number, bandTop: number): number {
  const margin = Math.round(H * 0.06);
  const header = Math.round(H * 0.052);
  const cardW = W - 2 * Math.round(W * 0.05);
  let size = MAX_FONT;
  for (; size > MIN_FONT; size -= 1) {
    ctx.font = `400 ${size}px ${MONO}`;
    const pad = size * 1.1;
    const gutter = spec.terminal ? 0 : size * 2.6;
    const widest = Math.max(...lines.map((l) => ctx.measureText((l.prompt ?? "") + l.tokens.map((t) => t.text).join("")).width), 0);
    const tall = header + 2 * pad + lines.length * size * 1.5;
    if (widest <= cardW - 2 * pad - gutter && tall <= bandTop - 2 * margin) break;
  }
  return size;
}

export async function drawCode(spec: CodeSpec, width: number, height: number, bandTop: number): Promise<CodePictures> {
  await Promise.all([document.fonts.load(`400 20px ${MONO}`, "const x = 1;"), document.fonts.load(`400 20px ${MONO}`, "ក")]);
  const lines = await layoutLines(spec);
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("no canvas");
  const fontPx = fitFont(ctx, spec, lines, width, height, bandTop);
  drawCard(canvas, spec, lines, lines.length, false, bandTop, fontPx);
  const still = await toPng(canvas);
  const frames: Uint8Array<ArrayBuffer>[] = [];
  if (spec.type_out && lines.length > 1) {
    for (let k = 1; k <= lines.length; k += 1) {
      drawCard(canvas, spec, lines, k, k < lines.length, bandTop, fontPx);
      frames.push(await toPng(canvas));
    }
  }
  return { still, frames };
}
