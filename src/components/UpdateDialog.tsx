import { Download } from "lucide-react";
import { useEffect, useRef } from "react";

import { Button } from "./Button";
import { Notice } from "./Notice";
import { ProgressBar } from "./ProgressBar";
import { useI18n } from "../i18n/context";
import type { Updater } from "../lib/updater";

interface Props {
  updater: Updater;
  currentVersion: string;
}

/** "Version x is available" → release notes → download progress → installer (docs/12-updates.md). */
export function UpdateDialog({ updater, currentVersion }: Props) {
  const { t } = useI18n();
  const { state, update, install, closeDialog } = updater;
  const laterRef = useRef<HTMLButtonElement>(null);
  const working = state.kind === "downloading" || state.kind === "installing";

  useEffect(() => {
    laterRef.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !working) closeDialog();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [closeDialog, working]);

  if (!update) return null;
  const notes = update.body?.trim();
  const percent =
    state.kind === "downloading" && state.total ? Math.round((state.done / state.total) * 100) : null;

  return (
    <div className="backdrop" onMouseDown={(e) => e.target === e.currentTarget && !working && closeDialog()}>
      <div className="dialog dialog--update" role="dialog" aria-modal="true" aria-labelledby="update-title">
        <div className="update__head">
          <span className="update__icon" aria-hidden="true">
            <Download size={18} strokeWidth={1.75} />
          </span>
          <div>
            <h2 id="update-title" className="t-h2">
              {t("update.title", { version: update.version })}
            </h2>
            <p className="t-small">{t("update.current", { version: currentVersion || "—" })}</p>
          </div>
        </div>

        <div className="update__notes">
          <span className="t-micro t-muted">{t("update.notes")}</span>
          <div className="update__notes-body t-small">{notes || t("update.noNotes")}</div>
        </div>

        {state.kind === "downloading" && (
          <div className="update__progress">
            <ProgressBar
              value={state.total ? state.done / state.total : undefined}
              label={t("update.downloadingUnknown")}
            />
            <span className="t-small">
              {percent === null ? t("update.downloadingUnknown") : t("update.downloading", { percent })}
            </span>
          </div>
        )}
        {state.kind === "installing" && <Notice>{t("update.installing")}</Notice>}
        {state.kind === "installFailed" && (
          <Notice tone="danger">{t("update.installError", { error: state.message })}</Notice>
        )}
        {!working && <p className="t-small t-muted">{t("update.warnJobs")}</p>}

        <div className="dialog__actions">
          <Button ref={laterRef} disabled={working} onClick={closeDialog}>
            {t("update.later")}
          </Button>
          <Button
            variant="primary"
            icon={<Download size={16} strokeWidth={1.75} />}
            disabled={working}
            onClick={() => void install()}
          >
            {t("update.install")}
          </Button>
        </div>
      </div>
    </div>
  );
}
