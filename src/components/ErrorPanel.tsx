import { RotateCcw } from "lucide-react";

import { Button } from "./Button";
import { Notice } from "./Notice";
import { useI18n } from "../i18n/context";
import { ERROR_KINDS, type ErrorKind } from "../lib/types";

function toKind(kind: string | null | undefined): ErrorKind {
  return ERROR_KINDS.find((k) => k === kind) ?? "other";
}

interface Props {
  kind: string | null | undefined;
  /** Raw error text, shown in a collapsible "Details" section. */
  details?: string | null;
  onRetry?: () => void;
}

/** Friendly, translated error with an optional "Try again" button and raw details. */
export function ErrorPanel({ kind, details, onRetry }: Props) {
  const { t } = useI18n();
  const k = toKind(kind);
  return (
    <Notice
      tone="danger"
      actions={
        onRetry && (
          <Button variant="secondary" icon={<RotateCcw size={16} strokeWidth={1.5} />} onClick={onRetry}>
            {t("common.tryAgain")}
          </Button>
        )
      }
    >
      <div className="error-panel">
        <strong>{t(`error.${k}.title`)}</strong>
        <span className="t-small">{t(`error.${k}.body`)}</span>
        {details && (
          <details className="details">
            <summary className="t-small">{t("common.details")}</summary>
            <pre lang="en">{details}</pre>
          </details>
        )}
      </div>
    </Notice>
  );
}
