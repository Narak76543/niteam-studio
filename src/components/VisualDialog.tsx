import { AlertTriangle, Check, ExternalLink, ImagePlus, Search } from "lucide-react";
import { type MouseEvent, useCallback, useEffect, useState } from "react";

import { Badge } from "./Badge";
import { Button } from "./Button";
import { ErrorPanel } from "./ErrorPanel";
import { Notice } from "./Notice";
import { useToast } from "./toast";
import { useI18n } from "../i18n/context";
import { api, errorMessage } from "../lib/api";
import { ERROR_KINDS } from "../lib/types";
import type {
  CodeSpec,
  DiagramSpec,
  Project,
  Scene,
  SourceNotice,
  StockCandidate,
  StockMedia,
  StockSource,
  VisualEdit,
  VisualKind,
} from "../lib/types";
import { drawDiagram } from "../lib/diagramRender";
import { assetId, cropAt, licenseLabel } from "../lib/visuals";

const ICON = { size: 14, strokeWidth: 1.5 } as const;
const KINDS: VisualKind[] = ["real_photo", "stock_video", "diagram", "code", "screenshot", "ai_image"];
const PHOTO_SOURCES: StockSource[] = ["wikimedia", "openverse", "pixabay", "pexels"];
const VIDEO_SOURCES: StockSource[] = ["pixabay", "pexels"];
/** Must match `gemini::script::CODE_LANGUAGES`. */
const CODE_LANGUAGES = [
  "bash",
  "powershell",
  "python",
  "javascript",
  "typescript",
  "json",
  "yaml",
  "rust",
  "go",
  "java",
  "c",
  "cpp",
  "sql",
  "html",
  "css",
  "text",
] as const;
/** Subtitle band of the 1080p frame (`render::clip::Frame`): 240 px tall, 24 px above the bottom. */
const BAND = { top: (1080 - 240 - 24) / 1080, height: 240 / 1080 };
const PEXELS_URL = "https://www.pexels.com";

interface Props {
  project: Project;
  scene: Scene;
  onClose: () => void;
  onChanged: (p: Project) => void;
}

function emptyDiagram(): DiagramSpec {
  return { template: "flow", mermaid: 'flowchart LR\n  A["…"] --> B["…"]' };
}

function emptyCode(): CodeSpec {
  return { language: "bash", text: "", terminal: true, type_out: true };
}

