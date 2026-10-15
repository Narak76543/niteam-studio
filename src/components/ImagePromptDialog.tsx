import { useEffect, useRef, useState } from "react";

import { Button } from "./Button";
import { useI18n } from "../i18n/context";

interface Props {
  title: string;
  initial: string;
  onSave: (prompt: string) => void;
  onCancel: () => void;
}

/** Edit one scene's image prompt (Scenes page); saving marks only that image outdated. */
export function ImagePromptDialog({ title, initial, onSave, onCancel }: Props) {
  const { t } = useI18n();
  const [value, setValue] = useState(initial);
  const ref = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    ref.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onCancel();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel]);

  const changed = value.trim() !== initial.trim() && value.trim() !== "";

  return (
    <div className="backdrop" onMouseDown={(e) => e.target === e.currentTarget && onCancel()}>
      <form
        className="dialog dialog--wide"
        role="dialog"
        aria-modal="true"
        aria-labelledby="prompt-dialog-title"
        onSubmit={(e) => {
          e.preventDefault();
          if (changed) onSave(value);
        }}
      >
        <h2 id="prompt-dialog-title" className="t-h2">
          {title}
        </h2>
        <label className="field">
          <span className="field__label">{t("script.imagePrompt")}</span>
          <textarea ref={ref} className="textarea" lang="en" value={value} onChange={(e) => setValue(e.target.value)} />
          <span className="t-small">{t("script.imagePromptHint")}</span>
        </label>
        <div className="dialog__actions">
          <Button onClick={onCancel}>{t("common.cancel")}</Button>
          <Button type="submit" variant="primary" disabled={!changed}>
            {t("common.save")}
          </Button>
        </div>
      </form>
    </div>
  );
}
