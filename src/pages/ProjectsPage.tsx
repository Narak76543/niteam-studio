import { Film, FolderOpen, Plus, Search, Trash2 } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import { ProjectStatusBadge } from "../components/Badge";
import { Button, IconButton } from "../components/Button";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { EmptyState } from "../components/EmptyState";
import { LocalOfferCard } from "../components/LocalOfferCard";
import { Notice } from "../components/Notice";
import { PageHeader } from "../components/PageHeader";
import { PresetIcon } from "../components/PresetIcon";
import { useToast } from "../components/toast";
import { useI18n } from "../i18n/context";
import { api, errorMessage } from "../lib/api";
import { formatDate } from "../lib/format";
import { PRESET_IDS, presetKey } from "../lib/presets";
import type { Navigate } from "../lib/route";
import type { ProjectSummary } from "../lib/types";

interface Props {
  projects: ProjectSummary[];
  hasKey: boolean;
  navigate: Navigate;
  onChanged: () => Promise<void>;
  searchToken: number;
}

export function ProjectsPage({ projects, hasKey, navigate, onChanged, searchToken }: Props) {
  const { t, lang } = useI18n();
  const toast = useToast();
  const [query, setQuery] = useState("");
  const [toDelete, setToDelete] = useState<ProjectSummary | null>(null);
  const searchRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (searchToken > 0) searchRef.current?.focus();
  }, [searchToken]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return projects;
    return projects.filter(
      (p) => p.title.toLowerCase().includes(q) || p.topic.toLowerCase().includes(q),
    );
  }, [projects, query]);

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
          </div>
          {filtered.length === 0 ? (
            <p className="t-small">{t("projects.noMatch", { q: query })}</p>
          ) : (
            <div className="grid">
              {filtered.map((p) => (
                <article key={p.id} className="feature-card project-card">
                  <div className="feature-card__inner">
                    <button
                      className="project-card__open"
                      onClick={() =>
                        navigate({ name: p.status === "script_draft" ? "script" : "scenes", projectId: p.id })
                      }
                    >
                      <div className="thumb" aria-hidden="true">
                        <Film size={24} strokeWidth={1.5} />
                      </div>
                      <h3 className="t-h3 project-card__title" lang={p.language}>
                        {p.title}
                      </h3>
                      <p className="t-small">
                        {t("script.length", {
                          minutes: p.target_minutes,
                          scenes: p.scene_count,
                          lang: t(`lang.${p.language}`),
                        })}
                      </p>
                    </button>
                    <div className="project-card__footer">
                      <ProjectStatusBadge status={p.status} />
                      <span className="t-small">
                        {t("projects.updated", { date: formatDate(p.updated_at, lang) })}
                      </span>
                      <IconButton
                        label={t("projects.delete")}
                        icon={<Trash2 size={16} strokeWidth={1.5} />}
                        onClick={() => setToDelete(p)}
                      />
                    </div>
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
            <button key={id} className="shortcut" onClick={() => navigate({ name: "new", preset: id })}>
              <span className="shortcut__art">
                <PresetIcon id={id} />
              </span>
              <span className="t-small">{t(presetKey(id))}</span>
            </button>
          ))}
        </div>
      </section>

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

