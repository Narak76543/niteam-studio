import { useI18n } from "../i18n/context";

type Layout = "grid" | "split" | "editor";

/** Pulsing placeholder shaped like the page that is loading (docs/06-ui-design.md §5 Motion). */
export function PageSkeleton({ layout }: { layout: Layout }) {
  const { t } = useI18n();
  return (
    <div className="page" role="status" aria-busy="true">
      <span className="visually-hidden">{t("common.loading")}</span>
      {layout !== "editor" && (
        <div className="skeleton-header" aria-hidden="true">
          <span className="skeleton skeleton--line" style={{ width: 80 }} />
          <span className="skeleton skeleton--title" />
          <span className="skeleton skeleton--line" style={{ width: 260 }} />
        </div>
      )}
      {layout === "grid" && (
        <div className="scene-grid" aria-hidden="true">
          {Array.from({ length: 8 }, (_, i) => (
            <div key={i} className="skeleton-card">
              <span className="skeleton skeleton--media" />
              <span className="skeleton skeleton--line" />
              <span className="skeleton skeleton--line" style={{ width: "60%" }} />
            </div>
          ))}
        </div>
      )}
      {layout === "split" && (
        <div className="skeleton-split" aria-hidden="true">
          <div className="skeleton-stack">
            {Array.from({ length: 7 }, (_, i) => (
              <span key={i} className="skeleton skeleton--row" />
            ))}
          </div>
          <div className="skeleton-stack">
            <span className="skeleton skeleton--line" style={{ width: 160 }} />
            <span className="skeleton skeleton--block" />
            <span className="skeleton skeleton--line" style={{ width: 200 }} />
            <span className="skeleton skeleton--block" />
          </div>
        </div>
      )}
      {layout === "editor" && (
        <div className="skeleton-editor" aria-hidden="true">
          <span className="skeleton skeleton--line" style={{ width: 320 }} />
          <div className="skeleton-editor__main">
            <span className="skeleton skeleton--video" />
            <div className="skeleton-stack">
              <span className="skeleton skeleton--line" style={{ width: 120 }} />
              <span className="skeleton skeleton--line" />
              <span className="skeleton skeleton--line" style={{ width: "70%" }} />
            </div>
          </div>
          <span className="skeleton skeleton--timeline" />
        </div>
      )}
    </div>
  );
}
