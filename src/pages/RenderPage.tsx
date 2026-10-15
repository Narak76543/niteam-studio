import { Clapperboard, FolderOpen, Play, Square } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";

import { Badge, ProjectStatusBadge } from "../components/Badge";
import { Button } from "../components/Button";
import { ErrorPanel } from "../components/ErrorPanel";
import { Notice } from "../components/Notice";
import { PageHeader } from "../components/PageHeader";
import { ProgressBar } from "../components/ProgressBar";
import { useToast } from "../components/toast";
import { useI18n } from "../i18n/context";
import { api, errorMessage, onRenderProgress } from "../lib/api";
import { prepareSubtitles } from "../lib/prepareSubtitles";
import type { Navigate } from "../lib/route";
import type { Project, RenderProgress, Settings } from "../lib/types";

const ICON = { size: 16, strokeWidth: 1.5 } as const;

type Phase =
  | { kind: "idle" }
  | { kind: "subs"; done: number; total: number }
  | { kind: "render"; progress: RenderProgress | null };

type Outcome = { kind: "done"; path: string } | { kind: "failed"; errorKind: string | null; error: string } | { kind: "cancelled" } | null;

function formatEta(seconds: number): string {
  const s = Math.max(0, Math.round(seconds));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

function totalDurationMs(p: Project): number {
  return p.scenes.reduce((sum, s) => sum + (s.voice.duration_ms ?? 0) + 250, 0);
}

export function RenderPage({ projectId, navigate }: { projectId: string; navigate: Navigate }) {
  const { t } = useI18n();
  const toast = useToast();
  const [project, setProject] = useState<Project | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [phase, setPhase] = useState<Phase>({ kind: "idle" });
  const [outcome, setOutcome] = useState<Outcome>(null);
  const cancelSubs = useRef(false);

  const reload = useCallback(async () => {
    try {
      setProject(await api.projectGet(projectId));
    } catch (e) {
      setLoadError(errorMessage(e));
    }
  }, [projectId]);

  useEffect(() => {
    api
      .projectGet(projectId)
      .then(setProject)
      .catch((e: unknown) => setLoadError(errorMessage(e)));
    api
      .settingsGet()
      .then(setSettings)
      .catch(() => setSettings(null));
    api
      .renderRunning(projectId)
      .then((running) => {
        if (running) setPhase({ kind: "render", progress: null });
      })
      .catch(() => undefined);
  }, [projectId]);

  useEffect(() => {
    const unlisten = onRenderProgress((p) => {
      if (p.project_id !== projectId) return;
      if (p.stage === "clips" || p.stage === "concat") {
        setPhase({ kind: "render", progress: p });
        return;
      }
      setPhase({ kind: "idle" });
      if (p.stage === "done") setOutcome({ kind: "done", path: p.output_path ?? "" });
      else if (p.stage === "cancelled") setOutcome({ kind: "cancelled" });
      else setOutcome({ kind: "failed", errorKind: p.error_kind, error: p.error ?? "" });
      void reload();
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [projectId, reload]);

  const start = async () => {
    if (!project || !settings) return;
    setOutcome(null);
    cancelSubs.current = false;
    try {
      setPhase({ kind: "subs", done: 0, total: 0 });
      await prepareSubtitles(
        projectId,
        project.language,
        project.render.resolution,
        (done, total) => setPhase({ kind: "subs", done, total }),
        () => cancelSubs.current,
      );
      if (cancelSubs.current) {
        setPhase({ kind: "idle" });
        setOutcome({ kind: "cancelled" });
        return;
      }
      setPhase({ kind: "render", progress: null });
      await api.renderStart(projectId);
    } catch (e) {
      setPhase({ kind: "idle" });
      setOutcome({ kind: "failed", errorKind: null, error: errorMessage(e) });
    }
  };

  const cancel = () => {
    if (phase.kind === "subs") cancelSubs.current = true;
    else void api.renderCancel(projectId);
  };

  const open = async (reveal: boolean) => {
    try {
      await api.renderOpenOutput(projectId, reveal);
    } catch (e) {
      toast.show(errorMessage(e), "danger");
    }
  };

  if (loadError) {
    return (
      <div className="page">
        <Notice tone="danger">{loadError}</Notice>
      </div>
    );
  }
  if (!project) {
    return (
      <div className="page">
        <p className="t-small">{t("common.loading")}</p>
      </div>
    );
  }

  const scenes = project.scenes;
  const voicesDone = scenes.filter((s) => s.voice.status === "done").length;
  const imagesDone = scenes.filter((s) => s.image.status === "done").length;
  const ready = scenes.length > 0 && voicesDone === scenes.length && imagesDone === scenes.length;
  const busy = phase.kind !== "idle";
  const lengthS = Math.round(totalDurationMs(project) / 1000);
  const existing = outcome === null && project.status === "done" ? project.render.output_path : null;
  const encoder = project.render.encoder ?? settings?.render.encoder ?? null;

  const progressLabel = (() => {
    if (phase.kind === "subs") return t("render.subs", { done: phase.done, total: phase.total });
    if (phase.kind !== "render") return "";
    const p = phase.progress;
    if (!p) return t("render.starting");
    if (p.stage === "concat") return t("render.joining");
    return t("render.scene", { n: p.scene_index, total: p.total });
  })();
  const fraction =
    phase.kind === "subs"
      ? phase.total > 0
        ? phase.done / phase.total
        : undefined
      : phase.kind === "render" && phase.progress
        ? phase.progress.percent
        : undefined;
  const eta = phase.kind === "render" ? phase.progress?.eta_seconds : null;

  return (
    <div className="page">
      <PageHeader
        breadcrumb={[
          { label: t("nav.projects"), onClick: () => navigate({ name: "projects" }) },
          { label: project.title, onClick: () => navigate({ name: "scenes", projectId }) },
        ]}
        title={t("render.title")}
        meta={<ProjectStatusBadge status={project.status} />}
        actions={
          busy ? (
            <Button icon={<Square {...ICON} />} onClick={cancel}>
              {t("common.cancel")}
            </Button>
          ) : (
            <Button variant="primary" icon={<Clapperboard {...ICON} />} disabled={!ready || !settings} onClick={() => void start()}>
              {existing || outcome?.kind === "done" ? t("render.again") : t("render.start")}
            </Button>
          )
        }
      />

      <div className="render-layout">
        <section className="card render-card" aria-label={t("render.settings")}>
          <h2 className="t-h3">{t("render.settings")}</h2>
          <dl className="render-facts">
            <dt>{t("render.resolution")}</dt>
            <dd>1920 × 1080 (1080p)</dd>
            <dt>{t("render.length")}</dt>
            <dd>
              {t("render.lengthValue", { m: Math.floor(lengthS / 60), s: String(lengthS % 60).padStart(2, "0"), n: scenes.length })}
            </dd>
            <dt>{t("render.encoder")}</dt>
            <dd>{encoder ?? t("render.encoderAuto")}</dd>
            <dt>{t("render.output")}</dt>
            <dd className="path">{settings?.output_dir ?? t("settings.output.default")}</dd>
            <dt>{t("render.music")}</dt>
            <dd className="t-muted">{t("render.musicLater")}</dd>
          </dl>
          {!ready && (
            <Notice tone="warn" actions={<Button variant="pill" onClick={() => navigate({ name: "scenes", projectId })}>{t("render.openScenes")}</Button>}>
              {t("render.notReady", { voices: voicesDone, images: imagesDone, total: scenes.length })}
            </Notice>
          )}
        </section>

        <section className="card render-card" aria-label={t("render.progress")}>
          <h2 className="t-h3">{t("render.progress")}</h2>
          {busy ? (
            <>
              <div className="row render-card__status">
                <span className="t-small">{progressLabel}</span>
                {eta != null && <Badge>{t("render.eta", { time: formatEta(eta) })}</Badge>}
              </div>
              <ProgressBar label={progressLabel || t("render.progress")} value={fraction} />
              <span className="t-small t-muted">{t("render.lowPriority")}</span>
            </>
          ) : outcome?.kind === "failed" ? (
            <ErrorPanel kind={outcome.errorKind} details={outcome.error} onRetry={() => void start()} />
          ) : outcome?.kind === "cancelled" ? (
            <Notice tone="warn">{t("render.cancelled")}</Notice>
          ) : outcome?.kind === "done" || existing ? (
            <Notice
              tone="ok"
              actions={
                <>
                  <Button variant="primary" icon={<Play {...ICON} />} onClick={() => void open(false)}>
                    {t("render.openVideo")}
                  </Button>
                  <Button icon={<FolderOpen {...ICON} />} onClick={() => void open(true)}>
                    {t("render.showFolder")}
                  </Button>
                </>
              }
            >
              <strong>{t("render.done")}</strong>
              <span className="path"> {outcome?.kind === "done" ? outcome.path : existing}</span>
            </Notice>
          ) : (
            <p className="t-small">{ready ? t("render.readyHint") : t("render.waitHint")}</p>
          )}
        </section>
      </div>
    </div>
  );
}
