import { FolderOpen, Play, Upload, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { Button, IconButton } from "../Button";
import { ErrorPanel } from "../ErrorPanel";
import { Notice } from "../Notice";
import { ProgressBar } from "../ProgressBar";
import { useToast } from "../toast";
import { useI18n } from "../../i18n/context";
import { api, errorMessage, onRenderProgress } from "../../lib/api";
import { prepareSubtitles } from "../../lib/prepareSubtitles";
import type { Project, RenderProgress, Settings } from "../../lib/types";
import { formatTime } from "../../lib/timeline";

type Phase =
  | { kind: "setup" }
  | { kind: "subs"; done: number; total: number }
  | { kind: "render"; progress: RenderProgress | null }
  | { kind: "done"; path: string }
  | { kind: "failed"; errorKind: string | null; error: string }
  | { kind: "cancelled" };

interface Props {
  project: Project;
  settings: Settings | null;
  totalMs: number;
  /** A render was already running when the editor opened. */
  resume: boolean;
  onClose: () => void;
  onFinished: () => void;
}

/** CapCut-style export: settings → progress with ETA → open the video. */
export function ExportDialog({ project, settings, totalMs, resume, onClose, onFinished }: Props) {
  const { t } = useI18n();
  const toast = useToast();
  const [phase, setPhase] = useState<Phase>(resume ? { kind: "render", progress: null } : { kind: "setup" });
  const cancelSubs = useRef(false);
  const projectId = project.id;
  const busy = phase.kind === "subs" || phase.kind === "render";

  useEffect(() => {
    const unlisten = onRenderProgress((p) => {
      if (p.project_id !== projectId) return;
      if (p.stage === "clips" || p.stage === "concat") setPhase({ kind: "render", progress: p });
      else if (p.stage === "done") setPhase({ kind: "done", path: p.output_path ?? "" });
      else if (p.stage === "cancelled") setPhase({ kind: "cancelled" });
      else setPhase({ kind: "failed", errorKind: p.error_kind, error: p.error ?? "" });
      if (p.stage !== "clips" && p.stage !== "concat") onFinished();
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [projectId, onFinished]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !busy) onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [busy, onClose]);

  const start = async () => {
    cancelSubs.current = false;
    try {
      setPhase({ kind: "subs", done: 0, total: 0 });
      await prepareSubtitles(
        projectId,
        project.language,
        project.render.resolution,
        project.render.subtitle_highlight,
        (done, total) => setPhase({ kind: "subs", done, total }),
        () => cancelSubs.current,
      );
      if (cancelSubs.current) {
        setPhase({ kind: "cancelled" });
        return;
      }
      setPhase({ kind: "render", progress: null });
      await api.renderStart(projectId);
    } catch (e) {
      setPhase({ kind: "failed", errorKind: null, error: errorMessage(e) });
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

  const label = (() => {
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
        ? (phase.done / phase.total) * 0.15
        : undefined
      : phase.kind === "render" && phase.progress
        ? 0.15 + phase.progress.percent * 0.85
        : undefined;
  const eta = phase.kind === "render" ? phase.progress?.eta_seconds : null;

  return (
    <div className="backdrop" onMouseDown={(e) => e.target === e.currentTarget && !busy && onClose()}>
      <div className="dialog export" role="dialog" aria-modal="true" aria-labelledby="export-title">
        <div className="export__head">
          <h2 id="export-title" className="t-h2">
            {t("editor.export")}
          </h2>
          {!busy && <IconButton label={t("common.close")} icon={<X size={16} strokeWidth={1.5} />} onClick={onClose} />}
        </div>

        <dl className="render-facts">
          <dt>{t("render.title")}</dt>
          <dd lang={project.language}>{project.title}</dd>
          <dt>{t("render.resolution")}</dt>
          <dd>1920 × 1080 · 30 fps · H.264 / AAC</dd>
          <dt>{t("render.length")}</dt>
          <dd>{formatTime(totalMs)}</dd>
          <dt>{t("render.encoder")}</dt>
          <dd>{project.render.encoder ?? settings?.render.encoder ?? t("render.encoderAuto")}</dd>
          <dt>{t("render.output")}</dt>
          <dd className="path">{settings?.output_dir ?? t("settings.output.default")}</dd>
        </dl>

        {busy && (
          <div className="export__progress">
            <div className="row export__status">
              <span className="t-small">{label}</span>
              <span className="t-small t-muted">
                {fraction !== undefined && `${Math.round(fraction * 100)} %`}
                {eta != null && ` · ${t("render.eta", { time: formatTime(eta * 1000) })}`}
              </span>
            </div>
            <ProgressBar label={label || t("render.progress")} value={fraction} />
            <span className="t-small t-muted">{t("render.lowPriority")}</span>
          </div>
        )}
        {phase.kind === "failed" && <ErrorPanel kind={phase.errorKind} details={phase.error} onRetry={() => void start()} />}
        {phase.kind === "cancelled" && <Notice tone="warn">{t("render.cancelled")}</Notice>}
        {phase.kind === "done" && (
          <Notice tone="ok">
            <strong>{t("render.done")}</strong>
            <span className="path"> {phase.path}</span>
          </Notice>
        )}

        <div className="dialog__actions">
          {busy ? (
            <Button onClick={cancel}>{t("common.cancel")}</Button>
          ) : phase.kind === "done" ? (
            <>
              <Button icon={<FolderOpen size={16} strokeWidth={1.5} />} onClick={() => void open(true)}>
                {t("render.showFolder")}
              </Button>
              <Button variant="primary" icon={<Play size={16} strokeWidth={1.5} />} onClick={() => void open(false)}>
                {t("render.openVideo")}
              </Button>
            </>
          ) : (
            <>
              <Button onClick={onClose}>{t("common.cancel")}</Button>
              <Button variant="primary" icon={<Upload size={16} strokeWidth={1.5} />} onClick={() => void start()}>
                {phase.kind === "setup" ? t("editor.export") : t("common.tryAgain")}
              </Button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
