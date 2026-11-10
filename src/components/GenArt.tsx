import { ImageIcon, PenLine, Sparkles } from "lucide-react";

export type GenKind = "script" | "voice" | "image";

/**
 * Small looping illustration for a running job (docs/06-ui-design.md §5 "Generation"):
 * script = lines being written by a pen, voice = sound-wave bars, image = a frame with a
 * shimmer sweep. Pure CSS (transform/opacity), static under prefers-reduced-motion.
 */
export function GenArt({ kind }: { kind: GenKind }) {
  if (kind === "voice") {
    return (
      <span className="gen-art gen-art--voice" aria-hidden="true">
        <i />
        <i />
        <i />
        <i />
        <i />
      </span>
    );
  }
  if (kind === "image") {
    return (
      <span className="gen-art gen-art--image" aria-hidden="true">
        <ImageIcon size={18} strokeWidth={1.5} />
        <span className="gen-art__sweep" />
        <Sparkles className="gen-art__spark" size={11} strokeWidth={2} />
      </span>
    );
  }
  return (
    <span className="gen-art gen-art--script" aria-hidden="true">
      <i />
      <i />
      <i />
      <PenLine className="gen-art__pen" size={13} strokeWidth={2} />
    </span>
  );
}
