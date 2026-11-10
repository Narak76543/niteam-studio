import { Check } from "lucide-react";
import type { ReactNode } from "react";

import { GenArt, type GenKind } from "./GenArt";
import { ProgressBar } from "./ProgressBar";

export interface GenStep {
  label: string;
  state: "done" | "active" | "todo";
}

interface Props {
  kind: GenKind;
  title: string;
  /** Count, speed, ETA… (small, muted). */
  detail?: ReactNode;
  /** 0..1; omit for an indeterminate bar. */
  value?: number;
  /** Optional stage row (e.g. Outline → Plan → Chapters). */
  steps?: GenStep[];
  /** Stop button etc. */
  action?: ReactNode;
  /** Retry notice under the bar. */
  children?: ReactNode;
}

/** One running job (script, voices, images…) as a card with a looping illustration. */
export function GenerationCard({ kind, title, detail, value, steps, action, children }: Props) {
  return (
    <section className={`gen-card gen-card--${kind}`} aria-live="polite">
      <GenArt kind={kind} />
      <div className="gen-card__body">
        <div className="gen-card__head">
          <span className="t-h3">{title}</span>
          {detail && <span className="t-small gen-card__detail">{detail}</span>}
        </div>
        {steps && (
          <ol className="gen-steps">
            {steps.map((s) => (
              <li key={s.label} className={`gen-steps__item gen-steps__item--${s.state}`}>
                <span className="gen-steps__dot" aria-hidden="true">
                  {s.state === "done" && <Check size={10} strokeWidth={3} />}
                </span>
                <span className="t-micro">{s.label}</span>
              </li>
            ))}
          </ol>
        )}
        <ProgressBar label={title} value={value} />
        {children}
      </div>
      {action && <div className="gen-card__action">{action}</div>}
    </section>
  );
}
