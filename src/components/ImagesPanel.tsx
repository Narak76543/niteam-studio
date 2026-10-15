import { Play } from "lucide-react";
import { useEffect, useState } from "react";

import { Button } from "./Button";
import { Notice } from "./Notice";
import { useI18n } from "../i18n/context";
import { formatTime } from "../lib/format";
import type { ImageEstimate, Project } from "../lib/types";

const TICK_MS = 30_000;

interface Props {
  project: Project;
  estimate: ImageEstimate | null;
  running: boolean;
  onScenesPerImage: (n: 1 | 2) => void;
  onResume: () => void;
  /** Benchmark result for local images (Settings → Local images), for the time estimate. */
  secondsPerImage: number | null;
  onBattery: boolean;
}

/** True once the free allowance has reset; re-evaluated every 30 s. */
function useResetPassed(resetsAtUtc: string | null): boolean {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!resetsAtUtc) return;
    const id = window.setInterval(() => setNow(Date.now()), TICK_MS);
    return () => window.clearInterval(id);
  }, [resetsAtUtc]);
  if (!resetsAtUtc) return false;
  const at = new Date(resetsAtUtc).getTime();
  return !Number.isNaN(at) && now >= at;
}

/** Scenes page: quota pause / resume, cost estimate and "Scenes per image" (07 §5–6). */
export function ImagesPanel({ project, estimate, running, onScenesPerImage, onResume, secondsPerImage, onBattery }: Props) {
  const { t, lang } = useI18n();
  const pause = project.status === "paused_quota" ? project.image_pause : null;
  const ready = useResetPassed(pause?.resets_at_utc ?? null);
  const perImage = project.image_settings.scenes_per_image;

  const estimateText = (() => {
    if (!estimate || estimate.images === 0) return null;
    const parts = [t("images.estimate.images", { n: estimate.images })];
    if (estimate.cloudflare > 0) {
      parts.push(
        t("images.estimate.cloudflare", {
          n: estimate.cloudflare,
          percent: Math.max(1, Math.round(estimate.cloudflare_percent_of_daily)),
        }),
      );
    }
    if (estimate.local > 0) {
      parts.push(
        secondsPerImage
          ? t("images.estimate.localTime", {
              n: estimate.local,
              min: Math.max(1, Math.round((estimate.local * secondsPerImage) / 60)),
            })
          : t("images.estimate.local", { n: estimate.local }),
      );
    }
    if (estimate.gemini > 0) {
      parts.push(t("images.estimate.gemini", { n: estimate.gemini, cost: estimate.gemini_cost_usd.toFixed(2) }));
    }
    return parts.join(" · ");
  })();

  return (
    <div className="images-panel">
      {pause && !running && (
        <Notice
          tone={ready ? "ok" : "warn"}
          actions={
            <Button variant={ready ? "primary" : "secondary"} icon={<Play size={16} strokeWidth={1.5} />} onClick={onResume}>
              {t("images.resume")}
            </Button>
          }
        >
          {ready ? (
            <>
              <strong>{t("images.readyToResume")}</strong> {t("images.pausedProgress", { done: pause.done, total: pause.total })}
            </>
          ) : (
            <>
              <strong>{t("images.paused")}</strong>{" "}
              {t("images.pausedProgress", { done: pause.done, total: pause.total })}{" "}
              {t("images.resumeAfter", { time: formatTime(pause.resets_at_utc, lang) })}
            </>
          )}
        </Notice>
      )}

      <div className="row images-panel__options">
        <span className="t-small">{t("images.perImage")}</span>
        <div className="segmented" role="group" aria-label={t("images.perImage")}>
          {([1, 2] as const).map((n) => (
            <button
              key={n}
              type="button"
              aria-pressed={perImage === n}
              disabled={running || project.status === "rendering"}
              onClick={() => perImage !== n && onScenesPerImage(n)}
            >
              {n}
            </button>
          ))}
        </div>
        {!running && estimateText && <span className="t-small">{estimateText}</span>}
        {!running && estimate && estimate.images === 0 && estimate.cached > 0 && (
          <span className="t-small">{t("images.estimate.upToDate")}</span>
        )}
      </div>
      {!running && onBattery && estimate && estimate.local > 10 && (
        <Notice tone="warn">{t("images.onBattery")}</Notice>
      )}
      {!running && estimate && estimate.cloudflare > estimate.cloudflare_images_per_day && (
        <Notice tone="warn">{t("images.estimate.overDaily", { n: estimate.cloudflare_images_per_day })}</Notice>
      )}
    </div>
  );
}
