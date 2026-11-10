import { ArrowRight, Clapperboard, Eye, FileText, FolderOpen, Images, Plus, Search, Trash2 } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import { ProjectStatusBadge } from "../components/Badge";
import { Button, IconButton } from "../components/Button";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { EmptyState } from "../components/EmptyState";
import { LocalOfferCard } from "../components/LocalOfferCard";
import { Notice } from "../components/Notice";
import { PageHeader } from "../components/PageHeader";
import { PresetIcon } from "../components/PresetIcon";
import { SceneThumb } from "../components/SceneThumb";
import { TechPresetSampleDialog } from "../components/TechPresetSampleDialog";
import techCodeSample from "../assets/preset-samples/tech-code.jpg";
import techDiagramSample from "../assets/preset-samples/tech-diagram.jpg";
import techPhotoSample from "../assets/preset-samples/tech-photo.jpg";
import { useToast } from "../components/toast";
import { useI18n } from "../i18n/context";
import { api, errorMessage } from "../lib/api";
import { formatDate } from "../lib/format";
import { PRESET_IDS, presetKey } from "../lib/presets";
import type { Navigate } from "../lib/route";
import type { ProjectStatus, ProjectSummary } from "../lib/types";

interface Props {
  projects: ProjectSummary[];
  hasKey: boolean;
  navigate: Navigate;
  onChanged: () => Promise<void>;
  searchToken: number;
}

/** How far a project is: 1 = script, 2 = scenes, 3 = render / export, 4 = finished. */
function stepOf(status: ProjectStatus): number {
  if (status === "script_draft") return 1;
  if (status === "generating" || status === "paused_quota") return 2;
  if (status === "done") return 4;
  return 3;
}

type StatusFilter = "all" | "active" | "done" | "paused";
type SortKey = "updated" | "title" | "status";
const FILTERS: StatusFilter[] = ["all", "active", "done", "paused"];
const SORTS: SortKey[] = ["updated", "title", "status"];

/** The page a project should open on: the step it is at. */
function resumeStep(status: ProjectStatus): "script" | "scenes" | "render" {
  if (status === "script_draft") return "script";
  if (status === "generating" || status === "paused_quota") return "scenes";
  return "render";
}

function filterOf(status: ProjectStatus): Exclude<StatusFilter, "all"> {
  if (status === "done") return "done";
  if (status === "paused_quota") return "paused";
  return "active";
}

/** Three short segments (Script · Scenes · Render) filled up to where the project is. */
function StepMeter({ status }: { status: ProjectStatus }) {
  const { t } = useI18n();
  const step = stepOf(status);
  const names = [t("flow.script"), t("flow.scenes"), t("flow.render")];
  const label = step > 3 ? t("projects.stepDone") : t("projects.step", { n: step, name: names[step - 1] ?? "" });
  return (
    <span className="step-meter" title={label} aria-label={label} role="img">
      {[1, 2, 3].map((i) => (
        <i key={i} className={i < step ? "step-meter__seg step-meter__seg--done" : i === step ? "step-meter__seg step-meter__seg--now" : "step-meter__seg"} />
      ))}
    </span>
  );
}

/** "3h 12m" / "12m" until `iso`; null once it has passed. */
function timeUntil(iso: string, now: number): string | null {
  const at = new Date(iso).getTime();
  if (Number.isNaN(at) || at <= now) return null;
  const mins = Math.ceil((at - now) / 60_000);
  const h = Math.floor(mins / 60);
  return h > 0 ? `${h}h ${mins % 60}m` : `${mins}m`;
}

/** Quota-paused projects: live countdown to the allowance reset (re-evaluated every 30 s). */
function PauseCountdown({ resetsAt }: { resetsAt: string }) {
  const { t } = useI18n();
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 30_000);
    return () => window.clearInterval(id);
  }, []);
  const left = timeUntil(resetsAt, now);
  return (
    <span className="t-micro project-card__pause">
      {left ? t("projects.pausedIn", { time: left }) : t("projects.pausedReady")}
    </span>
  );
}

/** Aspect drawn over a project cover (the status lives in the card body). */
function CardOverlay({ project }: { project: ProjectSummary }) {
  return <span className="thumb__aspect t-micro">{project.aspect}</span>;
}

/** Cover for a project with no picture yet: its first letter on a soft colour from its id. */
function CoverPlaceholder({ project }: { project: ProjectSummary }) {
  let hash = 0;
  for (const ch of project.id) hash = (hash * 31 + ch.charCodeAt(0)) % 360;
  const letter = [...project.title.trim()][0] ?? "";
  return (
    <div
      className="thumb thumb--placeholder"
      style={{ "--cover-hue": hash } as React.CSSProperties}
      aria-hidden="true"
    >
      <span className="thumb__letter" lang={project.language}>
        {letter}
      </span>
      <CardOverlay project={project} />
    </div>
  );
}

