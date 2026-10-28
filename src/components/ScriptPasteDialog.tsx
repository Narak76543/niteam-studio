import { useEffect, useMemo, useRef, useState } from "react";

import { Button } from "./Button";
import { Notice } from "./Notice";
import { useI18n } from "../i18n/context";
import { formatParsedScript, parseScriptDraft, type ParsedScene } from "../lib/scriptImport";
import type { Scene } from "../lib/types";

interface Props {
  scenes: Pick<Scene, "order" | "narration" | "spoken_text">[];
  onApply: (scenes: ParsedScene[]) => Promise<void>;
  onCancel: () => void;
}

/** Paste a corrected script from ChatGPT/Gemini back onto the existing scenes. */
export function ScriptPasteDialog({ scenes, onApply, onCancel }: Props) {
  const { t } = useI18n();
  const [text, setText] = useState("");
  const [applying, setApplying] = useState(false);
  const ref = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    ref.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !applying) onCancel();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel, applying]);

  const parsed = useMemo(() => parseScriptDraft(text), [text]);
  const current = useMemo(() => new Map(scenes.map((s) => [s.order, s])), [scenes]);
  const pasted = useMemo(() => new Map(parsed.scenes.map((s) => [s.order, s])), [parsed.scenes]);
  const missing = scenes.filter((s) => !pasted.has(s.order)).map((s) => s.order);
  const unknown = parsed.scenes.filter((s) => !current.has(s.order)).map((s) => s.order);
  const changed = parsed.scenes
    .filter((p) => {
      const s = current.get(p.order);
      return s && (p.narration !== s.narration || p.spoken_text !== s.spoken_text);
    })
    .map((p) => p.order);

  const problems: string[] = [];
  if (text.trim() && parsed.scenes.length === 0) problems.push(t("script.pasteParseFailed"));
  if (parsed.scenes.length > 0 && missing.length > 0)
    problems.push(t("script.pasteMissing", { list: missing.join(", ") }));
  if (unknown.length > 0) problems.push(t("script.pasteUnknown", { list: unknown.join(", ") }));
  const fieldErrors = new Set(
    parsed.errors.map((e) =>
      e.kind === "duplicate" ? t("script.pasteDuplicate", { n: e.order }) : t("script.pasteMissingFields", { n: e.order }),
    ),
  );
  problems.push(...fieldErrors);

  const canApply = problems.length === 0 && changed.length > 0 && !applying;

  const apply = async () => {
    if (!canApply) return;
    setApplying(true);
    try {
      await onApply(parsed.scenes);
    } catch {
      // The page shows the error and reloads the project.
    } finally {
      setApplying(false);
    }
  };

  return (
    <div className="backdrop" onMouseDown={(e) => e.target === e.currentTarget && !applying && onCancel()}>
      <form
        className="dialog dialog--wide"
        role="dialog"
        aria-modal="true"
        aria-labelledby="paste-dialog-title"
        onSubmit={(e) => {
          e.preventDefault();
          void apply();
        }}
      >
        <h2 id="paste-dialog-title" className="t-h2">
          {t("script.pasteTitle")}
        </h2>
        <p className="t-small">{t("script.pasteHint")}</p>
        <label className="field">
          <span className="field__label">{t("script.pasteLabel")}</span>
          <textarea
            ref={ref}
            className="textarea script-paste__input"
            value={text}
            onChange={(e) => setText(e.target.value)}
            placeholder={t("script.pastePlaceholder")}
            spellCheck={false}
          />
        </label>

        {text.trim() !== "" && (
          <div className="script-paste__status">
            {problems.length > 0 ? (
              <Notice tone="danger">
                <ul className="script-paste__list">
                  {problems.map((p) => (
                    <li key={p}>{p}</li>
                  ))}
                </ul>
              </Notice>
            ) : changed.length === 0 && parsed.scenes.length > 0 ? (
              <Notice tone="ok">{t("script.pasteNoChanges")}</Notice>
            ) : (
              <div className="t-small">
                <p>
                  {t("script.pasteFound", {
                    found: parsed.scenes.length,
                    total: scenes.length,
                    changed: changed.length,
                  })}
                </p>
                {changed.length > 0 && (
                  <p className="t-muted">{t("script.pasteChanged", { list: changed.join(", ") })}</p>
                )}
              </div>
            )}
          </div>
        )}

        {problems.length === 0 && parsed.scenes.length > 0 && (
          <details className="script-paste__preview">
            <summary className="t-small">{t("script.pastePreview")}</summary>
            <pre className="credits t-micro">{formatParsedScript(parsed.scenes)}</pre>
          </details>
        )}

        <div className="dialog__actions">
          <Button onClick={onCancel} disabled={applying}>
            {t("common.cancel")}
          </Button>
          <Button type="submit" variant="primary" disabled={!canApply}>
            {applying ? t("script.pasteApplying") : t("script.pasteApply")}
          </Button>
        </div>
      </form>
    </div>
  );
}
