import { FileUp, Sparkles } from "lucide-react";
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
import type { Audience, Language, PdfInfo } from "../lib/types";

const MIN_MINUTES = 1;
const MAX_MINUTES = 20;
const DEFAULT_MINUTES = 3;
const EXAMPLES = ["wizard.topic.example1", "wizard.topic.example2", "wizard.topic.example3"] as const;
const LANGUAGES: Language[] = ["km", "en"];
const AUDIENCES: Audience[] = ["general", "students", "experts"];
type Mode = "topic" | "document";

function sizeMb(bytes: number): string {
  return (bytes / (1024 * 1024)).toFixed(1);
}

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
  const [mode, setMode] = useState<Mode>("topic");
  const [pdfPath, setPdfPath] = useState<string | null>(null);
  const [pdf, setPdf] = useState<PdfInfo | null>(null);
  const [pdfError, setPdfError] = useState<string | null>(null);
  const [audience, setAudience] = useState<Audience>("general");
  const [focus, setFocus] = useState("");
  const [pageFrom, setPageFrom] = useState(1);
  const [pageTo, setPageTo] = useState(1);

  const steps = [
    mode === "topic" ? t("wizard.step.topic") : t("wizard.step.document"),
    t("wizard.step.format"),
    t("wizard.step.style"),
  ];
  const rangeOk =
    pdf !== null &&
    (!pdf.needs_range ||
      (pageFrom >= 1 && pageTo >= pageFrom && pageTo <= pdf.page_count && pageTo - pageFrom + 1 <= pdf.max_pages));
  const canNext = step !== 0 || (mode === "topic" ? topic.trim().length > 0 : rangeOk);

  const pickPdf = async () => {
    const path = await api.pickPdfFile();
    if (!path) return;
    setPdfError(null);
    setPdf(null);
    try {
      const info = await api.sourcePdfInfo(path);
      setPdfPath(path);
      setPdf(info);
      setPageFrom(1);
      setPageTo(Math.min(info.page_count, info.max_pages));
    } catch (e) {
      setPdfPath(null);
      setPdfError(errorMessage(e));
    }
  };

  const submitDocument = async () => {
    if (!pdfPath || !pdf) return;
    setBusy(true);
    let projectId: string;
    try {
      const project = await api.projectCreateFromDocument({
        path: pdfPath,
        language,
        target_minutes: minutes,
        style_preset: preset,
        audience,
        focus,
        page_from: pdf.needs_range ? pageFrom : null,
        page_to: pdf.needs_range ? pageTo : null,
      });
      projectId = project.id;
    } catch (e) {
      toast.show(errorMessage(e), "danger");
      setBusy(false);
      return;
    }
    if (hasKey) {
      // Reading only: nothing else is generated until the points are confirmed.
      await api.sourceRead(projectId).catch((e: unknown) => toast.show(errorMessage(e), "danger"));
    }
    navigate({ name: "script", projectId });
  };
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
            if (isLast) void (mode === "topic" ? submit() : submitDocument());
            else setStep(step + 1);
          }}
        >
          {step === 0 && (
            <div className="segmented" role="group" aria-label={t("wizard.mode.label")}>
              {(["topic", "document"] as const).map((m) => (
                <button key={m} type="button" aria-pressed={mode === m} onClick={() => setMode(m)}>
                  {t(m === "topic" ? "wizard.mode.topic" : "wizard.mode.document")}
                </button>
              ))}
            </div>
          )}

          {step === 0 && mode === "document" && (
            <>
              <div className="field">
                <span className="field__label">{t("wizard.doc.label")}</span>
                <div className="row">
                  <Button icon={<FileUp size={16} strokeWidth={1.5} />} onClick={() => void pickPdf()}>
                    {pdf ? t("wizard.doc.change") : t("wizard.doc.choose")}
                  </Button>
                  {pdf && (
                    <span className="t-small">
                      {t("wizard.doc.info", { name: pdf.file_name, pages: pdf.page_count, mb: sizeMb(pdf.size_bytes) })}
                    </span>
                  )}
                </div>
                {pdfError && <Notice tone="danger">{pdfError}</Notice>}
                <span className="t-small t-muted">{t("wizard.doc.limits")}</span>
              </div>
              {pdf?.needs_range && (
                <div className="field">
                  <span className="field__label">{t("wizard.doc.range", { max: pdf.max_pages })}</span>
                  <div className="row">
                    <input
                      className="input input--number"
                      type="number"
                      min={1}
                      max={pdf.page_count}
                      aria-label={t("wizard.doc.from")}
                      value={pageFrom}
                      onChange={(e) => setPageFrom(Number(e.target.value))}
                    />
                    <span>–</span>
                    <input
                      className="input input--number"
                      type="number"
                      min={1}
                      max={pdf.page_count}
                      aria-label={t("wizard.doc.to")}
                      value={pageTo}
                      onChange={(e) => setPageTo(Number(e.target.value))}
                    />
                    <span className="t-small t-muted">{t("wizard.doc.of", { n: pdf.page_count })}</span>
                  </div>
                  {!rangeOk && <Notice tone="warn">{t("wizard.doc.rangeHint", { max: pdf.max_pages })}</Notice>}
                </div>
              )}
              <div className="field">
                <span className="field__label">{t("wizard.doc.audience")}</span>
                <div className="segmented" role="group" aria-label={t("wizard.doc.audience")}>
                  {AUDIENCES.map((a) => (
                    <button key={a} type="button" aria-pressed={audience === a} onClick={() => setAudience(a)}>
                      {t(`wizard.audience.${a}`)}
                    </button>
                  ))}
                </div>
              </div>
              <label className="field">
                <span className="field__label">{t("wizard.doc.focus")}</span>
                <input
                  className="input"
                  maxLength={300}
                  placeholder={t("wizard.doc.focusPlaceholder")}
                  value={focus}
                  onChange={(e) => setFocus(e.target.value)}
                />
              </label>
              <Notice tone="warn">{t("wizard.doc.freeTier")}</Notice>
            </>
          )}

          {step === 0 && mode === "topic" && (
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
                <span className="field__label">
                  {mode === "document" ? t("wizard.doc.language") : t("wizard.language.label")}
                </span>
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
                    ? mode === "document"
                      ? t("wizard.doc.read")
                      : t("wizard.generate")
                    : t("wizard.createOnly")
                  : t("common.next")}
            </Button>
          </div>
        </form>
      </div>
    </div>
  );
}
