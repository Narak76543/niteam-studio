import { Sparkles } from "lucide-react";
import { useState } from "react";

import { Button } from "../components/Button";
import { Notice } from "../components/Notice";
import { PageHeader } from "../components/PageHeader";
import { PresetIcon } from "../components/PresetIcon";
import { Stepper } from "../components/Stepper";
import { useToast } from "../components/toast";
import { useI18n } from "../i18n/context";
import { api, errorMessage } from "../lib/api";
import { estimateScenes } from "../lib/format";
import { PRESET_IDS, isPresetId, presetKey, type PresetId } from "../lib/presets";
import type { Navigate } from "../lib/route";
import type { Language } from "../lib/types";

const MIN_MINUTES = 1;
const MAX_MINUTES = 20;
const DEFAULT_MINUTES = 3;
const EXAMPLES = ["wizard.topic.example1", "wizard.topic.example2", "wizard.topic.example3"] as const;
const LANGUAGES: Language[] = ["km", "en"];

interface Props {
  initialPreset?: string;
  hasKey: boolean;
  navigate: Navigate;
}

export function NewProjectPage({ initialPreset, hasKey, navigate }: Props) {
  const { t } = useI18n();
  const toast = useToast();
  const [step, setStep] = useState(0);
  const [topic, setTopic] = useState("");
  const [language, setLanguage] = useState<Language>("km");
  const [minutes, setMinutes] = useState(DEFAULT_MINUTES);
  const [preset, setPreset] = useState<PresetId>(
    initialPreset && isPresetId(initialPreset) ? initialPreset : "stickman",
  );
  const [busy, setBusy] = useState(false);

  const steps = [t("wizard.step.topic"), t("wizard.step.format"), t("wizard.step.style")];
  const canNext = step !== 0 || topic.trim().length > 0;
  const isLast = step === steps.length - 1;

  const submit = async () => {
    setBusy(true);
    let projectId: string;
    try {
      const project = await api.projectCreate({
        topic,
        language,
        target_minutes: minutes,
        style_preset: preset,
      });
      projectId = project.id;
    } catch (e) {
      toast.show(errorMessage(e), "danger");
      setBusy(false);
      return;
    }
    if (hasKey) {
      // The project exists either way; a failed start is shown and retried on the script page.
      await api.scriptGenerate(projectId, false).catch((e: unknown) => {
        toast.show(errorMessage(e), "danger");
      });
    }
    navigate({ name: "script", projectId });
  };

  return (
    <div className="page">
      <div className="wizard">
        <PageHeader
          breadcrumb={[{ label: t("nav.projects"), onClick: () => navigate({ name: "projects" }) }]}
          title={t("wizard.title")}
        />
        <Stepper steps={steps} current={step} />

        <form
          className="card"
          onSubmit={(e) => {
            e.preventDefault();
            if (!canNext || busy) return;
            if (isLast) void submit();
            else setStep(step + 1);
          }}
        >
          {step === 0 && (
            <>
              <label className="field">
                <span className="field__label">{t("wizard.topic.label")}</span>
                <textarea
                  className="textarea"
                  value={topic}
                  maxLength={500}
                  autoFocus
                  placeholder={t("wizard.topic.placeholder")}
                  onChange={(e) => setTopic(e.target.value)}
                />
              </label>
              <div className="field">
                <span className="t-small">{t("wizard.topic.examples")}</span>
                <div className="chips">
                  {EXAMPLES.map((k) => (
                    <Button key={k} variant="pill" onClick={() => setTopic(t(k))}>
                      {t(k)}
                    </Button>
                  ))}
                </div>
              </div>
            </>
          )}

          {step === 1 && (
            <>
              <div className="field">
                <span className="field__label">{t("wizard.language.label")}</span>
                <div className="segmented" role="group" aria-label={t("wizard.language.label")}>
                  {LANGUAGES.map((l) => (
                    <button
                      key={l}
                      type="button"
                      aria-pressed={language === l}
                      lang={l}
                      onClick={() => setLanguage(l)}
                    >
                      {t(`lang.${l}`)}
                    </button>
                  ))}
                </div>
              </div>
              <label className="field">
                <span className="field__label">{t("wizard.length.label")}</span>
                <span className="t-h2">{t("wizard.length.value", { n: minutes })}</span>
                <input
                  type="range"
                  min={MIN_MINUTES}
                  max={MAX_MINUTES}
                  step={1}
                  value={minutes}
                  onChange={(e) => setMinutes(Number(e.target.value))}
                />
                <span className="t-small">
                  {t("wizard.length.scenes", { n: estimateScenes(minutes) })}
                </span>
              </label>
            </>
          )}

          {step === 2 && (
            <>
              <div className="field">
                <span className="field__label">{t("wizard.style.label")}</span>
                <div className="option-grid">
                  {PRESET_IDS.map((id) => (
                    <button
                      key={id}
                      type="button"
                      className="option"
                      aria-pressed={preset === id}
                      onClick={() => setPreset(id)}
                    >
                      <span className="shortcut__art">
                        <PresetIcon id={id} />
                      </span>
                      <span className="t-small">{t(presetKey(id))}</span>
                    </button>
                  ))}
                </div>
              </div>
              {!hasKey && <Notice tone="warn">{t("wizard.noKey")}</Notice>}
            </>
          )}

          <div className="wizard__footer">
            <Button disabled={step === 0 || busy} onClick={() => setStep(step - 1)}>
              {t("common.back")}
            </Button>
            <Button
              type="submit"
              variant="primary"
              disabled={!canNext || busy}
              icon={isLast && hasKey ? <Sparkles size={16} strokeWidth={1.5} /> : undefined}
            >
              {busy
                ? t("wizard.creating")
                : isLast
                  ? hasKey
                    ? t("wizard.generate")
                    : t("wizard.createOnly")
                  : t("common.next")}
            </Button>
          </div>
        </form>
      </div>
    </div>
  );
}
