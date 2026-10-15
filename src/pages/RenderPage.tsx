import { ArrowLeft, Upload } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { ProjectStatusBadge } from "../components/Badge";
import { Button, IconButton } from "../components/Button";
import { ExportDialog } from "../components/editor/ExportDialog";
import { PreviewPlayer, type PlayerApi } from "../components/editor/PreviewPlayer";
import { SubtitleStylePanel } from "../components/editor/SubtitleStylePanel";
import { Timeline } from "../components/editor/Timeline";
import { Notice } from "../components/Notice";
import { useToast } from "../components/toast";
import { useI18n } from "../i18n/context";
import { api, errorMessage } from "../lib/api";
import type { Navigate } from "../lib/route";
import { subtitleStates } from "../lib/subtitleStates";
import { sceneIndexAt, totalMs } from "../lib/timeline";
import type { Project, Settings, TimelineScene } from "../lib/types";

/** Keys handled by the editor unless the user is typing. */
function isTyping(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  return !!el && (el.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(el.tagName));
}

/** CapCut-style editor: live preview + timeline, then Export (FFmpeg render). */
export function RenderPage({ projectId, navigate }: { projectId: string; navigate: Navigate }) {
  const { t } = useI18n();
  const toast = useToast();
  const [project, setProject] = useState<Project | null>(null);
  const [scenes, setScenes] = useState<TimelineScene[]>([]);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [time, setTime] = useState(0);
  const [exportOpen, setExportOpen] = useState(false);
  const [resume, setResume] = useState(false);
  const player = useRef<PlayerApi | null>(null);

  const reload = useCallback(async () => {
    try {
      const [p, tl] = await Promise.all([api.projectGet(projectId), api.renderTimeline(projectId)]);
      setProject(p);
      setScenes(tl);
    } catch (e) {
      setLoadError(errorMessage(e));
    }
  }, [projectId]);

  useEffect(() => {
    Promise.all([api.projectGet(projectId), api.renderTimeline(projectId)])
      .then(([p, tl]) => {
        setProject(p);
        setScenes(tl);
      })
      .catch((e: unknown) => setLoadError(errorMessage(e)));
    api
      .settingsGet()
      .then(setSettings)
      .catch(() => setSettings(null));
    api
      .renderRunning(projectId)
      .then((running) => {
        if (running) {
          setResume(true);
          setExportOpen(true);
        }
      })
      .catch(() => undefined);
  }, [projectId]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (exportOpen || isTyping(e.target)) return;
      if (e.code === "Space") {
        e.preventDefault();
        player.current?.toggle();
      } else if (e.key === "ArrowLeft") {
        player.current?.stepScene(-1);
      } else if (e.key === "ArrowRight") {
        player.current?.stepScene(1);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [exportOpen]);

  const lang = project?.language ?? "km";
  const highlight = project?.render.subtitle_highlight ?? null;
  const states = useMemo(
    () =>
      scenes.map((sc) =>
        sc.voice_ready ? subtitleStates({ narration: sc.narration, duration_ms: sc.duration_ms, words: sc.words }, lang, highlight !== null) : [],
      ),
    [scenes, lang, highlight],
  );
  const total = totalMs(scenes);
  const onFinished = useCallback(() => void reload(), [reload]);

  const setHighlight = async (h: string | null) => {
    try {
      setProject(await api.renderSetSubtitleStyle(projectId, h));
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

  const voicesDone = scenes.filter((s) => s.voice_ready).length;
  const imagesDone = scenes.filter((s) => s.image_ready).length;
  const ready = scenes.length > 0 && voicesDone === scenes.length && imagesDone === scenes.length;
  const current = scenes[sceneIndexAt(scenes, time)];
  const rendering = project.status === "rendering";

  return (
    <div className="studio">
      <header className="studio__top">
        <IconButton
          label={t("editor.back")}
          icon={<ArrowLeft size={18} strokeWidth={1.5} />}
          onClick={() => {
            player.current?.pause();
            navigate({ name: "scenes", projectId });
          }}
        />
        <div className="studio__title">
          <span className="t-h3" lang={lang} title={project.title}>
            {project.title}
          </span>
          <ProjectStatusBadge status={project.status} />
        </div>
        <Button
          variant="primary"
          className="studio__export"
          icon={<Upload size={16} strokeWidth={1.5} />}
          disabled={!ready}
          title={ready ? undefined : t("render.notReady", { voices: voicesDone, images: imagesDone, total: scenes.length })}
          onClick={() => {
            player.current?.pause();
            setResume(false);
            setExportOpen(true);
          }}
        >
          {t("editor.export")}
        </Button>
      </header>

      <div className="studio__main">
        <div className="studio__preview">
          <PreviewPlayer
            projectId={projectId}
            scenes={scenes}
            states={states}
            highlight={highlight}
            resolution={project.render.resolution}
            audioMime={project.voice.engine === "gemini" ? "audio/wav" : "audio/mpeg"}
            apiRef={player}
            onTime={setTime}
          />
        </div>

        <aside className="studio__panel" aria-label={t("editor.properties")}>
          {!ready && (
            <Notice
              tone="warn"
              actions={
                <Button variant="pill" onClick={() => navigate({ name: "scenes", projectId })}>
                  {t("render.openScenes")}
                </Button>
              }
            >
              {t("render.notReady", { voices: voicesDone, images: imagesDone, total: scenes.length })}
            </Notice>
          )}
          <SubtitleStylePanel highlight={highlight} disabled={rendering} onChange={(h) => void setHighlight(h)} />
          {current && (
            <section className="panel-section" aria-label={t("editor.scene")}>
              <h3 className="panel-section__title">
                {t("script.scene", { n: current.order })}
                <span className="t-micro t-muted"> · {(current.clip_ms / 1000).toFixed(1)} s</span>
              </h3>
              <p className="panel-narration" lang={lang}>
                {current.narration}
              </p>
            </section>
          )}
          <p className="t-micro t-muted studio__keys">{t("editor.keys")}</p>
        </aside>
      </div>

      <div className="studio__timeline">
        <Timeline
          projectId={projectId}
          lang={lang}
          scenes={scenes}
          projectScenes={project.scenes}
          states={states}
          timeMs={time}
          totalMs={total}
          onSeek={(ms) => player.current?.seek(ms)}
        />
      </div>

      {exportOpen && (
        <ExportDialog
          project={project}
          settings={settings}
          totalMs={total}
          resume={resume}
          onClose={() => setExportOpen(false)}
          onFinished={onFinished}
        />
      )}
    </div>
  );
}
