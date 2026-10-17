import { ArrowDown, ArrowUp, BookOpenText, Check, FileText, RefreshCw, Square } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";

import { Badge } from "./Badge";
import { Button, IconButton } from "./Button";
import { ConfirmDialog } from "./ConfirmDialog";
import { ErrorPanel } from "./ErrorPanel";
import { Notice } from "./Notice";
import { ProgressBar } from "./ProgressBar";
import { RetryStatus } from "./RetryStatus";
import { useToast } from "./toast";
import { useI18n } from "../i18n/context";
import { api, errorMessage, onGeminiRetry, onSourceProgress } from "../lib/api";
import { moveKeyPoint, notesForPage, pageRanges, toReviewEdit, updateItem } from "../lib/sourceReview";
import type { GeminiRetry, Project, SourceItem, SourceProgress, SourceSummary } from "../lib/types";

const ICON = { size: 16, strokeWidth: 1.5 } as const;
const SAVE_DELAY_MS = 600;
/** Roughly how much video one key point fills. */
const SECONDS_PER_POINT = 25;

interface Props {
  project: Project;
  onProject: (p: Project) => void;
  /** Points confirmed: the page starts writing the script. */
  onConfirmed: (p: Project) => void;
}

/** "From document": reading progress / quota pause, then the key-point review (10 §6).
 *  Render with `key={source.stage}` so a finished reading starts the review fresh. */
