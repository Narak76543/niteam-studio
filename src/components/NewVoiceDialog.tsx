import { FileAudio, Play, Sparkles } from "lucide-react";
import { useEffect, useState } from "react";

import { Button } from "./Button";
import { Notice } from "./Notice";
import { useToast } from "./toast";
import { useI18n } from "../i18n/context";
import { api, errorMessage } from "../lib/api";
import { playAudio } from "../lib/audioPlayer";
import type { Language } from "../lib/types";

const ICON = { size: 14, strokeWidth: 1.5 } as const;

type Source = "design" | "recording";

interface Props {
  onCreated: () => void;
  onCancel: () => void;
}

/** New Voice Profile: from a description (pick one of 3 samples) or from the user's own
 * recording (consent required). docs/09-voice-profiles.md */
export function NewVoiceDialog({ onCreated, onCancel }: Props) {
  const { t } = useI18n();
  const toast = useToast();
  const [source, setSource] = useState<Source>("design");
  const [name, setName] = useState("");
  const [language, setLanguage] = useState<Language>("km");
  const [description, setDescription] = useState("");
  const [samples, setSamples] = useState(0);
  const [chosen, setChosen] = useState<number | null>(null);
  const [file, setFile] = useState<string | null>(null);
  const [transcript, setTranscript] = useState("");
  const [consent, setConsent] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && busy === null) onCancel();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel, busy]);

  const run = async (key: string, fn: () => Promise<void>) => {
    setBusy(key);
    try {
      await fn();
    } catch (e) {
      toast.show(errorMessage(e), "danger");
    } finally {
      setBusy(null);
    }
  };

  const makeSamples = () =>
    run("samples", async () => {
      setSamples(0);
      setChosen(null);
      const n = await api.voiceDesignSamples(language, description);
      setSamples(n);
      setChosen(0);
    });

  const play = (i: number) =>
    run(`play-${i}`, async () => {
      setChosen(i);
      await playAudio(`design-${i}`, await api.voiceDesignSampleAudio(i), "audio/wav");
    });

  const pick = () =>
    run("pick", async () => {
      const p = await api.pickAudioFile();
      if (p) setFile(p);
    });

  const create = () =>
    run("create", async () => {
      if (source === "design") {
        if (chosen === null) return;
        await api.voiceProfileCreateDesign(name, language, description, chosen);
      } else {
        if (!file) return;
        await api.voiceProfileImport(file, name, language, transcript, consent);
      }
      toast.show(t("voices.created"), "ok");
      onCreated();
    });

  const canCreate =
    name.trim() !== "" && busy === null && (source === "design" ? chosen !== null && samples > 0 : file !== null && consent);
  const fileName = file?.split(/[\\/]/).pop() ?? "";

  return (
    <div className="backdrop" onMouseDown={(e) => e.target === e.currentTarget && busy === null && onCancel()}>
      <form
        className="dialog dialog--wide"
        role="dialog"
        aria-modal="true"
        aria-labelledby="new-voice-title"
        onSubmit={(e) => {
          e.preventDefault();
          if (canCreate) void create();
        }}
      >
        <h2 id="new-voice-title" className="t-h2">
          {t("voices.newTitle")}
        </h2>
        <div className="segmented" role="group" aria-label={t("voices.source")}>
          {(["design", "recording"] as const).map((s) => (
            <button key={s} type="button" aria-pressed={source === s} disabled={busy !== null} onClick={() => setSource(s)}>
              {s === "design" ? t("voices.fromDescription") : t("voices.fromRecording")}
            </button>
          ))}
        </div>

        <div className="models-grid">
          <label className="field">
            <span className="field__label">{t("voices.name")}</span>
            <input className="input" maxLength={60} value={name} onChange={(e) => setName(e.target.value)} />
          </label>
          <label className="field">
            <span className="field__label">{t("voices.language")}</span>
            <select className="select" value={language} onChange={(e) => setLanguage(e.target.value === "en" ? "en" : "km")}>
              <option value="km">{t("lang.km")}</option>
              <option value="en">{t("lang.en")}</option>
            </select>
          </label>
        </div>

        {source === "design" ? (
          <>
            <label className="field">
              <span className="field__label">{t("voices.description")}</span>
              <textarea
                className="textarea"
                lang="en"
                maxLength={300}
                value={description}
                placeholder={t("voices.descriptionPlaceholder")}
                onChange={(e) => setDescription(e.target.value)}
              />
              <span className="t-small">{t("voices.descriptionHint")}</span>
            </label>
            <div className="row">
              <Button
                icon={<Sparkles {...ICON} />}
                disabled={busy !== null || !description.trim()}
                onClick={() => void makeSamples()}
              >
                {busy === "samples" ? t("voices.making") : samples > 0 ? t("voices.again") : t("voices.makeSamples")}
              </Button>
            </div>
            {busy === "samples" && <span className="t-small t-muted">{t("voices.makingHint")}</span>}
            {samples > 0 && (
              <div className="row" role="group" aria-label={t("voices.samples")}>
                {Array.from({ length: samples }, (_, i) => (
                  <Button
                    key={i}
                    variant="pill"
                    aria-pressed={chosen === i}
                    icon={<Play {...ICON} />}
                    disabled={busy !== null && busy !== `play-${i}`}
                    onClick={() => void play(i)}
                  >
                    {t("voices.sample", { n: i + 1 })}
                  </Button>
                ))}
              </div>
            )}
          </>
        ) : (
          <>
            <div className="row">
              <Button icon={<FileAudio {...ICON} />} disabled={busy !== null} onClick={() => void pick()}>
                {t("voices.chooseFile")}
              </Button>
              {file && <span className="t-small">{fileName}</span>}
            </div>
            <span className="t-small">{t("voices.recordingHint")}</span>
            <label className="field">
              <span className="field__label">{t("voices.transcript")}</span>
              <textarea
                className="textarea"
                lang={language}
                value={transcript}
                onChange={(e) => setTranscript(e.target.value)}
              />
              <span className="t-small">{t("voices.transcriptHint")}</span>
            </label>
            <label className="row">
              <input type="checkbox" checked={consent} onChange={(e) => setConsent(e.target.checked)} />
              <span className="t-small">{t("voices.consent")}</span>
            </label>
            {!consent && file && <Notice>{t("voices.consentNeeded")}</Notice>}
          </>
        )}

        <div className="dialog__actions">
          <Button disabled={busy !== null} onClick={onCancel}>
            {t("common.cancel")}
          </Button>
          <Button type="submit" variant="primary" disabled={!canCreate}>
            {busy === "create" ? t("voices.creating") : t("voices.create")}
          </Button>
        </div>
      </form>
    </div>
  );
}
