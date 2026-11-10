import { Check, Clapperboard, FileText, Images } from "lucide-react";
import type { ReactNode } from "react";

import { useI18n } from "../i18n/context";
import type { Navigate } from "../lib/route";
import type { ProjectStatus } from "../lib/types";

type Step = "script" | "scenes" | "render";

interface Props {
  projectId: string;
  status: ProjectStatus;
  current: Step;
  navigate: Navigate;
  /** Scenes page: true once every voice and picture is ready (unlocks Render early). */
  scenesReady?: boolean;
  /** A job is running: steps cannot be opened. */
  locked?: boolean;
}

const ICON = { size: 14, strokeWidth: 1.75 } as const;

/** Script → Scenes → Render bar under a project's title: where you are and what is done. */
export function ProjectFlow({ projectId, status, current, navigate, scenesReady = false, locked = false }: Props) {
  const { t } = useI18n();
  const scriptDone = status !== "script_draft";
  const scenesDone = scenesReady || status === "review" || status === "rendering" || status === "done";
  const steps: { id: Step; label: string; icon: ReactNode; done: boolean; open: boolean }[] = [
    { id: "script", label: t("flow.script"), icon: <FileText {...ICON} />, done: scriptDone, open: true },
    { id: "scenes", label: t("flow.scenes"), icon: <Images {...ICON} />, done: scenesDone, open: scriptDone },
    { id: "render", label: t("flow.render"), icon: <Clapperboard {...ICON} />, done: status === "done", open: scenesDone },
  ];
  return (
    <nav className="flow" aria-label={t("flow.label")}>
      {steps.map((s, i) => {
        const here = s.id === current;
        const cls = `flow__step${here ? " flow__step--current" : ""}${s.done ? " flow__step--done" : ""}`;
        return (
          <span key={s.id} className="flow__item">
            {i > 0 && <span className={`flow__line${steps[i - 1]?.done ? " flow__line--done" : ""}`} aria-hidden="true" />}
            <button
              type="button"
              className={cls}
              aria-current={here ? "step" : undefined}
              disabled={locked || !s.open || here}
              onClick={() => navigate({ name: s.id, projectId })}
            >
              <span className="flow__icon">{s.done && !here ? <Check {...ICON} strokeWidth={2.5} /> : s.icon}</span>
              <span className="t-micro">{s.label}</span>
            </button>
          </span>
        );
      })}
    </nav>
  );
}
