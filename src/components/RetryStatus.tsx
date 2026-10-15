import { LoaderCircle } from "lucide-react";
import { useEffect, useState } from "react";

import { useI18n } from "../i18n/context";
import type { GeminiRetry } from "../lib/types";

interface Props {
  retry: GeminiRetry;
  /** `Date.now()` when the notice arrived; the countdown runs from here. */
  receivedAt: number;
}

/** "Google is busy, retrying (attempt 2/5) in 4 s…" with a live countdown. */
export function RetryStatus({ retry, receivedAt }: Props) {
  const { t } = useI18n();
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, []);

  const seconds = Math.max(0, Math.ceil((receivedAt + retry.delay_ms - now) / 1000));
  const vars = { attempt: retry.attempt, max: retry.max_attempts, seconds, model: retry.model };
  const text =
    retry.reason === "fallback"
      ? t("retry.fallback", vars)
      : seconds === 0
        ? t("retry.now", vars)
        : t(`retry.${retry.reason}`, vars);

  return (
    <div className="notice notice--warn" role="status" aria-live="polite">
      <span className="notice__icon">
        <LoaderCircle size={16} strokeWidth={1.5} className="spin" aria-hidden="true" />
      </span>
      <div className="notice__body">{text}</div>
    </div>
  );
}
