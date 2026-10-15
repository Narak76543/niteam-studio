import { Check, SpellCheck, X } from "lucide-react";

import { Button } from "./Button";
import { useI18n } from "../i18n/context";
import { diffText } from "../lib/textDiff";
import type { Language, ProofSuggestion } from "../lib/types";

interface Props {
  before: { narration: string; spoken_text: string };
  suggestion: ProofSuggestion;
  lang: Language;
  onAccept: () => void;
  onDismiss: () => void;
}

function Diff({ before, after, lang }: { before: string; after: string; lang: Language }) {
  return (
    <p className="diff" lang={lang}>
      {diffText(before, after).map((p, i) =>
        p.kind === "same" ? (
          <span key={i}>{p.text}</span>
        ) : p.kind === "ins" ? (
          <ins key={i} className="diff__ins">
            {p.text}
          </ins>
        ) : (
          <del key={i} className="diff__del">
            {p.text}
          </del>
        ),
      )}
    </p>
  );
}

/** Proofreader's correction for the selected scene: what changes, why, Accept / Dismiss. */
export function SuggestionCard({ before, suggestion, lang, onAccept, onDismiss }: Props) {
  const { t } = useI18n();
  const spokenChanged = suggestion.spoken_text !== before.spoken_text;
  const narrationChanged = suggestion.narration !== before.narration;
  return (
    <section className="suggestion" aria-label={t("proof.suggestion")}>
      <div className="suggestion__head">
        <SpellCheck size={16} strokeWidth={1.5} aria-hidden="true" />
        <strong className="t-small">{t("proof.suggestion")}</strong>
      </div>
      {suggestion.note && (
        <p className="t-small suggestion__note" lang={lang}>
          {suggestion.note}
        </p>
      )}
      {narrationChanged && (
        <div className="field">
          <span className="t-micro t-muted">{t("script.tab.narration")}</span>
          <Diff before={before.narration} after={suggestion.narration} lang={lang} />
        </div>
      )}
      {spokenChanged && (
        <div className="field">
          <span className="t-micro t-muted">{t("script.tab.spoken")}</span>
          <Diff before={before.spoken_text} after={suggestion.spoken_text} lang={lang} />
        </div>
      )}
      <div className="row">
        <Button variant="primary" icon={<Check size={16} strokeWidth={1.5} />} onClick={onAccept}>
          {t("proof.accept")}
        </Button>
        <Button icon={<X size={16} strokeWidth={1.5} />} onClick={onDismiss}>
          {t("proof.dismiss")}
        </Button>
      </div>
    </section>
  );
}
