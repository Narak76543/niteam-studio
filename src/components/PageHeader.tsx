import { ChevronRight } from "lucide-react";
import type { ReactNode } from "react";

interface Crumb {
  label: string;
  onClick?: () => void;
}

interface Props {
  breadcrumb: Crumb[];
  title: string;
  titleLang?: string;
  meta?: ReactNode;
  actions?: ReactNode;
}

export function PageHeader({ breadcrumb, title, titleLang, meta, actions }: Props) {
  return (
    <header className="page-header">
      <div className="page-header__titles">
        <nav className="breadcrumb" aria-label="Breadcrumb">
          {breadcrumb.map((c, i) => (
            <span key={c.label} className="breadcrumb">
              {i > 0 && <ChevronRight size={12} strokeWidth={1.5} aria-hidden="true" />}
              {c.onClick ? <button onClick={c.onClick}>{c.label}</button> : <span>{c.label}</span>}
            </span>
          ))}
        </nav>
        <h1 className="t-display" lang={titleLang} title={title}>
          {title}
        </h1>
        {meta}
      </div>
      {actions && <div className="page-header__actions">{actions}</div>}
    </header>
  );
}