export function SourceReview({ project, onProject, onConfirmed }: Props) {
  const { t } = useI18n();
  const toast = useToast();
  const source = project.source;
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<SourceProgress | null>(null);
  const [retry, setRetry] = useState<{ notice: GeminiRetry; at: number } | null>(null);
  const [summary, setSummary] = useState<SourceSummary | null>(source?.summary ?? null);
  const [openPage, setOpenPage] = useState<number | null>(null);
  const [noteOpen, setNoteOpen] = useState<string | null>(null);
  const [confirmReread, setConfirmReread] = useState(false);
  const [busy, setBusy] = useState(false);
  const timer = useRef<number | null>(null);
  const id = project.id;

  useEffect(() => {
    api
      .sourceReadRunning(id)
      .then(setRunning)
      .catch(() => setRunning(false));
    const unlisten = onSourceProgress((p) => {
      if (p.project_id !== id) return;
      setProgress(p);
      setRetry(null);
      const live = p.stage === "reading" || p.stage === "summarizing";
      setRunning(live);
      if (!live) void api.projectGet(id).then(onProject);
    });
    const unretry = onGeminiRetry((n) => {
      if (n.scope === "source" && n.project_id === id) setRetry({ notice: n, at: Date.now() });
    });
    return () => {
      void unlisten.then((fn) => fn());
      void unretry.then((fn) => fn());
    };
  }, [id, onProject]);

  const read = async () => {
    try {
      setProgress(null);
      await api.sourceRead(id);
      setRunning(true);
    } catch (e) {
      toast.show(errorMessage(e), "danger");
    }
  };

  /** Edits are saved after a short pause, so leaving the page keeps them. */
  const change = useCallback(
    (next: SourceSummary) => {
      setSummary(next);
      if (timer.current !== null) window.clearTimeout(timer.current);
      timer.current = window.setTimeout(() => {
        api.sourceReviewSave(id, toReviewEdit(next)).catch((e: unknown) => toast.show(errorMessage(e), "danger"));
      }, SAVE_DELAY_MS);
    },
    [id, toast],
  );

  const confirm = async () => {
    if (!summary) return;
    if (timer.current !== null) window.clearTimeout(timer.current);
    setBusy(true);
    try {
      onConfirmed(await api.sourceConfirm(id, toReviewEdit(summary)));
    } catch (e) {
      toast.show(errorMessage(e), "danger");
    } finally {
      setBusy(false);
    }
  };

  const reread = async () => {
    setConfirmReread(false);
    try {
      onProject(await api.sourceReread(id));
      await read();
    } catch (e) {
      toast.show(errorMessage(e), "danger");
    }
  };

  if (!source) return null;
  const lang = project.language;
  const sections = source.chunks.flatMap((c) => c.sections);
  const chunksDone = source.chunks.filter((c) => c.done).length;
  const pause = source.pause;

  // ---- reading ----
  if (source.stage === "reading" || !summary) {
    const label =
      progress?.stage === "summarizing"
        ? t("source.summarizing")
        : t("source.reading", { done: chunksDone, total: source.chunks.length });
    return (
      <div className="card source">
        <div className="row">
          <FileText {...ICON} aria-hidden="true" />
          <strong>{source.file_name}</strong>
          <span className="t-small t-muted">
            {t("source.pagesRead", { from: source.page_from, to: source.page_to, total: source.page_count })}
          </span>
        </div>
        {running ? (
          <>
            <span className="t-small">{label}</span>
            <ProgressBar label={label} value={source.chunks.length ? chunksDone / source.chunks.length : undefined} />
            {retry && <RetryStatus key={retry.at} retry={retry.notice} receivedAt={retry.at} />}
            <div className="row">
              <Button icon={<Square {...ICON} />} onClick={() => void api.sourceReadCancel(id)}>
                {t("source.stop")}
              </Button>
            </div>
          </>
        ) : pause ? (
          <Notice
            tone="warn"
            actions={
              <Button variant="primary" onClick={() => void read()}>
                {t("source.resume")}
              </Button>
            }
          >
            {t(pause.daily ? "source.pausedDaily" : "source.pausedMinute", {
              time: new Date(pause.resets_at_utc).toLocaleString(),
              done: chunksDone,
              total: source.chunks.length,
            })}
          </Notice>
        ) : progress?.stage === "failed" ? (
          <ErrorPanel kind={progress.error_kind} details={progress.error} onRetry={() => void read()} />
        ) : (
          <div className="row">
            <Button variant="primary" icon={<BookOpenText {...ICON} />} onClick={() => void read()}>
              {chunksDone > 0 ? t("source.continue") : t("source.read")}
            </Button>
            <span className="t-small t-muted">{t("source.readHint")}</span>
          </div>
        )}
      </div>
    );
  }

  // ---- review ----
  const selected = summary.key_points.filter((k) => k.selected).length;
  const minutes = Math.max(1, Math.round((selected * SECONDS_PER_POINT) / 60));
  const by = [summary.authors, summary.year].filter((s) => s.trim()).join(" · ");

  const pageChips = (item: SourceItem) =>
    item.pages.map((p) => (
      <button
        key={p}
        type="button"
        className="page-chip"
        aria-pressed={openPage === p}
        title={t("source.showNotes")}
        onClick={() => setOpenPage(openPage === p ? null : p)}
      >
        {t("source.page", { n: p })}
      </button>
    ));

  const tickList = (items: SourceItem[], terms: boolean) =>
    items.map((i) => (
      <label key={i.id} className="source-tick">
        <input type="checkbox" checked={i.selected} onChange={(e) => change(updateItem(summary, i.id, { selected: e.target.checked }))} />
        <span lang={lang}>
          {terms && <strong lang="en">{i.term} — </strong>}
          {i.text}
        </span>
        <span className="t-micro t-muted">{pageRanges(i.pages)}</span>
      </label>
    ));

  return (
    <div className="source">
      <div className="card source__head">
        <div className="source__title">
          <span className="t-small t-muted">{t("source.reviewTitle")}</span>
          <h2 className="t-h2">{summary.title}</h2>
          {by && <span className="t-small">{by}</span>}
          <span className="t-small t-muted">
            {source.file_name} · {t("source.pagesRead", { from: source.page_from, to: source.page_to, total: source.page_count })}
          </span>
        </div>
        <Button icon={<RefreshCw {...ICON} />} onClick={() => setConfirmReread(true)}>
          {t("source.reread")}
        </Button>
      </div>

      <Notice>{t("source.reviewHint")}</Notice>

      <section className="field">
        <h3 className="t-h3">{t("source.keyPoints")}</h3>
        <ol className="source__points">
          {summary.key_points.map((k, n) => (
            <li key={k.id} className={k.selected ? "card source-point" : "card source-point source-point--off"}>
              <input
                type="checkbox"
                aria-label={t("source.include")}
                checked={k.selected}
                onChange={(e) => change(updateItem(summary, k.id, { selected: e.target.checked }))}
              />
              <div className="source-point__body">
                <p lang={lang}>{k.text}</p>
                <div className="chips">
                  {pageChips(k)}
                  <button type="button" className="link t-small" onClick={() => setNoteOpen(noteOpen === k.id ? null : k.id)}>
                    {k.note ? t("source.editNote") : t("source.addNote")}
                  </button>
                </div>
                {openPage !== null && k.pages.includes(openPage) && (
                  <div className="source-notes t-small">
                    {notesForPage(sections, openPage).map((s) => (
                      <p key={s.heading + s.notes.slice(0, 20)}>
                        <strong>{s.heading}:</strong> {s.notes}
                      </p>
                    ))}
                  </div>
                )}
                {(noteOpen === k.id || k.note) && (
                  <textarea
                    className="textarea textarea--short"
                    placeholder={t("source.notePlaceholder")}
                    maxLength={500}
                    value={k.note}
                    onChange={(e) => change(updateItem(summary, k.id, { note: e.target.value }))}
                  />
                )}
              </div>
              <div className="source-point__order">
                <IconButton
                  label={t("source.moveUp")}
                  icon={<ArrowUp {...ICON} />}
                  disabled={n === 0}
                  onClick={() => change(moveKeyPoint(summary, k.id, -1))}
                />
                <IconButton
                  label={t("source.moveDown")}
                  icon={<ArrowDown {...ICON} />}
                  disabled={n === summary.key_points.length - 1}
                  onClick={() => change(moveKeyPoint(summary, k.id, 1))}
                />
              </div>
            </li>
          ))}
        </ol>
      </section>

      {summary.facts.length > 0 && (
        <section className="field">
          <h3 className="t-h3">{t("source.facts")}</h3>
          <div className="card source-list">{tickList(summary.facts, false)}</div>
        </section>
      )}
      {summary.terms.length > 0 && (
        <section className="field">
          <h3 className="t-h3">{t("source.terms")}</h3>
          <div className="card source-list">{tickList(summary.terms, true)}</div>
        </section>
      )}

      <div className="source__footer">
        <span className="t-small">
          {t("source.selected", { n: selected, total: summary.key_points.length })}
          {" · "}
          {t("source.estimate", { m: minutes, target: project.target_minutes })}
        </span>
        <Badge tone="info">{t("source.nothingYet")}</Badge>
        <Button variant="primary" icon={<Check {...ICON} />} disabled={busy || selected === 0} onClick={() => void confirm()}>
          {t("source.confirm")}
        </Button>
      </div>

      {confirmReread && (
        <ConfirmDialog
          title={t("source.rereadTitle")}
          body={t("source.rereadBody")}
          confirmLabel={t("source.reread")}
          onConfirm={() => void reread()}
          onCancel={() => setConfirmReread(false)}
        />
      )}
    </div>
  );
}