/** Scene editor (docs/11 §8): visual type, stock search + pick, subject point, diagram / code text, own screenshot. */
export function VisualDialog({ project, scene, onClose, onChanged }: Props) {
  const { t } = useI18n();
  const toast = useToast();
  const v = scene.visual;
  const [kind, setKind] = useState<VisualKind>(v.kind);
  const [query, setQuery] = useState(v.keywords[0] ?? "");
  const [source, setSource] = useState<StockSource | "all">("all");
  const [results, setResults] = useState<StockCandidate[]>(v.candidates);
  const [notices, setNotices] = useState<SourceNotice[]>([]);
  const [page, setPage] = useState(1);
  const [busy, setBusy] = useState<string | null>(null);
  const [searchError, setSearchError] = useState<string | null>(null);
  const [diagram, setDiagram] = useState<DiagramSpec>(v.diagram ?? emptyDiagram());
  const [code, setCode] = useState<CodeSpec>(v.code ?? emptyCode());
  const [consent, setConsent] = useState(false);
  const [preview, setPreview] = useState<{ url: string; w: number; h: number } | null>(null);
  const [focus, setFocus] = useState<[number, number]>(v.focus ?? [0.5, 0.5]);
  const [diagramView, setDiagramView] = useState<{ url: string | null; error: string | null }>({ url: null, error: null });

  const media: StockMedia = kind === "stock_video" ? "video" : "photo";
  const sources = media === "video" ? VIDEO_SOURCES : PHOTO_SOURCES;
  const chosen = v.asset;
  const chosenCandidate = results.find((c) => assetId(c) === chosen) ?? v.candidates.find((c) => assetId(c) === chosen) ?? null;
  /** The scene currently shows a file (photo, clip, own screenshot). */
  const hasFile = (v.kind === "real_photo" || v.kind === "stock_video" || v.kind === "screenshot") && chosen !== null;

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  // Preview of the chosen photo / clip poster for the subject point.
  useEffect(() => {
    if (!hasFile) return;
    let url: string | null = null;
    let alive = true;
    api
      .sceneVisualPreview(project.id, scene.id)
      .then((bytes) => {
        if (!alive) return;
        url = URL.createObjectURL(new Blob([bytes], { type: "image/jpeg" }));
        const img = new Image();
        img.onload = () => alive && url && setPreview({ url, w: img.naturalWidth, h: img.naturalHeight });
        img.src = url;
      })
      .catch(() => setPreview(null));
    return () => {
      alive = false;
      if (url) URL.revokeObjectURL(url);
    };
  }, [project.id, scene.id, chosen, hasFile]);

  /** Runs a change; true when it worked. */
  // Live diagram preview (same drawing as the video, smaller), redrawn after typing stops.
  useEffect(() => {
    if (kind !== "diagram") return;
    let alive = true;
    let url: string | null = null;
    const timer = window.setTimeout(() => {
      drawDiagram(diagram.mermaid, 960, 540, 408)
        .then((png) => {
          if (!alive) return;
          url = URL.createObjectURL(new Blob([png], { type: "image/png" }));
          setDiagramView({ url, error: null });
        })
        .catch((e: unknown) => alive && setDiagramView({ url: null, error: errorMessage(e) }));
    }, 500);
    return () => {
      alive = false;
      window.clearTimeout(timer);
      if (url) URL.revokeObjectURL(url);
    };
  }, [kind, diagram.mermaid]);

  const run = async (label: string, fn: () => Promise<Project>): Promise<boolean> => {
    setBusy(label);
    try {
      onChanged(await fn());
      return true;
    } catch (e) {
      toast.show(errorMessage(e), "danger");
      return false;
    } finally {
      setBusy(null);
    }
  };

  const edit = (patch: Partial<VisualEdit>): VisualEdit => ({
    kind,
    keywords: v.keywords,
    diagram: kind === "diagram" ? diagram : v.diagram,
    code: kind === "code" ? code : v.code,
    focus: v.focus,
    confirm: false,
    ...patch,
  });

  const search = useCallback(
    async (nextPage: number) => {
      if (!query.trim()) return;
      setBusy("search");
      setSearchError(null);
      try {
        const keywords = [query, ...v.keywords.filter((k) => k !== query)];
        const out = await api.stockSearch(project.id, query, keywords, media, nextPage, source === "all" ? null : [source]);
        setResults(nextPage === 1 ? out.candidates : (r) => [...r, ...out.candidates.filter((c) => !r.some((x) => assetId(x) === assetId(c)))]);
        setNotices(out.notices);
        setPage(nextPage);
      } catch (e) {
        setSearchError(errorMessage(e));
      } finally {
        setBusy(null);
      }
    },
    [project.id, query, media, source, v.keywords],
  );

  const setPoint = (e: MouseEvent<HTMLDivElement>) => {
    const box = e.currentTarget.getBoundingClientRect();
    const next: [number, number] = [(e.clientX - box.left) / box.width, (e.clientY - box.top) / box.height];
    setFocus(next);
    void run("focus", () => api.sceneVisualUpdate(project.id, scene.id, edit({ kind: v.kind, focus: next })));
  };

  const crop = preview ? cropAt(preview.w, preview.h, focus) : null;
  const pexelsShown = results.some((c) => c.source === "pexels");

  return (
    <div className="backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="dialog dialog--picker" role="dialog" aria-modal="true" aria-labelledby="visual-dialog-title">
        <h2 id="visual-dialog-title" className="t-h2">
          {t("visual.title", { n: scene.order })}
        </h2>
        <p className="t-small" lang={project.language}>
          {scene.narration}
        </p>

        <div className="segmented visual-kinds" role="group" aria-label={t("visual.kind")}>
          {KINDS.map((k) => (
            <button key={k} type="button" aria-pressed={kind === k} onClick={() => setKind(k)}>
              {t(`visual.kind.${k}`)}
            </button>
          ))}
        </div>

        {(kind === "real_photo" || kind === "stock_video") && (
          <>
            <form
              className="row"
              onSubmit={(e) => {
                e.preventDefault();
                void search(1);
              }}
            >
              <input
                className="input input--grow"
                lang="en"
                aria-label={t("visual.search")}
                placeholder={t("visual.searchPlaceholder")}
                value={query}
                onChange={(e) => setQuery(e.target.value)}
              />
              <Button type="submit" icon={<Search {...ICON} />} disabled={busy !== null || !query.trim()}>
                {busy === "search" ? t("visual.searching") : t("visual.search")}
              </Button>
            </form>
            {v.keywords.length > 0 && (
              <div className="row visual-keywords">
                <span className="t-micro t-muted">{t("visual.keywords")}</span>
                {v.keywords.map((k) => (
                  <button key={k} type="button" className="chip-button t-micro" lang="en" onClick={() => setQuery(k)}>
                    {k}
                  </button>
                ))}
              </div>
            )}
            <div className="segmented" role="group" aria-label={t("visual.sources")}>
              {(["all", ...sources] as const).map((s) => (
                <button key={s} type="button" aria-pressed={source === s} onClick={() => setSource(s)}>
                  {s === "all" ? t("visual.allSources") : t(`visual.source.${s}`)}
                </button>
              ))}
            </div>
            {searchError && <ErrorPanel kind={null} details={searchError} onRetry={() => void search(1)} />}
            {notices.map((n) => (
              <Notice key={`${n.source}-${n.kind}`} tone="warn">
                {t(`visual.source.${n.source}`)}: {t(`error.${ERROR_KINDS.find((k) => k === n.kind) ?? "other"}.title`)}
                {n.retry_at_utc && ` · ${t("visual.retryAt", { time: new Date(n.retry_at_utc).toLocaleTimeString() })}`}
              </Notice>
            ))}

            {results.length === 0 ? (
              <p className="t-small t-muted">{t("visual.noResults")}</p>
            ) : (
              <div className="cand-grid">
                {results.map((c) => {
                  const id = assetId(c);
                  const selected = id === chosen;
                  return (
                    <div key={id} className={`cand${selected ? " cand--selected" : ""}`}>
                      <button
                        type="button"
                        className="cand__pick"
                        disabled={busy !== null}
                        title={c.title}
                        onClick={() => void run(id, () => api.stockPick(project.id, scene.id, c))}
                      >
                        <img className="cand__img" src={c.thumb_url} alt={c.title} loading="lazy" referrerPolicy="no-referrer" />
                        <span className="cand__badges">
                          {selected && (
                            <Badge tone="ok">
                              <Check size={12} strokeWidth={2} aria-hidden="true" /> {t("visual.chosen")}
                            </Badge>
                          )}
                          {c.low_res && <Badge tone="warn">{t("visual.lowRes")}</Badge>}
                          {c.restrictions.length > 0 && (
                            <span title={t("visual.restricted", { what: c.restrictions.join(", ") })}>
                              <Badge tone="warn">
                                <AlertTriangle size={12} strokeWidth={2} aria-hidden="true" /> {t("visual.restrictedShort")}
                              </Badge>
                            </span>
                          )}
                          {c.duration_ms !== null && <Badge>{Math.round(c.duration_ms / 1000)} s</Badge>}
                        </span>
                        {busy === id && <span className="cand__busy t-small">{t("visual.downloading")}</span>}
                      </button>
                      <div className="cand__meta t-micro">
                        <span>
                          {c.width}×{c.height} · {licenseLabel(c.license)}
                        </span>
                        <span className="t-muted">
                          {t(`visual.source.${c.source}`)}
                          {c.author ? ` · ${c.author}` : ""}
                        </span>
                        <button type="button" className="link t-micro" onClick={() => void api.openExternal(c.page_url)}>
                          {t("visual.openPage")} <ExternalLink size={10} strokeWidth={1.5} aria-hidden="true" />
                        </button>
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
            <div className="row">
              <Button variant="pill" disabled={busy !== null || !query.trim()} onClick={() => void search(page + 1)}>
                {t("visual.more")}
              </Button>
              <span className="t-micro t-muted">{t("visual.licenceNote")}</span>
            </div>
          </>
        )}

        {hasFile && kind === v.kind && (
          <section className="focus-section">
            <span className="t-small">{t("visual.subject")}</span>
            {preview && crop ? (
              <div className="focus-preview" onClick={setPoint} role="presentation">
                <img src={preview.url} alt={t("visual.previewAlt")} draggable={false} />
                <span
                  className="focus-crop"
                  style={{
                    left: `${(crop.x / preview.w) * 100}%`,
                    top: `${(crop.y / preview.h) * 100}%`,
                    width: `${(crop.w / preview.w) * 100}%`,
                    height: `${(crop.h / preview.h) * 100}%`,
                  }}
                >
                  <span className="focus-band" style={{ top: `${BAND.top * 100}%`, height: `${BAND.height * 100}%` }} />
                </span>
                <span className="focus-dot" style={{ left: `${focus[0] * 100}%`, top: `${focus[1] * 100}%` }} />
              </div>
            ) : (
              <p className="t-small t-muted">{t("common.loading")}</p>
            )}
            <span className="t-micro t-muted">{t("visual.subjectHint")}</span>
            {chosenCandidate && (
              <span className="t-micro">
                {chosenCandidate.title} · {chosenCandidate.author || t("visual.unknownAuthor")} · {licenseLabel(chosenCandidate.license)}
              </span>
            )}
            {v.auto_picked && (
              <Notice
                tone="warn"
                actions={
                  <Button variant="pill" disabled={busy !== null} onClick={() => void run("confirm", () => api.sceneVisualUpdate(project.id, scene.id, edit({ kind: v.kind, confirm: true })))}>
                    {t("visual.looksGood")}
                  </Button>
                }
              >
                {t("visual.autoPicked")}
              </Notice>
            )}
          </section>
        )}

        {kind === "diagram" && (
          <label className="field">
            <span className="field__label">{t("visual.mermaid")}</span>
            <textarea className="textarea textarea--code" lang="en" spellCheck={false} value={diagram.mermaid} onChange={(e) => setDiagram({ ...diagram, mermaid: e.target.value })} />
            <span className="t-small">{t("visual.mermaidHint")}</span>
          </label>
        )}
        {kind === "diagram" && (
          <section className="focus-section">
            {diagramView.url && <img className="diagram-preview" src={diagramView.url} alt={t("visual.diagramPreview")} />}
            {diagramView.error && (
              <Notice
                tone="danger"
                actions={
                  <Button
                    variant="pill"
                    disabled={busy !== null}
                    onClick={() =>
                      void (async () => {
                        // Save the current text first so Gemini fixes what the user sees.
                        const saved = await run("save", () => api.sceneVisualUpdate(project.id, scene.id, edit({ kind: "diagram" })).catch(() => api.projectGet(project.id)));
                        if (!saved) return;
                        await run("fix", async () => {
                          const p = await api.diagramFix(project.id, scene.id, diagramView.error ?? "");
                          const fixed = p.scenes.find((s) => s.id === scene.id)?.visual.diagram;
                          if (fixed) setDiagram(fixed);
                          return p;
                        });
                      })()
                    }
                  >
                    {busy === "fix" ? t("visual.fixing") : t("visual.fixWithGemini")}
                  </Button>
                }
              >
                {t("visual.diagramError")} <span className="t-micro">{diagramView.error}</span>
              </Notice>
            )}
          </section>
        )}

        {kind === "code" && (
          <>
            <div className="row">
              <label className="field">
                <span className="field__label">{t("visual.language")}</span>
                <select className="select" lang="en" value={code.language} onChange={(e) => setCode({ ...code, language: e.target.value })}>
                  {CODE_LANGUAGES.map((l) => (
                    <option key={l} value={l}>
                      {l}
                    </option>
                  ))}
                </select>
              </label>
              <label className="row">
                <input type="checkbox" checked={code.terminal} onChange={(e) => setCode({ ...code, terminal: e.target.checked })} />
                <span className="t-small">{t("visual.terminal")}</span>
              </label>
              <label className="row">
                <input type="checkbox" checked={code.type_out} onChange={(e) => setCode({ ...code, type_out: e.target.checked })} />
                <span className="t-small">{t("visual.typeOut")}</span>
              </label>
            </div>
            <textarea className="textarea textarea--code" lang="en" spellCheck={false} aria-label={t("visual.code")} value={code.text} onChange={(e) => setCode({ ...code, text: e.target.value })} />
          </>
        )}

        {kind === "screenshot" && (
          <>
            <label className="row">
              <input type="checkbox" checked={consent} onChange={(e) => setConsent(e.target.checked)} />
              <span className="t-small">{t("visual.consent")}</span>
            </label>
            <div className="row">
              <Button
                icon={<ImagePlus {...ICON} />}
                disabled={!consent || busy !== null}
                onClick={() =>
                  void (async () => {
                    const path = await api.pickImageFile();
                    if (path) await run("import", () => api.sceneScreenshotImport(project.id, scene.id, path, consent));
                  })()
                }
              >
                {t("visual.chooseImage")}
              </Button>
            </div>
          </>
        )}

        {kind === "ai_image" && <p className="t-small">{t("visual.aiHint")}</p>}

        {pexelsShown && (
          <button type="button" className="link t-micro row" onClick={() => void api.openExternal(PEXELS_URL)}>
            {t("visual.pexelsCredit")} <ExternalLink size={10} strokeWidth={1.5} aria-hidden="true" />
          </button>
        )}

        <div className="dialog__actions">
          <Button onClick={onClose}>{t("common.close")}</Button>
          {(kind === "diagram" || kind === "code" || kind === "ai_image") && (
            <Button variant="primary" disabled={busy !== null} onClick={() => void run("save", () => api.sceneVisualUpdate(project.id, scene.id, edit({}))).then((ok) => ok && onClose())}>
              {t("common.save")}
            </Button>
          )}
        </div>
      </div>
    </div>
  );
}
