import { Palette, Plus, RefreshCw, X } from "lucide-react";
import { useState } from "react";

import { Button, IconButton } from "./Button";
import { ConfirmDialog } from "./ConfirmDialog";
import { EmptyState } from "./EmptyState";
import { Notice } from "./Notice";
import { useToast } from "./toast";
import { useI18n } from "../i18n/context";
import { api, errorMessage } from "../lib/api";
import type { Project, VisualBible } from "../lib/types";

const ICON = { size: 16, strokeWidth: 1.5 } as const;
const MAX_SEED = 999_999;

interface Entry {
  key: string;
  description: string;
}

interface Draft {
  style: string;
  characters: Entry[];
  places: Entry[];
  culture_notes: string;
  seed: number;
}

function toDraft(b: VisualBible): Draft {
  const entries = (m: Record<string, string>) => Object.entries(m).map(([key, description]) => ({ key, description }));
  return {
    style: b.style,
    characters: entries(b.characters),
    places: entries(b.places),
    culture_notes: b.culture_notes,
    seed: b.seed,
  };
}

function toBible(d: Draft): VisualBible {
  const record = (list: Entry[]) =>
    Object.fromEntries(list.filter((e) => e.key.trim() && e.description.trim()).map((e) => [e.key.trim(), e.description]));
  return {
    style: d.style,
    characters: record(d.characters),
    places: record(d.places),
    culture_notes: d.culture_notes,
    seed: d.seed,
  };
}

interface Props {
  project: Project;
  onChange: (p: Project) => void;
}

/** Script page → "Look & feel": the project's Visual Bible (docs/07-image-generation.md §2). */
export function VisualBibleEditor({ project, onChange }: Props) {
  const { t } = useI18n();
  const toast = useToast();
  const stored = project.visual_bible;
  const [draft, setDraft] = useState<Draft | null>(stored ? toDraft(stored) : null);
  const [busy, setBusy] = useState<"saving" | "generating" | null>(null);
  const [confirmRegenerate, setConfirmRegenerate] = useState(false);

  const locked = project.status === "rendering";
  const dirty = draft !== null && stored !== null && JSON.stringify(toBible(draft)) !== JSON.stringify(stored);

  const save = async () => {
    if (!draft) return;
    setBusy("saving");
    try {
      const p = await api.visualBibleUpdate(project.id, toBible(draft));
      onChange(p);
      if (p.visual_bible) setDraft(toDraft(p.visual_bible));
      toast.show(t("common.saved"), "ok");
    } catch (e) {
      toast.show(errorMessage(e), "danger");
    } finally {
      setBusy(null);
    }
  };

  const generate = async () => {
    setConfirmRegenerate(false);
    setBusy("generating");
    try {
      const p = await api.visualBibleGenerate(project.id);
      onChange(p);
      if (p.visual_bible) setDraft(toDraft(p.visual_bible));
    } catch (e) {
      toast.show(errorMessage(e), "danger");
    } finally {
      setBusy(null);
    }
  };

  if (!draft) {
    return (
      <EmptyState
        icon={<Palette size={18} strokeWidth={1.5} />}
        title={t("look.empty.title")}
        body={t("look.empty.body")}
        action={
          <Button variant="primary" disabled={busy !== null || locked} onClick={() => void generate()}>
            {busy === "generating" ? t("look.generating") : t("look.generate")}
          </Button>
        }
      />
    );
  }

  const set = (patch: Partial<Draft>) => setDraft({ ...draft, ...patch });

  const entryList = (field: "characters" | "places", label: string, hint: string, addLabel: string) => {
    const list = draft[field];
    const update = (i: number, patch: Partial<Entry>) =>
      set({ [field]: list.map((e, j) => (j === i ? { ...e, ...patch } : e)) });
    return (
      <div className="field">
        <span className="field__label">{label}</span>
        <span className="t-small">{hint}</span>
        {list.map((e, i) => (
          <div key={i} className="bible-entry">
            <input
              className="input bible-entry__key"
              lang="en"
              spellCheck={false}
              aria-label={t("look.key")}
              placeholder={t("look.key")}
              value={e.key}
              readOnly={locked}
              onChange={(ev) => update(i, { key: ev.target.value })}
            />
            <textarea
              className="textarea bible-entry__desc"
              lang="en"
              aria-label={t("look.description")}
              placeholder={t("look.description")}
              value={e.description}
              readOnly={locked}
              onChange={(ev) => update(i, { description: ev.target.value })}
            />
            <IconButton
              label={t("look.remove")}
              icon={<X {...ICON} />}
              disabled={locked}
              onClick={() => set({ [field]: list.filter((_, j) => j !== i) })}
            />
          </div>
        ))}
        <div className="row">
          <Button
            variant="pill"
            icon={<Plus size={12} strokeWidth={1.5} />}
            disabled={locked}
            onClick={() => set({ [field]: [...list, { key: "", description: "" }] })}
          >
            {addLabel}
          </Button>
        </div>
      </div>
    );
  };

  return (
    <section className="editor look" aria-label={t("look.title")}>
      <div className="editor__toolbar">
        <div className="row">
          <h2 className="t-h2">{t("look.title")}</h2>
          {busy === "saving" && <span className="t-micro t-muted">{t("common.saving")}</span>}
        </div>
        <div className="row">
          <Button
            icon={<RefreshCw {...ICON} />}
            disabled={busy !== null || locked}
            onClick={() => setConfirmRegenerate(true)}
          >
            {busy === "generating" ? t("look.generating") : t("look.regenerate")}
          </Button>
          <Button variant="primary" disabled={!dirty || busy !== null || locked || !draft.style.trim()} onClick={() => void save()}>
            {t("common.save")}
          </Button>
        </div>
      </div>

      <Notice>{t("look.intro")}</Notice>
      {dirty && <Notice tone="warn">{t("look.staleNote")}</Notice>}

      <label className="field">
        <span className="field__label">{t("look.style")}</span>
        <textarea
          className="textarea"
          lang="en"
          value={draft.style}
          readOnly={locked}
          onChange={(e) => set({ style: e.target.value })}
        />
        <span className="t-small">{t("look.styleHint")}</span>
      </label>

      {entryList("characters", t("look.characters"), t("look.charactersHint"), t("look.addCharacter"))}
      {entryList("places", t("look.places"), t("look.placesHint"), t("look.addPlace"))}

      <label className="field">
        <span className="field__label">{t("look.culture")}</span>
        <textarea
          className="textarea"
          lang="en"
          value={draft.culture_notes}
          readOnly={locked}
          onChange={(e) => set({ culture_notes: e.target.value })}
        />
        <span className="t-small">{t("look.cultureHint")}</span>
      </label>

      <label className="field look__seed">
        <span className="field__label">{t("look.seed")}</span>
        <input
          className="input"
          type="number"
          min={0}
          max={MAX_SEED}
          value={draft.seed}
          readOnly={locked}
          onChange={(e) => set({ seed: Math.min(MAX_SEED, Math.max(0, Math.round(Number(e.target.value) || 0))) })}
        />
        <span className="t-small">{t("look.seedHint")}</span>
      </label>

      {confirmRegenerate && (
        <ConfirmDialog
          title={t("look.regenerateTitle")}
          body={t("look.regenerateBody")}
          confirmLabel={t("look.regenerate")}
          danger
          onConfirm={() => void generate()}
          onCancel={() => setConfirmRegenerate(false)}
        />
      )}
    </section>
  );
}
