import { Clapperboard, ImageIcon, ImagePlus, PencilLine, Play, RotateCcw, Sparkles, Square, Volume2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { AssetStatusBadge, Badge, ProjectStatusBadge } from "../components/Badge";
import { Button, IconButton } from "../components/Button";
import { EmptyState } from "../components/EmptyState";
import { ErrorPanel } from "../components/ErrorPanel";
import { ImagePromptDialog } from "../components/ImagePromptDialog";
import { ImagesPanel } from "../components/ImagesPanel";
import { Notice } from "../components/Notice";
import { PageHeader } from "../components/PageHeader";
import { ProgressBar } from "../components/ProgressBar";
import { ProjectVoicePicker } from "../components/ProjectVoicePicker";
import { RetryStatus } from "../components/RetryStatus";
import { SceneThumb } from "../components/SceneThumb";
import { useToast } from "../components/toast";
import { useI18n } from "../i18n/context";
import { api, errorMessage, onGenerateProgress, onGeminiRetry, onSceneProgress } from "../lib/api";
import { mimeForPath, onPlayingChange, playAudio, stopAudio } from "../lib/audioPlayer";
import type { Navigate } from "../lib/route";
import type { GeminiRetry, GenerateProgress, ImageEstimate, Project, Scene, SceneProgress } from "../lib/types";

const ICON = { size: 16, strokeWidth: 1.5 } as const;
/** Image "done" events arrive one per scene; reload the project at most this often. */
const RELOAD_DELAY_MS = 400;

type Job = "voice" | "image";
type JobState = { running: boolean; progress: GenerateProgress | null };
type Retry = { notice: GeminiRetry; at: number } | null;

const IDLE: JobState = { running: false, progress: null };

function formatDuration(ms: number | null): string {
  if (ms === null) return "–:––";
  const s = Math.round(ms / 1000);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

/** Mirrors `images::plan::source_index`: with 2 scenes per image, odd scenes reuse the previous one. */
function sourceIndex(index: number, scenesPerImage: number): number {
  return scenesPerImage >= 2 && index % 2 === 1 ? index - 1 : index;
}

function applySceneProgress(p: Project, ev: SceneProgress): Project {
  return {
    ...p,
    scenes: p.scenes.map((s) => {
      if (s.id !== ev.scene_id) return s;
      if (ev.asset === "image") {
        return { ...s, image: { ...s.image, status: ev.status, error: ev.error, error_kind: ev.error_kind } };
      }
      return {
        ...s,
        voice: {
          ...s.voice,
          status: ev.status,
          duration_ms: ev.duration_ms ?? s.voice.duration_ms,
          error: ev.error,
          error_kind: ev.error_kind,
        },
      };
    }),
  };
}

function countAssets(scenes: Scene[], pick: (s: Scene) => Scene["voice"]["status"]) {
  let done = 0;
  let failed = 0;
  for (const s of scenes) {
    const st = pick(s);
    if (st === "done") done += 1;
    if (st === "failed") failed += 1;
  }
  return { done, failed, todo: scenes.length - done };
}

export function ScenesPage({ projectId, navigate }: { projectId: string; navigate: Navigate }) {
  const { t } = useI18n();
  const toast = useToast();
  const [project, setProject] = useState<Project | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [jobs, setJobs] = useState<Record<Job, JobState>>({ voice: IDLE, image: IDLE });
  const [retry, setRetry] = useState<Record<Job, Retry>>({ voice: null, image: null });
  const [estimate, setEstimate] = useState<ImageEstimate | null>(null);
  const [local, setLocal] = useState<{ secondsPerImage: number | null; onBattery: boolean }>({
    secondsPerImage: null,
    onBattery: false,
  });
  /** When each image of the current run finished (for "≈ s / image · ~min left"). */
  const [imageTimes, setImageTimes] = useState<number[]>([]);
  const [playing, setPlaying] = useState<string | null>(null);
  const [promptEdit, setPromptEdit] = useState<Scene | null>(null);
  const reloadTimer = useRef<number | null>(null);

  const refreshEstimate = useCallback(() => {
    api
      .imagesEstimate(projectId)
      .then(setEstimate)
      .catch(() => setEstimate(null));
  }, [projectId]);

  const reload = useCallback(async () => {
    try {
      setProject(await api.projectGet(projectId));
      refreshEstimate();
    } catch (e) {
      setLoadError(errorMessage(e));
    }
  }, [projectId, refreshEstimate]);

  const reloadSoon = useCallback(() => {
    if (reloadTimer.current !== null) return;
    reloadTimer.current = window.setTimeout(() => {
      reloadTimer.current = null;
      void reload();
    }, RELOAD_DELAY_MS);
  }, [reload]);

  useEffect(
    () => () => {
      if (reloadTimer.current !== null) window.clearTimeout(reloadTimer.current);
    },
    [],
  );

  const setJob = (job: Job, patch: Partial<JobState>) => setJobs((j) => ({ ...j, [job]: { ...j[job], ...patch } }));

  useEffect(() => {
    api
      .projectGet(projectId)
      .then(setProject)
      .catch((e: unknown) => setLoadError(errorMessage(e)));
    api
      .imagesEstimate(projectId)
      .then(setEstimate)
      .catch(() => setEstimate(null));
    api
      .localStatus()
      .then((s) => setLocal({ secondsPerImage: s.prefs.seconds_per_image, onBattery: s.power.on_battery }))
      .catch(() => undefined);
    api
      .generateRunning(projectId)
      .then((running) => setJobs((j) => ({ ...j, voice: { ...j.voice, running } })))
      .catch(() => undefined);
    api
      .imagesRunning(projectId)
      .then((running) => setJobs((j) => ({ ...j, image: { ...j.image, running } })))
      .catch(() => undefined);
  }, [projectId]);

  useEffect(() => onPlayingChange(setPlaying), []);
  useEffect(() => () => stopAudio(), []);

  useEffect(() => {
    const subs = [
      onSceneProgress((ev) => {
        if (ev.project_id !== projectId) return;
        const job: Job = ev.asset === "image" ? "image" : "voice";
        if (job === "image" && ev.status === "done") setImageTimes((ts) => [...ts.slice(-9), Date.now()]);
        setRetry((r) => ({ ...r, [job]: null }));
        setProject((p) => (p ? applySceneProgress(p, ev) : p));
        // Thumbnail path / hash are not in the event.
        if (job === "image" && ev.status === "done") reloadSoon();
      }),
      onGenerateProgress((ev) => {
        if (ev.project_id !== projectId) return;
        const running = ev.stage === "running";
        setJobs((j) => ({ ...j, [ev.job]: { running, progress: ev } }));
        if (!running) {
          setRetry((r) => ({ ...r, [ev.job]: null }));
          void reload();
        }
      }),
      onGeminiRetry((r) => {
        if (r.project_id !== projectId) return;
        if (r.scope === "voice" || r.scope === "image") {
          setRetry((prev) => ({ ...prev, [r.scope]: { notice: r, at: Date.now() } }));
        }
      }),
    ];
    return () => {
      for (const s of subs) void s.then((fn) => fn());
    };
  }, [projectId, reload, reloadSoon]);

  const scenes = useMemo(() => project?.scenes ?? [], [project]);
  const voices = useMemo(() => countAssets(scenes, (s) => s.voice.status), [scenes]);
  const images = useMemo(() => countAssets(scenes, (s) => s.image.status), [scenes]);

  const start = async (job: Job, fn: () => Promise<null>) => {
    try {
      setJob(job, { progress: null });
      await fn();
      setJob(job, { running: true });
    } catch (e) {
      toast.show(errorMessage(e), "danger");
    }
  };

  const startVoices = () => void start("voice", () => api.generateStart(projectId));
  const startImages = () => {
    setImageTimes([]);
    void start("image", () => api.imagesStart(projectId));
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

  const update = async (fn: () => Promise<Project>) => {
    try {
      setProject(await fn());
      refreshEstimate();
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

  const lang = project.language;
  const voice = project.voice;
  const engineLabel = t(`engine.${voice.engine}`);
  const anyRunning = jobs.voice.running || jobs.image.running;
  const paused = project.status === "paused_quota";
  const perImage = project.image_settings.scenes_per_image;

  /** Average of the last few image intervals → "≈ 7 s / image · 38 left · ~5 min". */
  const imageSpeed = (left: number): string | null => {
    const ts = imageTimes;
    if (ts.length < 2 || left <= 0) return null;
    const first = ts[0] ?? 0;
    const last = ts.at(-1) ?? 0;
    const per = (last - first) / 1000 / (ts.length - 1);
    return t("images.speed", { s: per.toFixed(1), left, min: Math.max(1, Math.round((per * left) / 60)) });
  };

  const jobStrip = (job: Job, label: string, retryJob: () => void) => {
    const { running, progress } = jobs[job];
    const total = progress?.total ?? 0;
    const done = (progress?.done ?? 0) + (progress?.failed ?? 0);
    const r = retry[job];
    if (running) {
      return (
        <>
          <span className="t-small">
            {label}: {t("scenes.jobProgress", { done, total })}
            {(progress?.failed ?? 0) > 0 && ` · ${t("scenes.progressFailed", { failed: progress?.failed ?? 0 })}`}
            {job === "image" && imageSpeed(total - done) && ` · ${imageSpeed(total - done) ?? ""}`}
          </span>
          <ProgressBar label={label} value={total > 0 ? done / total : undefined} />
          {r && <RetryStatus key={r.at} retry={r.notice} receivedAt={r.at} />}
        </>
      );
    }
    if (progress?.stage === "failed") {
      return <ErrorPanel kind={progress.error_kind} details={progress.error} onRetry={retryJob} />;
    }
    if (progress?.stage === "cancelled") {
      return <Notice tone="warn">{job === "voice" ? t("scenes.cancelled") : t("images.cancelled")}</Notice>;
    }
    return null;
  };

  const failedTotal = voices.failed + images.failed;
  const allReady = scenes.length > 0 && voices.todo === 0 && images.todo === 0;

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
              {t("scenes.progress", { done: voices.done, total: scenes.length })} ·{" "}
              {t("images.progress", { done: images.done, total: scenes.length })}
              {failedTotal > 0 && ` · ${t("scenes.progressFailed", { failed: failedTotal })}`}
            </span>
            {voice.voice_name && voice.engine !== "voxcpm" && (
              <span className="t-small">· {t("scenes.voiceSummary", { engine: engineLabel, voice: voice.voice_name })}</span>
            )}
            <ProjectVoicePicker project={project} disabled={anyRunning} onChange={setProject} />
          </div>
        }
        actions={
          <>
            <Button
              icon={<PencilLine {...ICON} />}
              disabled={anyRunning}
              onClick={() => navigate({ name: "script", projectId })}
            >
              {t("script.edit")}
            </Button>
            {jobs.voice.running ? (
              <Button icon={<Square {...ICON} />} onClick={() => void api.generateCancel(projectId)}>
                {t("scenes.stopVoices")}
              </Button>
            ) : (
              voices.todo > 0 && (
                <Button icon={<Volume2 {...ICON} />} onClick={startVoices}>
                  {voices.done > 0 ? t("scenes.continue") : t("scenes.generate")}
                </Button>
              )
            )}
            {jobs.image.running ? (
              <Button icon={<Square {...ICON} />} onClick={() => void api.imagesCancel(projectId)}>
                {t("images.stop")}
              </Button>
            ) : (
              images.todo > 0 &&
              !paused && (
                <Button variant="primary" icon={<Sparkles {...ICON} />} onClick={startImages}>
                  {images.done > 0 ? t("images.continue") : t("images.generate")}
                </Button>
              )
            )}
            {allReady && !anyRunning && (
              <Button
                variant="primary"
                icon={<Clapperboard {...ICON} />}
                onClick={() => navigate({ name: "render", projectId })}
              >
                {t("render.open")}
              </Button>
            )}
          </>
        }
      />

      <div className="status-strip">
        {jobStrip("voice", t("scenes.voices"), startVoices)}
        {jobStrip("image", t("images.title"), startImages)}
        {scenes.length > 0 && (
          <ImagesPanel
            project={project}
            estimate={estimate}
            running={jobs.image.running}
            onScenesPerImage={(n) => void update(() => api.projectSetScenesPerImage(projectId, n))}
            onResume={startImages}
            secondsPerImage={local.secondsPerImage}
            onBattery={local.onBattery}
          />
        )}
        {!anyRunning && failedTotal > 0 && <Notice tone="warn">{t("scenes.someFailed", { n: failedTotal })}</Notice>}
        {!anyRunning && allReady && (
          <Notice tone="ok">
            {t("scenes.allReady")} {t("scenes.nextRender")}
          </Notice>
        )}
      </div>

      {scenes.length === 0 ? (
        <EmptyState icon={<Volume2 size={18} strokeWidth={1.5} />} title={t("scenes.empty")} body="" />
      ) : (
        <div className="scene-grid">
          {scenes.map((s, i) => {
            const hasVoice =
              s.voice.path !== null &&
              (s.voice.status === "done" || s.voice.status === "failed" || s.voice.status === "stale");
            const source = scenes[sourceIndex(i, perImage)] ?? s;
            const shared = source.id !== s.id;
            const thumbVersion = s.image.thumb_path ? `${s.image.thumb_path}#${s.image.prompt_hash ?? ""}` : null;
            return (
              <article key={s.id} className="scene-card card">
                <SceneThumb
                  projectId={projectId}
                  sceneId={s.id}
                  version={thumbVersion}
                  alt={t("images.thumbAlt", { n: s.order })}
                />
                <div className="scene-card__meta">
                  <span className="t-micro t-muted">
                    {String(s.order).padStart(3, "0")} · {formatDuration(s.voice.duration_ms)}
                  </span>
                  {s.key_scene && <Badge>{t("images.keyScene")}</Badge>}
                </div>
                <div className="scene-card__status">
                  <span className="scene-card__asset" title={t("scenes.voice")}>
                    <Volume2 size={12} strokeWidth={1.5} aria-label={t("scenes.voice")} />
                    <AssetStatusBadge status={s.voice.status} />
                  </span>
                  <span className="scene-card__asset" title={t("images.image")}>
                    <ImageIcon size={12} strokeWidth={1.5} aria-label={t("images.image")} />
                    <AssetStatusBadge status={s.image.status} />
                  </span>
                </div>
                <p className="scene-card__text" lang={lang}>
                  {s.narration}
                </p>
                {shared && <span className="t-micro t-muted">{t("images.sharedWith", { n: source.order })}</span>}
                {s.voice.status === "failed" && s.voice.error && (
                  <ErrorPanel kind={s.voice.error_kind} details={s.voice.error} />
                )}
                {s.image.status === "failed" && s.image.error && !shared && (
                  <>
                    <ErrorPanel kind={s.image.error_kind} details={s.image.error} />
                    {s.image.error_kind === "blocked" && <span className="t-small">{t("images.blockedHint")}</span>}
                  </>
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
                    disabled={jobs.voice.running}
                    onClick={() => void start("voice", () => api.sceneRegenerate(projectId, s.id, "voice"))}
                  />
                  <IconButton
                    label={t("images.regenerate")}
                    icon={<ImagePlus {...ICON} />}
                    disabled={jobs.image.running || project.status === "rendering"}
                    onClick={() => void start("image", () => api.sceneRegenerate(projectId, s.id, "image"))}
                  />
                  <IconButton
                    label={t("images.editPrompt")}
                    icon={<PencilLine {...ICON} />}
                    disabled={jobs.image.running || project.status === "rendering"}
                    onClick={() => setPromptEdit(source)}
                  />
                </div>
              </article>
            );
          })}
        </div>
      )}

      {promptEdit && (
        <ImagePromptDialog
          title={t("images.editPromptTitle", { n: promptEdit.order })}
          initial={promptEdit.image_prompt}
          onCancel={() => setPromptEdit(null)}
          onSave={(prompt) => {
            const id = promptEdit.id;
            setPromptEdit(null);
            void update(() => api.sceneImagePromptUpdate(projectId, id, prompt));
          }}
        />
      )}
    </div>
  );
}
