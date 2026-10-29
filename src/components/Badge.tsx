import { LoaderCircle } from "lucide-react";
import type { ReactNode } from "react";

import { useI18n } from "../i18n/context";
import type { AssetStatus, ProjectStatus } from "../lib/types";

export type Tone = "neutral" | "ok" | "info" | "warn" | "danger";

export function Badge({ tone = "neutral", dot, children }: { tone?: Tone; dot?: boolean; children: ReactNode }) {
  return (
    <span className={tone === "neutral" ? "badge" : `badge badge--${tone}`}>
      {dot && <span className="badge__dot" aria-hidden="true" />}
      {children}
    </span>
  );
}

const PROJECT_TONE: Record<ProjectStatus, Tone> = {
  script_draft: "neutral",
  generating: "info",
  review: "warn",
  rendering: "info",
  done: "ok",
  paused_quota: "warn",
};

export function ProjectStatusBadge({ status }: { status: ProjectStatus }) {
  const { t } = useI18n();
  return (
    <Badge tone={PROJECT_TONE[status]} dot>
      {t(`status.${status}`)}
    </Badge>
  );
}

const ASSET_TONE: Record<AssetStatus, Tone> = {
  pending: "neutral",
  running: "info",
  done: "ok",
  failed: "danger",
  stale: "warn",
};

export function AssetStatusBadge({ status }: { status: AssetStatus }) {
  const { t } = useI18n();
  return (
    <Badge tone={ASSET_TONE[status]}>
      {status === "running" && <LoaderCircle size={12} strokeWidth={2} className="spin" />}
      {t(`asset.${status}`)}
    </Badge>
  );
}
