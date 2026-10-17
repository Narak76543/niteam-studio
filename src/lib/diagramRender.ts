// Draws a Mermaid diagram as a frame-size PNG for the video (docs/11-real-world-visuals.md §10).
// Mermaid runs in the WebView (no Node, no headless browser). Labels are plain SVG text
// (htmlLabels off: an SVG with <foreignObject> can't be copied to a canvas) and Kantumruy Pro
// (both Khmer and Latin files, every weight) is embedded in the SVG so the rasterised picture
// shapes Khmer exactly like the app. Mermaid is loaded only when a diagram is drawn.

import khmerFontUrl from "../assets/fonts/kantumruy-pro-khmer-wght-normal.woff2?url";
import latinFontUrl from "../assets/fonts/kantumruy-pro-latin-wght-normal.woff2?url";

/** The video's diagram look (independent of the app theme). */
const PALETTE = {
  background: "#F7F4EF",
  node: "#FFFFFF",
  border: "#E8553E",
  text: "#141414",
  line: "#5B5B5B",
  cluster: "#EFE9E1",
  /** Timeline sections (cScale0…). */
  sections: ["#FDE3DC", "#E1EEF9", "#E3F3E8", "#FCF0D9", "#ECE6F7", "#DDF1F1", "#F9E1EC", "#EFEDE8"],
} as const;
const FONT = '"Kantumruy Pro", sans-serif';
/** Space around the diagram (share of the frame height). */
const MARGIN = 0.06;
/** A tiny diagram is not blown up beyond this. */
const MAX_SCALE = 2.6;

type Mermaid = (typeof import("mermaid"))["default"];
let mermaidReady: Promise<Mermaid> | null = null;
let fontCss: Promise<string> | null = null;
let counter = 0;

async function toBase64(url: string): Promise<string> {
  const bytes = new Uint8Array(await (await fetch(url)).arrayBuffer());
  let s = "";
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(s);
}

/** @font-face rules with the fonts inlined (an SVG drawn as an image can't load files). */
function embeddedFonts(): Promise<string> {
  fontCss ??= Promise.all([toBase64(khmerFontUrl), toBase64(latinFontUrl)]).then(
    ([km, latin]) =>
      `@font-face{font-family:"Kantumruy Pro";font-weight:100 700;src:url(data:font/woff2;base64,${km}) format("woff2");unicode-range:U+1780-17FF,U+19E0-19FF,U+200C-200D,U+25CC;}` +
      `@font-face{font-family:"Kantumruy Pro";font-weight:100 700;src:url(data:font/woff2;base64,${latin}) format("woff2");}`,
  );
  return fontCss;
}

function loadMermaid(): Promise<Mermaid> {
  mermaidReady ??= (async () => {
    // Mermaid measures labels in the page: the fonts must be ready first.
    await Promise.all([
      document.fonts.load(`400 20px ${FONT}`, "ស្ថាបត្យកម្ម"),
      document.fonts.load(`700 20px ${FONT}`, "ស្ថាបត្យកម្ម"),
      document.fonts.load(`400 20px ${FONT}`, "Server"),
      document.fonts.load(`700 20px ${FONT}`, "Server"),
    ]);
    const mermaid = (await import("mermaid")).default;
    const sections = Object.fromEntries(PALETTE.sections.flatMap((c, i) => [[`cScale${i}`, c], [`cScaleLabel${i}`, PALETTE.text]]));
    mermaid.initialize({
      startOnLoad: false,
      securityLevel: "strict",
      theme: "base",
      htmlLabels: false,
      flowchart: { htmlLabels: false, curve: "basis" },
      fontFamily: FONT,
      themeVariables: {
        fontFamily: FONT,
        fontSize: "20px",
        background: PALETTE.background,
        primaryColor: PALETTE.node,
        primaryBorderColor: PALETTE.border,
        primaryTextColor: PALETTE.text,
        secondaryColor: PALETTE.cluster,
        tertiaryColor: PALETTE.cluster,
        lineColor: PALETTE.line,
        textColor: PALETTE.text,
        actorBkg: PALETTE.node,
        actorBorder: PALETTE.border,
        actorTextColor: PALETTE.text,
        signalColor: PALETTE.line,
        signalTextColor: PALETTE.text,
        ...sections,
      },
    });
    return mermaid;
  })();
  return mermaidReady;
}

/** Throws Mermaid's parse error (shown to the user, sent to "Fix with Gemini"). */
export async function checkDiagram(source: string): Promise<void> {
  const mermaid = await loadMermaid();
  await mermaid.parse(source);
}

/**
 * The diagram centred above the subtitle band on the video background, as PNG bytes.
 * `bandTop` = top of the subtitle band in frame pixels.
 */
export async function drawDiagram(source: string, width: number, height: number, bandTop: number): Promise<Uint8Array<ArrayBuffer>> {
  const mermaid = await loadMermaid();
  await mermaid.parse(source);
  counter += 1;
  const { svg } = await mermaid.render(`nitean-diagram-${counter}`, source);
  const doc = new DOMParser().parseFromString(svg, "image/svg+xml");
  const root = doc.documentElement;
  const style = doc.createElementNS("http://www.w3.org/2000/svg", "style");
  style.textContent = await embeddedFonts();
  root.insertBefore(style, root.firstChild);
  const vb = (root as unknown as SVGSVGElement).viewBox.baseVal;
  const vw = vb.width || 800;
  const vh = vb.height || 450;
  root.setAttribute("width", String(vw));
  root.setAttribute("height", String(vh));
  root.removeAttribute("style");

  const url = URL.createObjectURL(new Blob([new XMLSerializer().serializeToString(root)], { type: "image/svg+xml" }));
  try {
    const img = new Image();
    await new Promise<void>((ok, fail) => {
      img.onload = () => ok();
      img.onerror = () => fail(new Error("the diagram could not be drawn"));
      img.src = url;
    });
    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("no canvas");
    ctx.fillStyle = PALETTE.background;
    ctx.fillRect(0, 0, width, height);
    const margin = height * MARGIN;
    const areaW = width - 2 * margin;
    const areaH = bandTop - 2 * margin;
    const scale = Math.min(areaW / vw, areaH / vh, MAX_SCALE);
    const dw = vw * scale;
    const dh = vh * scale;
    ctx.drawImage(img, (width - dw) / 2, margin + (areaH - dh) / 2, dw, dh);
    const blob = await new Promise<Blob | null>((ok) => canvas.toBlob(ok, "image/png"));
    if (!blob) throw new Error("the diagram could not be saved");
    return new Uint8Array(await blob.arrayBuffer());
  } finally {
    URL.revokeObjectURL(url);
  }
}
