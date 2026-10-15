import { Plus, X } from "lucide-react";
import { useState } from "react";

import { Button } from "./Button";
import { Notice } from "./Notice";
import { useToast } from "./toast";
import { useI18n } from "../i18n/context";
import { api, errorMessage } from "../lib/api";
import type { Project } from "../lib/types";

interface Props {
  project: Project;
  onChange: (p: Project) => void;
}

/** Script page → Names: product / brand / tech names that are never translated. */
export function KeepTermsEditor({ project, onChange }: Props) {
  const { t } = useI18n();
  const toast = useToast();
  const [input, setInput] = useState("");
  const terms = project.keep_terms;

  const save = async (next: string[]) => {
    try {
      onChange(await api.projectSetKeepTerms(project.id, next));
    } catch (e) {
      toast.show(errorMessage(e), "danger");
    }
  };

  const add = () => {
    const parts = input
      .split(",")
      .map((s) => s.trim())
      .filter(Boolean);
    if (parts.length === 0) return;
    setInput("");
    void save([...terms, ...parts]);
  };

  return (
    <section className="editor look" aria-label={t("names.title")}>
      <div className="editor__toolbar">
        <h2 className="t-h2">{t("names.title")}</h2>
      </div>
      <Notice>{t("names.intro")}</Notice>
      <form
        className="row"
        onSubmit={(e) => {
          e.preventDefault();
          add();
        }}
      >
        <input
          className="input input--grow"
          lang="en"
          spellCheck={false}
          placeholder={t("names.placeholder")}
          aria-label={t("names.placeholder")}
          value={input}
          onChange={(e) => setInput(e.target.value)}
        />
        <Button type="submit" icon={<Plus size={16} strokeWidth={1.5} />} disabled={!input.trim()}>
          {t("names.add")}
        </Button>
      </form>
      {terms.length === 0 ? (
        <p className="t-small t-muted">{t("names.empty")}</p>
      ) : (
        <div className="chips" role="list">
          {terms.map((term) => (
            <span key={term} className="term-chip" role="listitem" lang="en">
              {term}
              <button
                type="button"
                className="term-chip__remove"
                aria-label={t("names.remove", { name: term })}
                onClick={() => void save(terms.filter((x) => x !== term))}
              >
                <X size={12} strokeWidth={2} />
              </button>
            </span>
          ))}
        </div>
      )}
      <p className="t-small t-muted">{t("names.hint")}</p>
    </section>
  );
}
