import { Download, FileText, Languages, LayoutGrid, Plus, Search, Settings, X } from "lucide-react";
import type { ReactNode } from "react";

import appIcon from "../assets/brand/app-icon.png";
import { Button, IconButton } from "./Button";
import { useI18n } from "../i18n/context";
import type { Navigate, Route } from "../lib/route";
import type { Language, ProjectSummary } from "../lib/types";
import type { Updater } from "../lib/updater";

const RECENT_COUNT = 5;

interface Props {
  route: Route;
  navigate: Navigate;
  recent: ProjectSummary[];
  onSearch: () => void;
  onToggleLanguage: () => void;
  /** Icons only (the video editor needs the width). */
  compact?: boolean;
  updater: Updater;
}

function NavItem({
  icon,
  label,
  active,
  onClick,
  kbd,
  lang,
  className,
  hint,
}: {
  icon: ReactNode;
  label: string;
  active?: boolean;
  onClick: () => void;
  kbd?: string;
  lang?: Language;
  className?: string;
  /** Tooltip / accessible name when the visible label is shortened. */
  hint?: string;
}) {
  return (
    <button
      className={className ? `nav-item ${className}` : "nav-item"}
      aria-current={active ? "page" : undefined}
      onClick={onClick}
      title={hint ?? label}
      aria-label={hint ?? label}
    >
      {icon}
      <span className="nav-item__label" lang={lang}>
        {label}
      </span>
      {kbd && <span className="nav-item__kbd t-micro">{kbd}</span>}
    </button>
  );
}

const ICON = { size: 18, strokeWidth: 1.5 } as const;

/** "Update available" card above the footer (an icon with a dot when the sidebar is compact). */
function UpdateNotice({ updater }: { updater: Updater }) {
  const { t } = useI18n();
  if (!updater.update || updater.dismissed) return null;
  const label = t("update.available");
  return (
    <>
      <button className="nav-item nav-item--update" onClick={updater.openDialog} title={label} aria-label={label}>
        <Download {...ICON} />
        <span className="nav-item__dot" aria-hidden="true" />
      </button>
      <div className="update-card">
        <div className="update-card__head">
          <span className="badge__dot update-card__dot" aria-hidden="true" />
          <span className="update-card__title">{label}</span>
          <IconButton
            className="update-card__close"
            label={t("update.later")}
            icon={<X size={14} strokeWidth={1.75} />}
            onClick={updater.dismiss}
          />
        </div>
        <span className="t-micro t-muted">{t("update.version", { version: updater.update.version })}</span>
        <Button variant="pill" icon={<Download size={13} strokeWidth={1.75} />} onClick={updater.openDialog}>
          {t("update.view")}
        </Button>
      </div>
    </>
  );
}

export function Sidebar({ route, navigate, recent, onSearch, onToggleLanguage, compact, updater }: Props) {
  const { t, lang } = useI18n();
  const otherLang: Language = lang === "km" ? "en" : "km";
  return (
    <aside className={compact ? "sidebar sidebar--compact" : "sidebar"}>
      <div className="sidebar__top">
        <div className="sidebar__brand">
          <img className="sidebar__logo" src={appIcon} alt="" />
          <span className="sidebar__brand-name">{t("app.name")}</span>
        </div>

        <NavItem
          className="nav-item--search"
          icon={<Search size={16} strokeWidth={1.75} />}
          label={t("nav.search")}
          onClick={onSearch}
          kbd="Ctrl K"
        />
      </div>

      <div className="sidebar__group">
        <div className="sidebar__group-title">{t("nav.essentials")}</div>
        <NavItem
          icon={<LayoutGrid {...ICON} />}
          label={t("nav.projects")}
          active={route.name === "projects"}
          onClick={() => navigate({ name: "projects" })}
        />
        <NavItem
          icon={<Plus {...ICON} />}
          label={t("nav.newProject")}
          active={route.name === "new"}
          onClick={() => navigate({ name: "new" })}
        />
        <NavItem
          icon={<Settings {...ICON} />}
          label={t("nav.settings")}
          active={route.name === "settings"}
          onClick={() => navigate({ name: "settings" })}
        />
      </div>

      {recent.length > 0 && (
        <div className="sidebar__group">
          <div className="sidebar__group-title">{t("nav.recent")}</div>
          {recent.slice(0, RECENT_COUNT).map((p) => (
            <NavItem
              key={p.id}
              icon={<FileText {...ICON} />}
              label={p.title}
              lang={p.language}
              active={(route.name === "script" || route.name === "scenes" || route.name === "render") && route.projectId === p.id}
              onClick={() =>
                navigate({ name: p.status === "script_draft" ? "script" : "scenes", projectId: p.id })
              }
            />
          ))}
        </div>
      )}

      <div className="sidebar__spacer" />

      <UpdateNotice updater={updater} />

      <div className="sidebar__footer">
        <NavItem
          icon={<Languages {...ICON} />}
          label={t(`lang.${otherLang}`)}
          hint={`${t("nav.language")}: ${t(`lang.${otherLang}`)}`}
          onClick={onToggleLanguage}
        />
      </div>
    </aside>
  );
}
