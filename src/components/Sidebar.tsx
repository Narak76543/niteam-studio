import { BookOpenText, FileText, Languages, LayoutGrid, Plus, Search, Settings } from "lucide-react";
import type { ReactNode } from "react";

import { IconTile } from "./IconTile";
import { useI18n } from "../i18n/context";
import type { Navigate, Route } from "../lib/route";
import type { Language, ProjectSummary } from "../lib/types";

const RECENT_COUNT = 5;

interface Props {
  route: Route;
  navigate: Navigate;
  recent: ProjectSummary[];
  onSearch: () => void;
  onToggleLanguage: () => void;
}

function NavItem({
  icon,
  label,
  active,
  onClick,
  kbd,
  lang,
}: {
  icon: ReactNode;
  label: string;
  active?: boolean;
  onClick: () => void;
  kbd?: string;
  lang?: Language;
}) {
  return (
    <button
      className="nav-item"
      aria-current={active ? "page" : undefined}
      onClick={onClick}
      title={label}
      aria-label={label}
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

export function Sidebar({ route, navigate, recent, onSearch, onToggleLanguage }: Props) {
  const { t, lang } = useI18n();
  const otherLang: Language = lang === "km" ? "en" : "km";
  return (
    <aside className="sidebar">
      <div className="sidebar__brand">
        <IconTile>
          <BookOpenText size={18} strokeWidth={1.5} />
        </IconTile>
        <span className="sidebar__brand-name">{t("app.name")}</span>
      </div>

      <NavItem icon={<Search {...ICON} />} label={t("nav.search")} onClick={onSearch} kbd="Ctrl K" />

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
              active={route.name === "script" && route.projectId === p.id}
              onClick={() => navigate({ name: "script", projectId: p.id })}
            />
          ))}
        </div>
      )}

      <div className="sidebar__spacer" />

      <NavItem
        icon={<Languages {...ICON} />}
        label={`${t("nav.language")}: ${t(`lang.${otherLang}`)}`}
        onClick={onToggleLanguage}
      />
    </aside>
  );
}
