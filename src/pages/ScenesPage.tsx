import { ImageIcon, PencilLine, Play, RotateCcw, Sparkles, Square, Volume2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";

import { AssetStatusBadge, ProjectStatusBadge } from "../components/Badge";
import { Button, IconButton } from "../components/Button";
import { EmptyState } from "../components/EmptyState";
import { ErrorPanel } from "../components/ErrorPanel";
import { Notice } from "../components/Notice";
import { PageHeader } from "../components/PageHeader";
import { ProgressBar } from "../components/ProgressBar";
import { RetryStatus } from "../components/RetryStatus";
import { useToast } from "../components/toast";
import { useI18n } from "../i18n/context";
import { api, errorMessage, onGenerateProgress, onGeminiRetry, onSceneProgress } from "../lib/api";
import { mimeForPath, onPlayingChange, playAudio, stopAudio } from "../lib/audioPlayer";
import type { Navigate } from "../lib/route";
import type { GeminiRetry, GenerateProgress, Project, Scene, SceneProgress } from "../lib/types";

const ICON = { size: 16, strokeWidth: 1.5 } as const;

function formatDuration(ms: number | null): string {
  if (ms === null) return "–:––";
  const s = Math.round(ms / 1000);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

function applySceneProgress(p: Project, ev: SceneProgress): Project {
  if (ev.asset !== "voice") return p;
  return {
    ...p,
    scenes: p.scenes.map((s) =>
      s.id === ev.scene_id
        ? {
            ...s,
            voice: {
              ...s.voice,
              status: ev.status,
              duration_ms: ev.duration_ms ?? s.voice.duration_ms,
              error: ev.error,
              error_kind: ev.error_kind,
            },
          }
        : s,
    ),
  };
}

export function ScenesPage({ projectId, navigate }: { projectId: string; navigate: Navigate }) {
  const { t } = useI18n();
  const toast = useToast();
  const [project, setProject] = useState<Project | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<GenerateProgress | null>(null);
  const [retry, setRetry] = useState<{ notice: GeminiRetry; at: number } | null>(null);
  const [playing, setPlaying] = useState<string | null>(null);

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
      .generateRunning(projectId)
      .then(setRunning)
      .catch(() => setRunning(false));
  }, [projectId]);

  useEffect(() => onPlayingChange(setPlaying), []);
  useEffect(() => () => stopAudio(), []);

  useEffect(() => {
    const subs = [
      onSceneProgress((ev) => {
        if (ev.project_id !== projectId) return;
        setRetry(null);
        setProject((p) => (p ? applySceneProgress(p, ev) : p));
      }),
      onGenerateProgress((ev) => {
        if (ev.project_id !== projectId) return;
        setProgress(ev);
        if (ev.stage === "running") {
          setRunning(true);
        } else {
          setRunning(false);
          setRetry(null);
          void reload();
        }
      }),
      onGeminiRetry((r) => {
        if (r.scope === "voice" && r.project_id === projectId) setRetry({ notice: r, at: Date.now() });
      }),
    ];
    return () => {
      for (const s of subs) void s.then((fn) => fn());
    };
  }, [projectId, reload]);

  const scenes = useMemo(() => project?.scenes ?? [], [project]);
  const counts = useMemo(() => {
    let done = 0;
    let failed = 0;
    for (const s of scenes) {
      if (s.voice.status === "done") done += 1;
      if (s.voice.status === "failed") failed += 1;
    }
    return { done, failed, todo: scenes.length - done };
  }, [scenes]);

  const start = async (fn: () => Promise<null>) => {
    try {
      setProgress(null);
      await fn();
      setRunning(true);
    } catch (e) {
      toast.show(errorMessage(e), "danger");
    }
  };

  const play = async (scene: Scene) => {
    if (playing === scene.id) {
      stopAudio();
      return;
    }
    try {
      const bytes = await api.sceneVoiceAudio(projectId, scene.id);
      await playAudio(scene.id, bytes, mimeForPath(scene.voice.path));
    } catch (e) {
      toast.show(`${t("scenes.playError")} ${errorMessage(e)}`, "danger");
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

  const lang = project.language;
  const voice = project.voice;
  const engineLabel = voice.engine === "gemini" ? t("engine.gemini") : t("engine.edge");
  const total = progress?.total ?? 0;
  const jobDone = (progress?.done ?? 0) + (progress?.failed ?? 0);

  return (
    <div className="page">
      <PageHeader
        breadcrumb={[{ label: t("nav.projects"), onClick: () => navigate({ name: "projects" }) }]}
        title={project.title}
        titleLang={lang}
        meta={
          <div className="row">
            <ProjectStatusBadge status={project.status} />
            <span className="t-small">
              {t("scenes.progress", { done: counts.done, total: scenes.length })}
              {counts.failed > 0 && ` · ${t("scenes.progressFailed", { failed: counts.failed })}`}
            </span>
            {voice.voice_name && (
              <span className="t-small">
                · {t("scenes.voiceSummary", { engine: engineLabel, voice: voice.voice_name })}
              </span>
            )}
          </div>
        }
        actions={
          <>
            <Button
              icon={<PencilLine {...ICON} />}
              disabled={running}
              onClick={() => navigate({ name: "script", projectId })}
            >
              {t("script.edit")}
            </Button>
            {running ? (
              <Button icon={<Square {...ICON} />} onClick={() => void api.generateCancel(projectId)}>
                {t("scenes.stop")}
              </Button>
            ) : (
              counts.todo > 0 && (
                <Button
                  variant="primary"
                  icon={<Sparkles {...ICON} />}
                  onClick={() => void start(() => api.generateStart(projectId))}
                >
                  {counts.done > 0 ? t("scenes.continue") : t("scenes.generate")}
                </Button>
              )
            )}
          </>
        }
      />

      <div className="status-strip">
        {running && (
          <>
            <span className="t-small">
              {t("scenes.progress", { done: jobDone, total })}
              {(progress?.failed ?? 0) > 0 && ` · ${t("scenes.progressFailed", { failed: progress?.failed ?? 0 })}`}
            </span>
            <ProgressBar label={t("scenes.generate")} value={total > 0 ? jobDone / total : undefined} />
            {retry && <RetryStatus key={retry.at} retry={retry.notice} receivedAt={retry.at} />}
          </>
        )}
        {!running && progress?.stage === "failed" && (
          <ErrorPanel
            kind={progress.error_kind}
            details={progress.error}
            onRetry={() => void start(() => api.generateStart(projectId))}
          />
        )}
        {!running && progress?.stage === "cancelled" && <Notice tone="warn">{t("scenes.cancelled")}</Notice>}
        {!running && progress?.stage !== "failed" && counts.failed > 0 && (
          <Notice tone="warn">{t("scenes.someFailed", { n: counts.failed })}</Notice>
        )}
        {!running && scenes.length > 0 && counts.todo === 0 && (
          <Notice tone="ok">
            {t("scenes.done")} {t("scenes.imagesLater")}
          </Notice>
        )}
      </div>

      {scenes.length === 0 ? (
        <EmptyState icon={<Volume2 size={18} strokeWidth={1.5} />} title={t("scenes.empty")} body="" />
      ) : (
        <div className="scene-grid">
          {scenes.map((s) => {
            const hasVoice = s.voice.path !== null && (s.voice.status === "done" || s.voice.status === "failed" || s.voice.status === "stale");
            return (
              <article key={s.id} className="scene-card card">
                <div className="thumb" aria-hidden="true">
                  <ImageIcon size={20} strokeWidth={1.5} />
                </div>
                <div className="scene-card__meta">
                  <span className="t-micro t-muted">
                    {String(s.order).padStart(3, "0")} · {formatDuration(s.voice.duration_ms)}
                  </span>
                  <AssetStatusBadge status={s.voice.status} />
                </div>
                <p className="scene-card__text" lang={lang}>
                  {s.narration}
                </p>
                {s.voice.status === "failed" && s.voice.error && (
                  <ErrorPanel kind={s.voice.error_kind} details={s.voice.error} />
                )}
                <div className="scene-card__actions">
                  <IconButton
                    label={playing === s.id ? t("scenes.stopAudio") : hasVoice ? t("scenes.play") : t("scenes.noVoice")}
                    icon={playing === s.id ? <Square {...ICON} /> : <Play {...ICON} />}
                    disabled={!hasVoice}
                    onClick={() => void play(s)}
                  />
                  <IconButton
                    label={t("scenes.regenerateVoice")}
                    icon={<RotateCcw {...ICON} />}
                    disabled={running}
                    onClick={() => void start(() => api.sceneRegenerate(projectId, s.id, "voice"))}
                  />
                </div>
              </article>
            );
          })}
        </div>
      )}
    </div>
  );
}