export function ProjectsPage({ projects, hasKey, navigate, onChanged, searchToken }: Props) {
  const { t, lang } = useI18n();
  const toast = useToast();
  const [query, setQuery] = useState("");
  const [toDelete, setToDelete] = useState<ProjectSummary | null>(null);
  const [showTechSample, setShowTechSample] = useState(false);
  const searchRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (searchToken > 0) searchRef.current?.focus();
  }, [searchToken]);

  const [filter, setFilter] = useState<StatusFilter>("all");
  const [sort, setSort] = useState<SortKey>("updated");

  const counts = useMemo(() => {
    const c: Record<StatusFilter, number> = { all: projects.length, active: 0, done: 0, paused: 0 };
    for (const p of projects) c[filterOf(p.status)] += 1;
    return c;
  }, [projects]);

  const latest = useMemo(
    () => projects.reduce<ProjectSummary | null>((a, p) => (!a || p.updated_at > a.updated_at ? p : a), null),
    [projects],
  );

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    const list = projects.filter(
      (p) =>
        (filter === "all" || filterOf(p.status) === filter) &&
        (!q || p.title.toLowerCase().includes(q) || p.topic.toLowerCase().includes(q)),
    );
    const by: Record<SortKey, (a: ProjectSummary, b: ProjectSummary) => number> = {
      updated: (a, b) => b.updated_at.localeCompare(a.updated_at),
      title: (a, b) => a.title.localeCompare(b.title, lang),
      status: (a, b) => stepOf(a.status) - stepOf(b.status) || b.updated_at.localeCompare(a.updated_at),
    };
    return [...list].sort(by[sort]);
  }, [projects, query, filter, sort, lang]);

  const confirmDelete = async () => {
    if (!toDelete) return;
    const target = toDelete;
    setToDelete(null);
    try {
      await api.projectDelete(target.id);
      toast.show(t("projects.deleted"), "ok");
      await onChanged();
    } catch (e) {
      toast.show(errorMessage(e), "danger");
    }
  };

  return (
    <div className="page">
      <PageHeader
        breadcrumb={[{ label: t("projects.breadcrumb") }]}
        title={t("projects.title")}
        actions={
          <>
            <label className="search">
              <span className="visually-hidden">{t("projects.searchPlaceholder")}</span>
              <Search className="search__icon" size={16} strokeWidth={1.5} aria-hidden="true" />
              <input
                ref={searchRef}
                className="input"
                type="search"
                value={query}
                placeholder={t("projects.searchPlaceholder")}
                onChange={(e) => setQuery(e.target.value)}
              />
              <span className="search__kbd t-micro">Ctrl K</span>
            </label>
            <Button
              variant="primary"
              icon={<Plus size={16} strokeWidth={1.5} />}
              onClick={() => navigate({ name: "new" })}
            >
              {t("nav.newProject")}
            </Button>
          </>
        }
      />

      {!hasKey && (
        <Notice
          tone="warn"
          actions={
            <Button variant="pill" onClick={() => navigate({ name: "settings" })}>
              {t("projects.openSettings")}
            </Button>
          }
        >
          {t("projects.noKey")}
        </Notice>
      )}

      <LocalOfferCard onInstall={() => navigate({ name: "settings" })} />

      {projects.length === 0 ? (
        <EmptyState
          icon={<FolderOpen size={18} strokeWidth={1.5} />}
          title={t("projects.empty.title")}
          body={t("projects.empty.body")}
          extra={
            <ol className="empty__steps">
              <li>
                <FileText size={16} strokeWidth={1.5} aria-hidden="true" />
                <span className="t-small">{t("projects.empty.step1")}</span>
              </li>
              <li>
                <Images size={16} strokeWidth={1.5} aria-hidden="true" />
                <span className="t-small">{t("projects.empty.step2")}</span>
              </li>
              <li>
                <Clapperboard size={16} strokeWidth={1.5} aria-hidden="true" />
                <span className="t-small">{t("projects.empty.step3")}</span>
              </li>
            </ol>
          }
          action={
            <Button variant="primary" onClick={() => navigate({ name: "new" })}>
              {t("nav.newProject")}
            </Button>
          }
        />
      ) : (
        <section className="section">
          <div className="section__header">
            <h2 className="t-h2">{t("projects.recent")}</h2>
            <span className="section__count t-small">{projects.length}</span>
          </div>
          {latest && !query && filter === "all" && (
            <div className="continue-card">
              <div className="continue-card__cover">
                {latest.cover_scene ? (
                  <SceneThumb projectId={latest.id} sceneId={latest.cover_scene} version={latest.cover_version} alt="">
                    <CardOverlay project={latest} />
                  </SceneThumb>
                ) : (
                  <CoverPlaceholder project={latest} />
                )}
              </div>
              <div className="continue-card__body">
                <span className="t-micro t-muted">{t("projects.continue")}</span>
                <h3 className="t-h3 continue-card__title" lang={latest.language}>
                  {latest.title}
                </h3>
                <div className="row">
                  <ProjectStatusBadge status={latest.status} />
                  {latest.pause_resets_at && <PauseCountdown resetsAt={latest.pause_resets_at} />}
                  <StepMeter status={latest.status} />
                </div>
              </div>
              <Button
                variant="primary"
                icon={<ArrowRight size={16} strokeWidth={1.75} />}
                onClick={() => navigate({ name: resumeStep(latest.status), projectId: latest.id })}
              >
                {t(`projects.continueTo.${resumeStep(latest.status)}`)}
              </Button>
            </div>
          )}
          <div className="projects-toolbar">
            <div className="segmented" role="group" aria-label={t("projects.filter")}>
              {FILTERS.filter((f) => f === "all" || counts[f] > 0 || filter === f).map((f) => (
                <button key={f} type="button" aria-pressed={filter === f} onClick={() => setFilter(f)}>
                  {t(`projects.filter.${f}`)} <span className="t-micro t-muted">{counts[f]}</span>
                </button>
              ))}
            </div>
            <label className="projects-toolbar__sort t-small">
              <span className="t-muted">{t("projects.sort")}</span>
              <select className="input" value={sort} onChange={(e) => setSort(e.target.value as SortKey)}>
                {SORTS.map((s) => (
                  <option key={s} value={s}>
                    {t(`projects.sort.${s}`)}
                  </option>
                ))}
              </select>
            </label>
          </div>
          {filtered.length === 0 ? (
            <p className="t-small">{query ? t("projects.noMatch", { q: query }) : t("projects.noneInFilter")}</p>
          ) : (
            <div className="grid">
              {filtered.map((p) => (
                <article key={p.id} className="feature-card project-card">
                  <button
                    className="project-card__open"
                    onClick={() => navigate({ name: resumeStep(p.status), projectId: p.id })}
                  >
                    {p.cover_scene ? (
                      <SceneThumb projectId={p.id} sceneId={p.cover_scene} version={p.cover_version} alt="">
                        <CardOverlay project={p} />
                      </SceneThumb>
                    ) : (
                      <CoverPlaceholder project={p} />
                    )}
                    <span className="project-card__body">
                      <h3 className="t-h3 project-card__title" lang={p.language}>
                        {p.title}
                      </h3>
                      <span className="project-card__meta t-small">
                        {t("script.length", {
                          minutes: p.target_minutes,
                          scenes: p.scene_count,
                          lang: t(`lang.${p.language}`),
                        })}
                      </span>
                      <span className="project-card__status">
                        <ProjectStatusBadge status={p.status} />
                      </span>
                      {p.pause_resets_at && <PauseCountdown resetsAt={p.pause_resets_at} />}
                      <span className="project-card__footer t-micro">
                        {t("projects.updated", { date: formatDate(p.updated_at, lang) })}
                        <StepMeter status={p.status} />
                      </span>
                    </span>
                  </button>
                  <div className="project-card__delete">
                    <IconButton
                      label={t("projects.delete")}
                      icon={<Trash2 size={15} strokeWidth={1.75} />}
                      onClick={() => setToDelete(p)}
                    />
                  </div>
                </article>
              ))}
            </div>
          )}
        </section>
      )}

      <section className="section">
        <div className="section__header">
          <h2 className="t-h2">{t("projects.presets")}</h2>
          <span className="t-small">{t("projects.presetsHint")}</span>
        </div>
        <div className="shortcuts">
          {PRESET_IDS.map((id) => (
            <div key={id} className="shortcut-option">
              <button className="shortcut" onClick={() => navigate({ name: "new", preset: id })}>
                <span className={`shortcut__art${id === "tech_explainer" ? " shortcut__art--sample" : ""}`}>
                  {id === "tech_explainer" ? (
                    <>
                      <img className="shortcut__sample-photo" src={techPhotoSample} alt="" />
                      <span className="shortcut__sample-stack">
                        <img src={techDiagramSample} alt="" />
                        <img src={techCodeSample} alt="" />
                      </span>
                    </>
                  ) : (
                    <PresetIcon id={id} />
                  )}
                </span>
                <span className="t-small">{t(presetKey(id))}</span>
              </button>
              {id === "tech_explainer" && (
                <button className="shortcut__preview" onClick={() => setShowTechSample(true)}>
                  <Eye size={14} strokeWidth={1.5} aria-hidden="true" />
                  {t("projects.sample.view")}
                </button>
              )}
            </div>
          ))}
        </div>
      </section>

      {showTechSample && (
        <TechPresetSampleDialog
          onClose={() => setShowTechSample(false)}
          onStart={() => {
            setShowTechSample(false);
            navigate({ name: "new", preset: "tech_explainer" });
          }}
        />
      )}

      {toDelete && (
        <ConfirmDialog
          title={t("projects.deleteTitle", { title: toDelete.title })}
          body={t("projects.deleteBody")}
          confirmLabel={t("common.delete")}
          danger
          onConfirm={() => void confirmDelete()}
          onCancel={() => setToDelete(null)}
        />
      )}
    </div>
  );
}

