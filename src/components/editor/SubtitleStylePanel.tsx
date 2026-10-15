import { Check } from "lucide-react";

import { useI18n } from "../../i18n/context";
import { SUBTITLE_FONT_FAMILY } from "../../lib/subtitleCanvas";

/** Word-tracker colours (yellow is the default, `DEFAULT_HIGHLIGHT` in Rust). */
const HIGHLIGHT_COLORS = ["#FFD60A", "#E8553E", "#3DDC84", "#4FC3F7", "#FF6FB5"] as const;

interface Props {
  highlight: string | null;
  disabled: boolean;
  onChange: (highlight: string | null) => void;
}

/** Right panel → Text: subtitle font and the word tracker. */
export function SubtitleStylePanel({ highlight, disabled, onChange }: Props) {
  const { t } = useI18n();
  const on = highlight !== null;
  return (
    <section className="panel-section" aria-label={t("editor.text")}>
      <h3 className="panel-section__title">{t("editor.text")}</h3>
      <div className="panel-field">
        <span className="t-small t-muted">{t("editor.font")}</span>
        <span className="panel-font" style={{ fontFamily: `"${SUBTITLE_FONT_FAMILY}"` }}>
          Kantumruy Pro · ក ខ គ
        </span>
      </div>
      <label className="panel-switch">
        <span>
          <span className="t-small">{t("editor.tracker")}</span>
          <span className="t-micro t-muted panel-switch__hint">{t("editor.trackerHint")}</span>
        </span>
        <input
          type="checkbox"
          role="switch"
          checked={on}
          disabled={disabled}
          onChange={(e) => onChange(e.target.checked ? (highlight ?? HIGHLIGHT_COLORS[0]) : null)}
        />
      </label>
      {on && (
        <div className="panel-field">
          <span className="t-small t-muted">{t("editor.trackerColor")}</span>
          <div className="swatches" role="radiogroup" aria-label={t("editor.trackerColor")}>
            {HIGHLIGHT_COLORS.map((c) => (
              <button
                key={c}
                type="button"
                role="radio"
                aria-checked={highlight === c}
                aria-label={c}
                className="swatch"
                style={{ background: c }}
                disabled={disabled}
                onClick={() => onChange(c)}
              >
                {highlight === c && <Check size={14} strokeWidth={2.5} />}
              </button>
            ))}
          </div>
        </div>
      )}
    </section>
  );
}
