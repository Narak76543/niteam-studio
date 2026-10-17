import {
  ArrowDown,
  ArrowUp,
  BookOpenText,
  CircleAlert,
  GripVertical,
  ListPlus,
  PencilLine,
  RefreshCw,
  ScrollText,
  Sparkles,
  SpellCheck,
  Square,
  Trash2,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { Badge, ProjectStatusBadge } from "../components/Badge";
import { Button, IconButton } from "../components/Button";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { EmptyState } from "../components/EmptyState";
import { ErrorPanel } from "../components/ErrorPanel";
import { KeepTermsEditor } from "../components/KeepTermsEditor";
import { Notice } from "../components/Notice";
import { PageHeader } from "../components/PageHeader";
import { SceneThumb } from "../components/SceneThumb";
import { SourceReview } from "../components/SourceReview";
import { ProgressBar } from "../components/ProgressBar";
import { RetryStatus } from "../components/RetryStatus";
import { SuggestionCard } from "../components/SuggestionCard";
import { useToast } from "../components/toast";
import { VisualBibleEditor } from "../components/VisualBibleEditor";
import { useI18n } from "../i18n/context";
import { api, errorMessage, onGeminiRetry, onProofreadProgress, onScriptProgress } from "../lib/api";
import type { Navigate } from "../lib/route";
import { pageRanges } from "../lib/sourceReview";
import {
  MOODS,
  SHOTS,
  type GeminiRetry,
  type ProofProgress,
  type Project,
  type Scene,
  type SceneEdit,
  type ScriptProgress,
  type Shot,
} from "../lib/types";

const SAVE_DELAY_MS = 600;
const ICON = { size: 16, strokeWidth: 1.5 } as const;

type Tab = "narration" | "spoken";
type View = "scenes" | "look" | "names";
type SaveState = "idle" | "saving" | "saved";
type Confirm =
  | { kind: "regenerate" }
  | { kind: "deleteScene"; id: string; order: number }
  | { kind: "backToPoints" }
  | null;
type Finished = Pick<ScriptProgress, "stage" | "error" | "error_kind"> | null;
type Retry = { notice: GeminiRetry; at: number } | null;

function toEdit(s: Scene): SceneEdit {
  return {
    id: s.id,
    narration: s.narration,
    spoken_text: s.spoken_text,
    image_prompt: s.image_prompt,
    mood: s.mood,
    visual: { characters: s.characters, place: s.place, shot: s.shot },
  };
}

function toShot(v: string): Shot {
  return SHOTS.find((s) => s === v) ?? "medium";
}

export function ScriptPage({ projectId, navigate }: { projectId: string; navigate: Navigate }) {
  const { t } = useI18n();
  const toast = useToast();

  const [project, setProject] = useState<Project | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<ScriptProgress | null>(null);
  const [finished, setFinished] = useState<Finished>(null);
  const [retry, setRetry] = useState<Retry>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [draftState, setDraftState] = useState<SceneEdit | null>(null);
  const [tab, setTab] = useState<Tab>("narration");
  const [view, setView] = useState<View>("scenes");
  const [proofRunning, setProofRunning] = useState(false);
  const [proof, setProof] = useState<ProofProgress | null>(null);
  const [saveState, setSaveState] = useState<SaveState>("idle");
  const [confirm, setConfirm] = useState<Confirm>(null);
  const [dragId, setDragId] = useState<string | null>(null);
  const [overId, setOverId] = useState<string | null>(null);

  const timer = useRef<number | null>(null);
  const pending = useRef<SceneEdit | null>(null);

  const reload = useCallback(async () => {
    try {
      setProject(await api.projectGet(projectId));
    } catch (e) {
      setLoadError(errorMessage(e));
    }
  }, [projectId]);

  // Initial load + running state.
  useEffect(() => {
    api
      .projectGet(projectId)
      .then(setProject)
      .catch((e: unknown) => setLoadError(errorMessage(e)));
    api
      .scriptRunning(projectId)
      .then(setRunning)
      .catch(() => setRunning(false));
    api
      .proofreadRunning(projectId)
      .then(setProofRunning)
      .catch(() => setProofRunning(false));
  }, [projectId]);

  // Grammar check progress; suggestions appear as each batch finishes.
  useEffect(() => {
    const unlisten = onProofreadProgress((p) => {
      if (p.project_id !== projectId) return;
      setProof(p);
      setProofRunning(p.stage === "running");
      if (p.stage !== "running") setRetry(null);
      void reload();
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [projectId, reload]);

  // Live progress from the Rust job.
  useEffect(() => {
    const unlisten = onScriptProgress((p) => {
      if (p.project_id !== projectId) return;
      setProgress(p);
      setRetry(null);
      if (p.stage === "outline" || p.stage === "chapter") {
        setRunning(true);
        setFinished(null);
      } else {
        setRunning(false);
        setFinished({ stage: p.stage, error: p.error, error_kind: p.error_kind });
      }
      void reload();
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [projectId, reload]);

  // "Google is busy, retrying (attempt 2/5)…" while the job waits.
  useEffect(() => {
    const unlisten = onGeminiRetry((r) => {
      if (r.scope === "script" && r.project_id === projectId) setRetry({ notice: r, at: Date.now() });
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [projectId]);

  // ---- saving ----
  const save = useCallback(
    async (edit: SceneEdit) => {
      setSaveState("saving");
      try {
        const p = await api.sceneUpdate(projectId, edit);
        setProject(p);
        setSaveState("saved");
      } catch (e) {
        setSaveState("idle");
        toast.show(errorMessage(e), "danger");
      }
    },
    [projectId, toast],
  );

  const flush = useCallback(async () => {
    if (timer.current !== null) {
      window.clearTimeout(timer.current);
      timer.current = null;
    }
    const edit = pending.current;
    pending.current = null;
    if (edit) await save(edit);
  }, [save]);

  // Save anything pending when leaving the page.
  useEffect(() => () => void flush(), [flush]);

  // ---- selection ----
  // Derived: the chosen scene if it still exists, else the first one.
  const scenes = useMemo(() => project?.scenes ?? [], [project]);
  const selected = scenes.find((s) => s.id === selectedId) ?? scenes[0] ?? null;
  // The unsaved draft wins over stored text for the selected scene.
  const draft = selected ? (draftState?.id === selected.id ? draftState : toEdit(selected)) : null;

  const updateDraft = (patch: Partial<SceneEdit>) => {
    if (!draft) return;
    const next = { ...draft, ...patch };
    setDraftState(next);
    pending.current = next;
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      timer.current = null;
      void flush();
    }, SAVE_DELAY_MS);
  };

  const select = async (id: string) => {
    if (id === selected?.id) return;
    await flush();
    setSelectedId(id);
    setDraftState(null);
    setSaveState("idle");
  };

  // ---- actions ----
  const run = async (fn: () => Promise<Project | null>) => {
    try {
      await flush();
      const p = await fn();
      if (p) setProject(p);
    } catch (e) {
      toast.show(errorMessage(e), "danger");
    }
  };

  /** Approve → start voice generation → go to the Scenes page. */
  const approve = async () => {
    try {
      await flush();
      setProject(await api.scriptApprove(projectId));
    } catch (e) {
      toast.show(errorMessage(e), "danger");
      return;
    }
    await api.generateStart(projectId).catch((e: unknown) => toast.show(errorMessage(e), "danger"));
    navigate({ name: "scenes", projectId });
  };

  const generate = (restart: boolean) =>
    run(async () => {
      setFinished(null);
      setProgress(null);
      setRetry(null);
      await api.scriptGenerate(projectId, restart);
      setRunning(true);
      return null;
    });

  const addScene = () =>
    run(async () => {
      const before = new Set(scenes.map((s) => s.id));
      const p = await api.sceneAdd(projectId, selected?.id ?? null);
      const added = p.scenes.find((s) => !before.has(s.id));
      if (added) {
        setSelectedId(added.id);
        setDraftState(null);
      }
      return p;
    });

  const deleteScene = (id: string) => run(() => api.sceneDelete(projectId, id));

  const checkGrammar = () =>
    run(async () => {
      setProof(null);
      setRetry(null);
      await api.proofreadStart(projectId);
      setProofRunning(true);
      return null;
    });

  /** Accepting replaces the scene text, so any unsaved draft of it is dropped. */
  const acceptSuggestions = async (ids: string[]) => {
    await run(() => api.proofreadAccept(projectId, ids));
    setDraftState(null);
  };

  const dismissSuggestions = (ids: string[]) => run(() => api.proofreadDismiss(projectId, ids));

  const reorderTo = (ids: string[]) => run(() => api.scenesReorder(projectId, ids));

  const move = (id: string, delta: number) => {
    const ids = scenes.map((s) => s.id);
    const from = ids.indexOf(id);
    const to = from + delta;
    if (from < 0 || to < 0 || to >= ids.length) return;
    ids.splice(from, 1);
    ids.splice(to, 0, id);
    void reorderTo(ids);
  };

  const dropOn = (targetId: string) => {
    if (!dragId || dragId === targetId) return;
    const ids = scenes.map((s) => s.id).filter((i) => i !== dragId);
    const at = ids.indexOf(targetId);
    ids.splice(at < 0 ? ids.length : at, 0, dragId);
    void reorderTo(ids);
  };

  // ---- render ----
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

  // "From document": reading and point review come before the script.
  if (project.source && project.source.stage !== "confirmed") {
    return (
      <div className="page">
        <PageHeader
          breadcrumb={[{ label: t("script.breadcrumb"), onClick: () => navigate({ name: "projects" }) }]}
          title={project.title}
          titleLang={project.language}
        />
        <SourceReview
          key={project.source.stage}
          project={project}
          onProject={setProject}
          onConfirmed={(p) => {
            setProject(p);
            void generate(false);
          }}
        />
      </div>
    );
  }

  const editable = project.status === "script_draft";
  const suggestionIds = scenes.filter((s) => s.proofread !== null).map((s) => s.id);
  const outline = project.outline;
  const chaptersTotal = outline?.chapters.length ?? 0;
  const chaptersDone = outline?.chapters.filter((c) => c.done).length ?? 0;
  const incomplete = outline !== null && chaptersDone < chaptersTotal;
  const complete = outline !== null && !incomplete;
  const hasScenes = scenes.length > 0;
  const lang = project.language;

  const headerActions = (() => {
    if (!editable) {
      return (
        <Button icon={<PencilLine {...ICON} />} onClick={() => void run(() => api.scriptReopen(projectId))}>
          {t("script.edit")}
        </Button>
      );
    }
    if (running) {
      return (
        <Button icon={<Square {...ICON} />} onClick={() => void api.scriptCancel(projectId)}>
          {t("script.cancelGeneration")}
        </Button>
      );
    }
    if (proofRunning) {
      return (
        <Button icon={<Square {...ICON} />} onClick={() => void api.proofreadCancel(projectId)}>
          {t("proof.stop")}
        </Button>
      );
    }
    return (
      <>
        {hasScenes && !incomplete && (
          <Button icon={<SpellCheck {...ICON} />} onClick={() => void checkGrammar()}>
            {t("proof.check")}
          </Button>
        )}
        {hasScenes && (
          <Button icon={<ListPlus {...ICON} />} onClick={() => void addScene()}>
            {t("script.addScene")}
          </Button>
        )}
        {project.source && (
          <Button icon={<BookOpenText {...ICON} />} onClick={() => setConfirm({ kind: "backToPoints" })}>
            {t("source.backToPoints")}
          </Button>
        )}
        {(complete || (hasScenes && !outline)) && (
          <Button icon={<RefreshCw {...ICON} />} onClick={() => setConfirm({ kind: "regenerate" })}>
            {t("script.regenerate")}
          </Button>
        )}
        {incomplete && (
          <Button variant="primary" icon={<Sparkles {...ICON} />} onClick={() => void generate(false)}>
            {t("script.continue")}
          </Button>
        )}
        {!outline && !hasScenes && (
          <Button variant="primary" icon={<Sparkles {...ICON} />} onClick={() => void generate(false)}>
            {t("script.generate")}
          </Button>
        )}
        {hasScenes && !incomplete && (
          <Button variant="primary" onClick={() => void approve()}>
            {t("script.approve")}
          </Button>
        )}
      </>
    );
  })();

  const progressLabel =
    progress?.stage === "chapter"
      ? t("script.progress.chapter", { n: progress.chapters_done + 1, total: progress.chapters_total })
      : t("script.progress.outline");

  return (
    <div className="page page--split">
      <PageHeader
        breadcrumb={[{ label: t("script.breadcrumb"), onClick: () => navigate({ name: "projects" }) }]}
        title={project.title}
        titleLang={lang}
        meta={
          <div className="row">
            <ProjectStatusBadge status={project.status} />
            <span className="t-small">
              {t("script.length", {
                minutes: project.target_minutes,
                scenes: scenes.length,
                lang: t(`lang.${lang}`),
              })}
            </span>
          </div>
        }
        actions={headerActions}
      />

      <div className="status-strip">
        {running && (
          <>
            <span className="t-small">{progressLabel}</span>
            <ProgressBar
              label={progressLabel}
              value={
                progress?.stage === "chapter" && progress.chapters_total > 0
                  ? progress.chapters_done / progress.chapters_total
                  : undefined
              }
            />
            {retry && <RetryStatus key={retry.at} retry={retry.notice} receivedAt={retry.at} />}
          </>
        )}
        {!running && finished?.stage === "failed" && (
          <ErrorPanel
            kind={finished.error_kind}
            details={finished.error}
            onRetry={() => void generate(false)}
          />
        )}
        {proofRunning && (
          <>
            <span className="t-small">{t("proof.progress", { done: proof?.checked ?? 0, total: proof?.total ?? 0 })}</span>
            <ProgressBar
              label={t("proof.check")}
              value={proof && proof.total > 0 ? proof.checked / proof.total : undefined}
            />
            {retry && <RetryStatus key={retry.at} retry={retry.notice} receivedAt={retry.at} />}
          </>
        )}
        {!proofRunning && proof?.stage === "failed" && (
          <ErrorPanel kind={proof.error_kind} details={proof.error} onRetry={() => void checkGrammar()} />
        )}
        {!proofRunning && proof?.stage === "cancelled" && <Notice tone="warn">{t("proof.cancelled")}</Notice>}
        {!proofRunning && proof?.stage === "done" && suggestionIds.length === 0 && (
          <Notice tone="ok">{proof.total === 0 ? t("proof.nothingNew") : t("proof.allGood")}</Notice>
        )}
        {editable && suggestionIds.length > 0 && (
          <Notice
            tone="warn"
            actions={
              <>
                <Button variant="pill" onClick={() => void acceptSuggestions(suggestionIds)}>
                  {t("proof.acceptAll")}
                </Button>
                <Button variant="pill" onClick={() => void dismissSuggestions(suggestionIds)}>
                  {t("proof.dismissAll")}
                </Button>
              </>
            }
          >
            {t("proof.found", { n: suggestionIds.length })}
          </Notice>
        )}
        {!running && incomplete && finished?.stage !== "failed" && (
          <Notice tone="warn">{t("script.stopped", { done: chaptersDone, total: chaptersTotal })}</Notice>
        )}
        {!editable && (
          <Notice
            tone="ok"
            actions={
              <Button variant="pill" onClick={() => navigate({ name: "scenes", projectId })}>
                {t("script.openScenes")}
              </Button>
            }
          >
            <strong>{t("script.approved")}</strong> — {t("script.approvedBody")}
          </Notice>
        )}
      </div>

      {(hasScenes || project.visual_bible) && (
        <div className="tabs view-tabs" role="tablist">
          <button role="tab" className="tab" aria-selected={view === "scenes"} onClick={() => setView("scenes")}>
            {t("script.scenes")}
          </button>
          <button
            role="tab"
            className="tab"
            aria-selected={view === "look"}
            onClick={() => {
              void flush();
              setView("look");
            }}
          >
            {t("look.title")}
          </button>
          <button
            role="tab"
            className="tab"
            aria-selected={view === "names"}
            onClick={() => {
              void flush();
              setView("names");
            }}
          >
            {t("names.title")}
            {project.keep_terms.length > 0 && <span className="t-micro t-muted"> · {project.keep_terms.length}</span>}
          </button>
        </div>
      )}

      {view === "look" && (hasScenes || project.visual_bible) ? (
        <VisualBibleEditor key={project.id} project={project} onChange={setProject} />
      ) : view === "names" && (hasScenes || project.visual_bible) ? (
        <KeepTermsEditor project={project} onChange={setProject} />
      ) : !hasScenes ? (
        !running && (
          <EmptyState
            icon={<ScrollText size={18} strokeWidth={1.5} />}
            title={t("script.empty.title")}
            body={t("script.empty.body")}
          />
        )
      ) : (
        <div className="split">
          <nav className="scene-list" aria-label={t("script.scenes")}>
            {scenes.map((s, i) => {
              const prev = scenes[i - 1];
              const showChapter = !prev || prev.chapter !== s.chapter;
              const chapterTitle = outline?.chapters[s.chapter]?.title;
              const rowCls = [
                "scene-row",
                dragId === s.id ? "scene-row--dragging" : "",
                overId === s.id && dragId !== s.id ? "scene-row--drop-target" : "",
              ].join(" ");
              return (
                <div key={s.id}>
                  {showChapter && (
                    <div className="scene-list__chapter" lang={lang}>
                      {t("script.chapter", { n: s.chapter + 1 })}
                      {chapterTitle ? ` · ${chapterTitle}` : ""}
                    </div>
                  )}
                  <button
                    className={rowCls}
                    aria-current={s.id === selected?.id}
                    onClick={() => void select(s.id)}
                    draggable={editable}
                    onDragStart={() => setDragId(s.id)}
                    onDragEnd={() => {
                      setDragId(null);
                      setOverId(null);
                    }}
                    onDragOver={(e) => {
                      if (!dragId) return;
                      e.preventDefault();
                      setOverId(s.id);
                    }}
                    onDrop={(e) => {
                      e.preventDefault();
                      dropOn(s.id);
                      setDragId(null);
                      setOverId(null);
                    }}
                  >
                    {editable && (
                      <span className="scene-row__handle" title={t("script.dragHint")} aria-hidden="true">
                        <GripVertical size={14} strokeWidth={1.5} />
                      </span>
                    )}
                    <span className="scene-row__num t-small">{String(s.order).padStart(3, "0")}</span>
                    <span className="scene-row__text" lang={lang}>
                      {(s.id === draft?.id ? draft.narration : s.narration) || (
                        <span className="t-muted">{t("script.emptyField")}</span>
                      )}
                    </span>
                    {s.proofread && (
                      <span className="scene-row__flag scene-row__flag--proof" title={t("proof.suggestion")}>
                        <SpellCheck size={14} strokeWidth={1.5} aria-label={t("proof.suggestion")} />
                      </span>
                    )}
                    {s.fact_note && (
                      <span className="scene-row__flag" title={t("script.factNote")}>
                        <CircleAlert size={14} strokeWidth={1.5} aria-label={t("script.factNote")} />
                      </span>
                    )}
                  </button>
                </div>
              );
            })}
          </nav>

          {selected && draft ? (
            <section className="editor" aria-label={t("script.scene", { n: selected.order })}>
              <div className="editor__toolbar">
                <div className="row">
                  <h2 className="t-h2">{t("script.scene", { n: selected.order })}</h2>
                  <span className="t-small">{t("script.chapter", { n: selected.chapter + 1 })}</span>
                  {saveState !== "idle" && (
                    <span className="t-micro t-muted">
                      {saveState === "saving" ? t("common.saving") : t("common.saved")}
                    </span>
                  )}
                </div>
                {editable && (
                  <div className="editor__tools">
                    <IconButton
                      label={t("script.moveUp")}
                      icon={<ArrowUp {...ICON} />}
                      disabled={selected.order <= 1}
                      onClick={() => move(selected.id, -1)}
                    />
                    <IconButton
                      label={t("script.moveDown")}
                      icon={<ArrowDown {...ICON} />}
                      disabled={selected.order >= scenes.length}
                      onClick={() => move(selected.id, 1)}
                    />
                    <IconButton
                      label={t("script.addAfter")}
                      icon={<ListPlus {...ICON} />}
                      onClick={() => void addScene()}
                    />
                    <IconButton
                      label={t("script.deleteScene")}
                      icon={<Trash2 {...ICON} />}
                      onClick={() => setConfirm({ kind: "deleteScene", id: selected.id, order: selected.order })}
                    />
                  </div>
                )}
              </div>

              {selected.proofread && editable && (
                <SuggestionCard
                  before={{ narration: draft.narration, spoken_text: draft.spoken_text }}
                  suggestion={selected.proofread}
                  lang={lang}
                  onAccept={() => void acceptSuggestions([selected.id])}
                  onDismiss={() => void dismissSuggestions([selected.id])}
                />
              )}

              {project.source && (
                <div className="row t-small">
                  {selected.general_knowledge ? (
                    <Badge tone="info">{t("source.generalKnowledge")}</Badge>
                  ) : selected.source_pages.length > 0 ? (
                    <span>{t("source.scenePages", { pages: pageRanges(selected.source_pages) })}</span>
                  ) : (
                    <Badge tone="warn">{t("source.noPages")}</Badge>
                  )}
                </div>
              )}

              {selected.fact_note && (
                <Notice tone="warn">
                  <strong>{t("script.factNote")}:</strong> <span lang={lang}>{selected.fact_note}</span>
                </Notice>
              )}

              <div className="field">
                <div className="tabs" role="tablist">
                  <button
                    role="tab"
                    className="tab"
                    aria-selected={tab === "narration"}
                    onClick={() => setTab("narration")}
                  >
                    {t("script.tab.narration")}
                  </button>
                  <button
                    role="tab"
                    className="tab"
                    aria-selected={tab === "spoken"}
                    onClick={() => setTab("spoken")}
                  >
                    {t("script.tab.spoken")}
                  </button>
                </div>
                {tab === "narration" ? (
                  <>
                    <textarea
                      className="textarea"
                      lang={lang}
                      aria-label={t("script.tab.narration")}
                      value={draft.narration}
                      readOnly={!editable}
                      onChange={(e) => updateDraft({ narration: e.target.value })}
                    />
                    <span className="t-small">{t("script.narrationHint")}</span>
                  </>
                ) : (
                  <>
                    <textarea
                      className="textarea"
                      lang={lang}
                      aria-label={t("script.tab.spoken")}
                      value={draft.spoken_text}
                      readOnly={!editable}
                      onChange={(e) => updateDraft({ spoken_text: e.target.value })}
                    />
                    <span className="t-small">{t("script.spokenHint")}</span>
                  </>
                )}
              </div>

              <label className="field">
                <span className="field__label">{t("script.imagePrompt")}</span>
                <div className={selected.image.thumb_path ? "prompt-with-image" : undefined}>
                  <textarea
                    className="textarea"
                    lang="en"
                    value={draft.image_prompt}
                    readOnly={!editable}
                    onChange={(e) => updateDraft({ image_prompt: e.target.value })}
                  />
                  {selected.image.thumb_path && (
                    <SceneThumb
                      projectId={projectId}
                      sceneId={selected.id}
                      version={`${selected.image.thumb_path}#${selected.image.prompt_hash ?? ""}`}
                      alt={t("images.thumbAlt", { n: selected.order })}
                    />
                  )}
                </div>
                <span className="t-small">{t("script.imagePromptHint")}</span>
              </label>

              {project.visual_bible && draft.visual && (
                <div className="field">
                  <span className="field__label">{t("script.picture")}</span>
                  <div className="models-grid">
                    <label className="field">
                      <span className="t-small">{t("script.shot")}</span>
                      <select
                        className="select"
                        value={draft.visual.shot}
                        disabled={!editable}
                        onChange={(e) => draft.visual && updateDraft({ visual: { ...draft.visual, shot: toShot(e.target.value) } })}
                      >
                        {SHOTS.map((sh) => (
                          <option key={sh} value={sh}>
                            {t(`shot.${sh}`)}
                          </option>
                        ))}
                      </select>
                    </label>
                    <label className="field">
                      <span className="t-small">{t("script.place")}</span>
                      <select
                        className="select"
                        lang="en"
                        value={draft.visual.place ?? ""}
                        disabled={!editable}
                        onChange={(e) =>
                          draft.visual && updateDraft({ visual: { ...draft.visual, place: e.target.value || null } })
                        }
                      >
                        <option value="">{t("script.noPlace")}</option>
                        {Object.keys(project.visual_bible.places).map((k) => (
                          <option key={k} value={k}>
                            {k}
                          </option>
                        ))}
                      </select>
                    </label>
                  </div>
                  {Object.keys(project.visual_bible.characters).length > 0 && (
                    <div className="row" role="group" aria-label={t("script.characters")}>
                      <span className="t-small">{t("script.characters")}:</span>
                      {Object.keys(project.visual_bible.characters).map((k) => {
                        const on = draft.visual?.characters.includes(k) ?? false;
                        return (
                          <button
                            key={k}
                            type="button"
                            className="btn btn--pill"
                            lang="en"
                            aria-pressed={on}
                            disabled={!editable}
                            onClick={() => {
                              if (!draft.visual) return;
                              const characters = on
                                ? draft.visual.characters.filter((c) => c !== k)
                                : [...draft.visual.characters, k];
                              updateDraft({ visual: { ...draft.visual, characters } });
                            }}
                          >
                            {k}
                          </button>
                        );
                      })}
                    </div>
                  )}
                  {selected.key_scene && <span className="t-small">{t("script.keyScene")}</span>}
                </div>
              )}

              <label className="field">
                <span className="field__label">{t("script.mood")}</span>
                <select
                  className="select"
                  value={draft.mood}
                  disabled={!editable}
                  onChange={(e) => updateDraft({ mood: e.target.value })}
                >
                  {MOODS.map((m) => (
                    <option key={m} value={m}>
                      {t(`mood.${m}`)}
                    </option>
                  ))}
                </select>
              </label>
            </section>
          ) : (
            <p className="t-small">{t("script.selectScene")}</p>
          )}
        </div>
      )}

      {confirm?.kind === "regenerate" && (
        <ConfirmDialog
          title={t("script.regenerateTitle")}
          body={t("script.regenerateBody")}
          confirmLabel={t("script.regenerate")}
          danger
          onConfirm={() => {
            setConfirm(null);
            pending.current = null;
            void generate(true);
          }}
          onCancel={() => setConfirm(null)}
        />
      )}
      {confirm?.kind === "backToPoints" && (
        <ConfirmDialog
          title={t("source.backTitle")}
          body={t("source.backBody")}
          confirmLabel={t("source.backToPoints")}
          danger={hasScenes}
          onConfirm={() => {
            setConfirm(null);
            pending.current = null;
            void run(() => api.sourceReopen(projectId));
          }}
          onCancel={() => setConfirm(null)}
        />
      )}
      {confirm?.kind === "deleteScene" && (
        <ConfirmDialog
          title={t("script.deleteSceneTitle", { n: confirm.order })}
          body=""
          confirmLabel={t("common.delete")}
          danger
          onConfirm={() => {
            const id = confirm.id;
            setConfirm(null);
            void deleteScene(id);
          }}
          onCancel={() => setConfirm(null)}
        />
      )}
    </div>
  );
}
