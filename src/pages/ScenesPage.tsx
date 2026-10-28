import { AlertTriangle, Clapperboard, Headphones, ImageIcon, ImagePlus, Images, Pause, PencilLine, Play, RotateCcw, ScanSearch, Sparkles, Square, Volume2, WandSparkles } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { AssetStatusBadge, Badge, ProjectStatusBadge } from "../components/Badge";
import { Button, IconButton } from "../components/Button";
import { ConfirmDialog } from "../components/ConfirmDialog";
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
import { VisualDialog } from "../components/VisualDialog";
import { VoicePlayerBar } from "../components/VoicePlayerBar";
import { useI18n } from "../i18n/context";
import { api, errorMessage, onGenerateProgress, onGeminiRetry, onPromptRewriteProgress, onSceneProgress } from "../lib/api";
import { mimeForPath, onAudioTimeUpdate, onPlayingChange, pauseAudio, playAudio, resumeAudio, seekAudio, stopAudio } from "../lib/audioPlayer";
import { prepareVisuals } from "../lib/prepareVisuals";
import { isRealWorld } from "../lib/presets";
import type { Navigate } from "../lib/route";
import type { GeminiRetry, GenerateProgress, ImageEstimate, Project, RewriteProgress, Scene, SceneProgress } from "../lib/types";
import { assetId, needsAiImage, visualReady } from "../lib/visuals";

const ICON = { size: 16, strokeWidth: 1.5 } as const;
/** Image "done" events arrive one per scene; reload the project at most this often. */
const RELOAD_DELAY_MS = 400;

