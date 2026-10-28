// Parses a corrected script pasted back from an external model (see scriptReview.ts).

export interface ParsedScene {
  order: number;
  narration: string;
  spoken_text: string;
}

export type ScriptParseError =
  | { kind: "duplicate"; order: number }
  | { kind: "missingFields"; order: number };

export interface ScriptParseResult {
  scenes: ParsedScene[];
  errors: ScriptParseError[];
}

const KHMER_DIGITS = "០១២៣៤៥៦៧៨៩";
const SCENE = /^(?:scene|ឈុត)(?:\s*(?:ទី|#|no\.?))?\s*[:\-–—]?\s*([0-9០-៩]+)/i;
const CHAPTER = /^(?:chapter|ជំពូក)(?:\s*(?:ទី|#|no\.?))?\s*[0-9០-៩]+/i;
const LABELS = "narration|spoken(?:\\s+text)?|voice[ -]?over|tts|fact(?:[ -]?check)?|អត្ថបទនិទាន|អត្ថបទសម្រាប់អាន|សូមពិនិត្យការពិតនេះ|ការពិត";
const FIELD = new RegExp(`^(${LABELS})\\s*[:：៖]\\s*(.*)$`, "i");
const LABEL_ONLY = new RegExp(`^(${LABELS})\\s*$`, "i");

function asciiNumber(text: string): number {
  const digits = text.replace(/[០-៩]/g, (d) => String(KHMER_DIGITS.indexOf(d)));
  return Number(digits);
}

function fieldOf(label: string): "narration" | "spoken" | "fact" {
  const l = label.toLowerCase();
  if (l.startsWith("narration") || l.startsWith("អត្ថបទនិទាន")) return "narration";
  if (l.startsWith("spoken") || l.startsWith("voice") || l === "tts" || l.startsWith("អត្ថបទសម្រាប់អាន"))
    return "spoken";
  return "fact";
}

/** Tolerates Markdown headings, bold labels and bullets around the plain-text format. */
function clean(line: string): string {
  const trimmed = line.trim();
  if (trimmed.startsWith("```")) return "";
  const marked = trimmed
    .replace(/^#{1,6}\s*/, "")
    .replace(/^>\s*/, "")
    .replace(/\*\*/g, "")
    .trim();
  const unbulleted = marked.replace(/^[-*+•]\s*/, "");
  if (unbulleted !== marked && (SCENE.test(unbulleted) || CHAPTER.test(unbulleted) || FIELD.test(unbulleted) || LABEL_ONLY.test(unbulleted)))
    return unbulleted;
  return marked;
}

/** The app stores one plain-text line per field. */
function tidy(text: string): string {
  return text.replace(/\s+/g, " ").trim();
}

function append(text: string, more: string): string {
  return [text, more].filter(Boolean).join(" ");
}

/** Formats parsed scenes exactly like the script the app's paste parser accepts. */
export function formatParsedScript(scenes: ParsedScene[]): string {
  return scenes
    .map((s) => `Scene ${s.order}\nNarration: ${s.narration}\nSpoken: ${s.spoken_text}`)
    .join("\n\n");
}

/**
 * Reads `Scene N` / `Narration:` / `Spoken:` blocks, also with Khmer labels and digits. Chapter
 * headings, title lines and fact notes are ignored; wrapped lines are joined and each field is
 * reduced to one trimmed line. `errors` blocks applying the paste.
 */
export function parseScriptDraft(text: string): ScriptParseResult {
  const scenes: ParsedScene[] = [];
  const errors: ScriptParseError[] = [];
  const seen = new Set<number>();
  let order: number | null = null;
  let narration = "";
  let spoken = "";
  let field: "narration" | "spoken" | null = null;

  const finish = () => {
    if (order === null) return;
    const n = tidy(narration);
    const s = tidy(spoken);
    if (!n || !s) errors.push({ kind: "missingFields", order });
    scenes.push({ order, narration: n, spoken_text: s });
    order = null;
  };

  for (const raw of text.split(/\r?\n/)) {
    const line = clean(raw);
    if (!line) {
      field = null;
      continue;
    }
    const scene = line.match(SCENE);
    if (scene) {
      finish();
      order = asciiNumber(scene[1] ?? "");
      if (seen.has(order)) errors.push({ kind: "duplicate", order });
      seen.add(order);
      narration = "";
      spoken = "";
      field = null;
      continue;
    }
    if (CHAPTER.test(line)) {
      field = null;
      continue;
    }
    const labeled = line.match(FIELD);
    if (labeled && order !== null) {
      const kind = fieldOf(labeled[1] ?? "");
      const value = labeled[2] ?? "";
      if (kind === "fact") {
        field = null;
      } else {
        if (kind === "narration") narration = append(narration, value);
        else spoken = append(spoken, value);
        field = kind;
      }
      continue;
    }
    const labelOnly = line.match(LABEL_ONLY);
    if (labelOnly && order !== null) {
      const kind = fieldOf(labelOnly[1] ?? "");
      field = kind === "fact" ? null : kind;
      continue;
    }
    if (order !== null && field) {
      if (field === "narration") narration = append(narration, line);
      else spoken = append(spoken, line);
    }
  }
  finish();

  return { scenes, errors };
}