type Job = "voice" | "image" | "visual";
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
      if (ev.asset === "visual") {
        return { ...s, visual: { ...s.visual, status: ev.status, error: ev.error, error_kind: ev.error_kind } };
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

/** Consecutive scenes of the same chapter, in story order. */
function groupByChapter(scenes: Scene[]): { chapter: number; items: { scene: Scene; index: number }[] }[] {
  const groups: { chapter: number; items: { scene: Scene; index: number }[] }[] = [];
  scenes.forEach((scene, index) => {
    const last = groups.at(-1);
    if (last && last.chapter === scene.chapter) last.items.push({ scene, index });
    else groups.push({ chapter: scene.chapter, items: [{ scene, index }] });
  });
  return groups;
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
  const [jobs, setJobs] = useState<Record<Job, JobState>>({ voice: IDLE, image: IDLE, visual: IDLE });
  const [retry, setRetry] = useState<Record<Job, Retry>>({ voice: null, image: null, visual: null });
  /** Scene whose visual is being edited (real-world projects). */
  const [visualEdit, setVisualEdit] = useState<string | null>(null);
  const [estimate, setEstimate] = useState<ImageEstimate | null>(null);
  const [local, setLocal] = useState<{ secondsPerImage: number | null; onBattery: boolean }>({
    secondsPerImage: null,
    onBattery: false,
  });
  /** When each image of the current run finished (for "≈ s / image · ~min left"). */
  const [imageTimes, setImageTimes] = useState<number[]>([]);
  const [playing, setPlaying] = useState<string | null>(null);
  const [continuousMode, setContinuousMode] = useState(false);
  const [isPaused, setIsPaused] = useState(false);
  const [audioTime, setAudioTime] = useState<{ currentTime: number; duration: number }>({ currentTime: 0, duration: 0 });
  const [autoScroll, setAutoScroll] = useState(true);
  const [promptEdit, setPromptEdit] = useState<Scene | null>(null);
  const [rewrite, setRewrite] = useState<{ running: boolean; progress: RewriteProgress | null }>({
    running: false,
    progress: null,
  });
  const [confirmRewrite, setConfirmRewrite] = useState(false);
  /** Aspect switch asked from the header badge (marks images/subtitles/clips outdated). */
  const [aspectAsk, setAspectAsk] = useState(false);
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
    api
      .visualsRunning(projectId)
      .then((running) => setJobs((j) => ({ ...j, visual: { ...j.visual, running } })))
      .catch(() => undefined);
    api
      .imagePromptsRewriteRunning(projectId)
      .then((running) => setRewrite((r) => ({ ...r, running })))
      .catch(() => undefined);
  }, [projectId]);

  // Real-world projects: draw diagram / code pictures whenever some are pending.
  const drawing = useRef(false);
  const needsDrawing =
    project !== null &&
    isRealWorld(project.style.preset) &&
    project.scenes.some((s) => (s.visual.kind === "diagram" || s.visual.kind === "code") && s.visual.status === "pending");
  useEffect(() => {
    if (!needsDrawing || drawing.current) return;
    drawing.current = true;
    prepareVisuals(projectId)
      .then(() => reload())
      .catch((e: unknown) => toast.show(errorMessage(e), "danger"))
      .finally(() => {
        drawing.current = false;
      });
  }, [needsDrawing, projectId, reload, toast]);

  useEffect(() => {
    return onPlayingChange((key) => {
      setPlaying(key);
      if (!key) {
        setIsPaused(false);
      }
    });
  }, []);
  useEffect(() => onAudioTimeUpdate(setAudioTime), []);
  useEffect(() => () => stopAudio(), []);

  useEffect(() => {
    const subs = [
      onSceneProgress((ev) => {
        if (ev.project_id !== projectId) return;
        const job: Job = ev.asset;
        if (job === "image" && ev.status === "done") setImageTimes((ts) => [...ts.slice(-9), Date.now()]);
        setRetry((r) => ({ ...r, [job]: null }));
        setProject((p) => (p ? applySceneProgress(p, ev) : p));
        // Thumbnail path / hash / picked file are not in the event.
        if ((job === "image" || job === "visual") && ev.status !== "running") reloadSoon();
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
      onPromptRewriteProgress((ev) => {
        if (ev.project_id !== projectId) return;
        setRewrite({ running: ev.stage === "running", progress: ev });
        // New prompts are saved per batch: show them (and the outdated images) right away.
        void reload();
        if (ev.stage === "done") toast.show(t("prompts.done", { n: ev.changed }), "ok");
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
  }, [projectId, reload, reloadSoon, t, toast]);

  const scenes = useMemo(() => project?.scenes ?? [], [project]);
  const realWorld = project !== null && isRealWorld(project.style.preset);
  const voices = useMemo(() => countAssets(scenes, (s) => s.voice.status), [scenes]);
  // Real-world projects: AI images only for the scenes that need one.
  const images = useMemo(
    () => countAssets(realWorld ? scenes.filter(needsAiImage) : scenes, (s) => s.image.status),
    [scenes, realWorld],
  );
  const stockScenes = useMemo(
    () => scenes.filter((s) => s.visual.kind === "real_photo" || s.visual.kind === "stock_video"),
    [scenes],
  );
  const visualTodo = stockScenes.filter((s) => s.visual.asset === null && s.visual.status !== "failed").length;
  const toCheck = scenes.filter((s) => s.visual.auto_picked && !needsAiImage(s)).length;

  const start = async (job: Job, fn: () => Promise<null>) => {
    try {
      setJob(job, { progress: null });
      await fn();
      setJob(job, { running: true });
    } catch (e) {
      toast.show(errorMessage(e), "danger");
    }
  };

  const startRewrite = async () => {
    setConfirmRewrite(false);
    try {
      setRewrite({ running: false, progress: null });
      await api.imagePromptsRewrite(projectId);
      setRewrite((r) => ({ ...r, running: true }));
    } catch (e) {
      toast.show(errorMessage(e), "danger");
    }
  };

  const startVoices = () => void start("voice", () => api.generateStart(projectId));
  const startVisuals = () => void start("visual", () => api.visualsStart(projectId, null));
  const startImages = () => {
    setImageTimes([]);
    void start("image", () => api.imagesStart(projectId));
  };

  const readyScenes = useMemo(
    () => scenes.filter((s) => s.voice.path !== null && s.voice.status === "done"),
    [scenes],
  );
  const totalVoiceMs = useMemo(
    () => readyScenes.reduce((acc, s) => acc + (s.voice.duration_ms ?? 0), 0),
    [readyScenes],
  );
  const currentVoiceSceneIndex = useMemo(() => {
    return playing ? readyScenes.findIndex((s) => s.id === playing) + 1 : 0;
  }, [playing, readyScenes]);

  const elapsedBeforeMs = useMemo(() => {
    if (!playing) return 0;
    const idx = readyScenes.findIndex((s) => s.id === playing);
    if (idx <= 0) return 0;
    return readyScenes.slice(0, idx).reduce((acc, s) => acc + (s.voice.duration_ms ?? 0), 0);
  }, [playing, readyScenes]);

  const activePlayingScene = useMemo(() => {
    return scenes.find((s) => s.id === playing) ?? null;
  }, [scenes, playing]);

  const playSceneRef = useRef<(id: string) => Promise<void>>(() => Promise.resolve());

  const playSceneInPlaylist = useCallback(
    async (sceneId: string) => {
      const targetIndex = scenes.findIndex((s) => s.id === sceneId);
      if (targetIndex === -1) return;
      const scene = scenes[targetIndex];
      if (!scene) return;
      if (scene.voice.status !== "done") {
        const nextReady = scenes.slice(targetIndex + 1).find((s) => s.voice.status === "done");
        if (nextReady) {
          void playSceneRef.current(nextReady.id);
        } else {
          stopAudio();
          setContinuousMode(false);
          setIsPaused(false);
        }
        return;
      }

      setIsPaused(false);

      if (autoScroll) {
        const el = document.getElementById(`scene-${scene.id}`);
        if (el) {
          el.scrollIntoView({ behavior: "smooth", block: "nearest" });
        }
      }

      try {
        const bytes = await api.sceneVoiceAudio(projectId, scene.id);
        await playAudio(scene.id, bytes, mimeForPath(scene.voice.path), () => {
          const nextReady = scenes.slice(targetIndex + 1).find((s) => s.voice.status === "done");
          if (nextReady) {
            void playSceneRef.current(nextReady.id);
          } else {
            setContinuousMode(false);
            setIsPaused(false);
            toast.show(t("scenes.playbackFinished"), "ok");
          }
        });
      } catch (e) {
        toast.show(`${t("scenes.playError")} ${errorMessage(e)}`, "danger");
        setContinuousMode(false);
        setIsPaused(false);
      }
    },
    [scenes, autoScroll, projectId, toast, t],
  );

  useEffect(() => {
    playSceneRef.current = playSceneInPlaylist;
  }, [playSceneInPlaylist]);

  const startContinuous = useCallback(() => {
    const firstReady = (playing ? scenes.find((s) => s.id === playing && s.voice.status === "done") : null)
      ?? scenes.find((s) => s.voice.status === "done");
    if (!firstReady) {
      toast.show(t("scenes.noVoice"), "neutral");
      return;
    }
    setContinuousMode(true);
    void playSceneInPlaylist(firstReady.id);
  }, [playing, scenes, toast, t, playSceneInPlaylist]);

  const toggleContinuousPlay = useCallback(() => {
    if (!continuousMode) {
      startContinuous();
      return;
    }
    if (isPaused) {
      void resumeAudio();
      setIsPaused(false);
    } else {
      pauseAudio();
      setIsPaused(true);
    }
  }, [continuousMode, isPaused, startContinuous]);

  const stopContinuous = useCallback(() => {
    stopAudio();
    setContinuousMode(false);
    setIsPaused(false);
  }, []);

  const stepContinuousScene = useCallback(
    (direction: -1 | 1) => {
      const curIdx = scenes.findIndex((s) => s.id === playing);
      if (curIdx === -1) return;
      if (direction === 1) {
        const next = scenes.slice(curIdx + 1).find((s) => s.voice.status === "done");
        if (next) void playSceneInPlaylist(next.id);
      } else {
        const prev = [...scenes.slice(0, curIdx)].reverse().find((s) => s.voice.status === "done");
        if (prev) void playSceneInPlaylist(prev.id);
      }
    },
    [scenes, playing, playSceneInPlaylist],
  );

  const play = async (scene: Scene) => {
    if (playing === scene.id) {
      stopAudio();
      setContinuousMode(false);
      setIsPaused(false);
      return;
    }
    if (continuousMode) {
      void playSceneInPlaylist(scene.id);
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
  const anyRunning = jobs.voice.running || jobs.image.running || jobs.visual.running || rewrite.running;
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
    if (job === "visual" && progress?.stage === "paused_quota") {
      return (
        <Notice
          tone="warn"
          actions={
            <Button variant="pill" onClick={retryJob}>
              {t("visual.resume")}
            </Button>
          }
        >
          {t("visual.paused", { time: progress.resets_at_utc ? new Date(progress.resets_at_utc).toLocaleTimeString() : "–" })}
        </Notice>
      );
    }
    return null;
  };

  const rewriteStrip = () => {
    const { running, progress } = rewrite;
    if (running) {
      const done = progress?.done ?? 0;
      const total = progress?.total ?? 0;
      return (
        <div className="row">
          <span className="t-small">{t("prompts.progress", { done, total })}</span>
          <ProgressBar label={t("prompts.action")} value={total > 0 ? done / total : undefined} />
          <Button
            variant="pill"
            icon={<Square size={12} strokeWidth={1.5} />}
            onClick={() => void api.imagePromptsRewriteCancel(projectId)}
          >
            {t("prompts.stop")}
          </Button>
        </div>
      );
    }
    if (progress?.stage === "failed") {
      return <ErrorPanel kind={progress.error_kind} details={progress.error} onRetry={() => void startRewrite()} />;
    }
    return null;
  };

  const failedTotal = voices.failed + images.failed;
  const allReady =
    scenes.length > 0 && voices.todo === 0 && (realWorld ? scenes.every(visualReady) : images.todo === 0);
  const editing = scenes.find((s) => s.id === visualEdit) ?? null;
  const drawPending = realWorld && scenes.some((s) => (s.visual.kind === "diagram" || s.visual.kind === "code") && s.visual.status === "pending");

  return (
    <div className="page">
      <PageHeader
        breadcrumb={[{ label: t("nav.projects"), onClick: () => navigate({ name: "projects" }) }]}
        title={project.title}
        titleLang={lang}
        meta={
          <div className="row">
            <ProjectStatusBadge status={project.status} />
            <button
              type="button"
              className={`badge${project.aspect === "9:16" ? " badge--info" : ""}`}
              disabled={anyRunning}
              title={t("aspect.changeTitle")}
              onClick={() => setAspectAsk(true)}
            >
              {project.aspect}
            </button>
            <span className="t-small">
              {t("scenes.progress", { done: voices.done, total: scenes.length })} ·{" "}
              {realWorld
                ? t("visual.progress", { done: scenes.filter(visualReady).length, total: scenes.length })
                : t("images.progress", { done: images.done, total: scenes.length })}
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
            {voices.done > 0 && !jobs.voice.running && (
              <Button
                variant={continuousMode ? "primary" : "secondary"}
                icon={continuousMode && !isPaused ? <Pause {...ICON} /> : <Headphones {...ICON} />}
                onClick={toggleContinuousPlay}
              >
                {continuousMode
                  ? (isPaused ? t("scenes.resumeAll") : t("scenes.pauseAll"))
                  : t("scenes.playAllVoices")}
              </Button>
            )}
            {realWorld &&
              (jobs.visual.running ? (
                <Button icon={<Square {...ICON} />} onClick={() => void api.visualsCancel(projectId)}>
                  {t("visual.stop")}
                </Button>
              ) : (
                visualTodo > 0 && (
                  <Button variant="primary" icon={<ScanSearch {...ICON} />} disabled={anyRunning} onClick={startVisuals}>
                    {t("visual.find")}
                  </Button>
                )
              ))}
            {scenes.length > 0 && !rewrite.running && !realWorld && (
              <Button
                icon={<WandSparkles {...ICON} />}
                disabled={anyRunning || project.status === "rendering"}
                onClick={() => setConfirmRewrite(true)}
              >
                {t("prompts.action")}
              </Button>
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
        {jobStrip("visual", t("visual.jobTitle"), startVisuals)}
        {realWorld && !anyRunning && toCheck > 0 && <Notice>{t("visual.toCheck", { n: toCheck })}</Notice>}
        {drawPending && <span className="t-small t-muted">{t("visual.drawing")}</span>}
        {rewriteStrip()}
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

      {continuousMode && activePlayingScene && (
        <VoicePlayerBar
          currentScene={activePlayingScene}
          currentIndex={currentVoiceSceneIndex}
          totalScenes={readyScenes.length}
          isPaused={isPaused}
          currentTime={audioTime.currentTime}
          duration={audioTime.duration}
          totalVoiceMs={totalVoiceMs}
          elapsedBeforeMs={elapsedBeforeMs}
          language={lang}
          autoScroll={autoScroll}
          onToggleAutoScroll={() => setAutoScroll((v) => !v)}
          onPlayPause={toggleContinuousPlay}
          onNext={() => stepContinuousScene(1)}
          onPrev={() => stepContinuousScene(-1)}
          onStop={stopContinuous}
          onSeek={seekAudio}
        />
      )}

      {scenes.length === 0 ? (
        <EmptyState icon={<Volume2 size={18} strokeWidth={1.5} />} title={t("scenes.empty")} body="" />
      ) : (
        groupByChapter(scenes).map((group) => (
          <section key={`${group.chapter}-${group.items[0]?.scene.id ?? ""}`} className="scene-chapter">
            <h2 className="scene-chapter__title">
              <span className="t-micro t-muted">{t("script.chapter", { n: group.chapter + 1 })}</span>
              <span className="t-h3" lang={lang}>
                {project.outline?.chapters[group.chapter]?.title ?? ""}
              </span>
              <span className="t-small">{t("scenes.chapterCount", { n: group.items.length })}</span>
            </h2>
            <div className="scene-grid">
              {group.items.map(({ scene: s, index: i }) => {
                const hasVoice =
                  s.voice.path !== null &&
                  (s.voice.status === "done" || s.voice.status === "failed" || s.voice.status === "stale");
                const source = scenes[sourceIndex(i, perImage)] ?? s;
                const shared = source.id !== s.id;
                const ai = !realWorld || needsAiImage(s);
                const thumbVersion = ai
                  ? s.image.thumb_path
                    ? `${s.image.thumb_path}#${s.image.prompt_hash ?? ""}`
                    : null
                  : s.visual.thumb_path
                    ? `${s.visual.thumb_path}#${s.visual.asset ?? ""}#${s.visual.focus?.join(",") ?? ""}#${s.visual.render_hash ?? ""}`
                    : null;
                const pick = s.visual.candidates.find((c) => assetId(c) === s.visual.asset);
                return (
                  <article
                    key={s.id}
                    id={`scene-${s.id}`}
                    className={`scene-card card ${playing === s.id ? "scene-card--playing" : ""}`}
                  >
                    <SceneThumb
                      projectId={projectId}
                      sceneId={s.id}
                      version={thumbVersion}
                      alt={t("images.thumbAlt", { n: s.order })}
                    >
                      <span className="thumb__meta t-micro">
                        {String(s.order).padStart(3, "0")} · {formatDuration(s.voice.duration_ms)}
                      </span>
                      {realWorld ? (
                        <span className="thumb__corner thumb__badges">
                          <Badge>{t(`visual.kind.${ai ? "ai_image" : s.visual.kind}`)}</Badge>
                          {!ai && s.visual.auto_picked && <Badge tone="warn">{t("visual.check")}</Badge>}
                          {!ai && pick?.low_res && <Badge tone="warn">{t("visual.lowRes")}</Badge>}
                          {!ai && pick && pick.restrictions.length > 0 && (
                            <span title={t("visual.restricted", { what: pick.restrictions.join(", ") })}>
                              <Badge tone="warn">
                                <AlertTriangle size={12} strokeWidth={2} aria-hidden="true" />
                              </Badge>
                            </span>
                          )}
                        </span>
                      ) : (
                        s.key_scene && (
                          <span className="thumb__corner">
                            <Badge>{t("images.keyScene")}</Badge>
                          </span>
                        )
                      )}
                    </SceneThumb>
                    {(s.voice.status !== "done" || (ai ? s.image.status !== "done" : !visualReady(s))) && (
                      <div className="scene-card__status">
                        <span className="scene-card__asset" title={t("scenes.voice")}>
                          <Volume2 size={12} strokeWidth={1.5} aria-label={t("scenes.voice")} />
                          <AssetStatusBadge status={s.voice.status} />
                        </span>
                        <span className="scene-card__asset" title={t("images.image")}>
                          <ImageIcon size={12} strokeWidth={1.5} aria-label={t("images.image")} />
                          <AssetStatusBadge status={ai ? s.image.status : s.visual.status} />
                        </span>
                      </div>
                    )}
                    {realWorld && s.visual.status === "failed" && s.visual.error && (
                      <span className="t-micro t-muted">{t("visual.fallback", { why: s.visual.error })}</span>
                    )}
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
                      {realWorld && (
                        <IconButton
                          label={t("visual.choose")}
                          icon={<Images {...ICON} />}
                          disabled={jobs.visual.running || project.status === "rendering"}
                          onClick={() => setVisualEdit(s.id)}
                        />
                      )}
                      {ai && (
                        <>
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
                        </>
                      )}
                    </div>
                  </article>
                );
              })}
            </div>
          </section>
        ))
      )}

      {confirmRewrite && (
        <ConfirmDialog
          title={t("prompts.confirmTitle")}
          body={t("prompts.confirmBody", { n: scenes.length })}
          confirmLabel={t("prompts.action")}
          onConfirm={() => void startRewrite()}
          onCancel={() => setConfirmRewrite(false)}
        />
      )}

      {aspectAsk && project && (
        <ConfirmDialog
          title={t("aspect.confirmTitle", { aspect: project.aspect === "16:9" ? "9:16" : "16:9" })}
          body={t("aspect.confirmBody", { images: scenes.length })}
          confirmLabel={t("aspect.confirmLabel", { aspect: project.aspect === "16:9" ? "9:16" : "16:9" })}
          onConfirm={() => {
            setAspectAsk(false);
            api
              .projectSetAspect(projectId, project.aspect === "16:9" ? "9:16" : "16:9")
              .then(setProject)
              .catch((e: unknown) => toast.show(errorMessage(e), "danger"));
          }}
          onCancel={() => setAspectAsk(false)}
        />
      )}

      {editing && (
        <VisualDialog
          project={project}
          scene={editing}
          onClose={() => setVisualEdit(null)}
          onChanged={(p) => {
            setProject(p);
            refreshEstimate();
          }}
        />
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
