import { Play } from "lucide-react";
import { useEffect, useState } from "react";

import { Button } from "./Button";
import { Notice } from "./Notice";
import { useToast } from "./toast";
import { useI18n } from "../i18n/context";
import { api, errorMessage } from "../lib/api";
import { playAudio } from "../lib/audioPlayer";
import type { EngineKind, Language, Settings, VoiceCatalog, VoiceDefaults } from "../lib/types";

const ENGINES: EngineKind[] = ["edge", "gemini", "voxcpm"];

interface Props {
  settings: Settings;
  onSave: (s: Settings) => Promise<Settings>;
}

/** Settings → Voice: engine, voices, rate/pitch (Edge), model/style (Gemini), preview. */
export function VoiceSettingsSection({ settings, onSave }: Props) {
  const { t } = useI18n();
  const toast = useToast();
  const [voice, setVoice] = useState<VoiceDefaults>(settings.voice);
  const [catalog, setCatalog] = useState<VoiceCatalog | null>(null);
  const [previewing, setPreviewing] = useState<Language | null>(null);

  useEffect(() => {
    api
      .voicesCatalog()
      .then(setCatalog)
      .catch(() => setCatalog(null));
  }, []);

  // The default local voice is chosen in Settings → Local voices, not here.
  const current: VoiceDefaults = { ...voice, voxcpm_profile: settings.voice.voxcpm_profile };
  const dirty = JSON.stringify(current) !== JSON.stringify(settings.voice);
  const set = (patch: Partial<VoiceDefaults>) => setVoice({ ...voice, ...patch });
  const edgeVoices = (lang: Language) => catalog?.edge.filter((v) => v.language === lang) ?? [];
  const ttsModels = Array.from(new Set([settings.models.tts, settings.models.tts_lite, voice.gemini_model]));

  const save = async () => {
    try {
      await onSave({ ...settings, voice: current });
      toast.show(t("common.saved"), "ok");
    } catch (e) {
      toast.show(errorMessage(e), "danger");
    }
  };

  const preview = async (language: Language) => {
    setPreviewing(language);
    try {
      const bytes = await api.voicePreview(language, current);
      await playAudio(`preview-${language}`, bytes, voice.engine === "edge" ? "audio/mpeg" : "audio/wav");
    } catch (e) {
      toast.show(errorMessage(e), "danger");
    } finally {
      setPreviewing(null);
    }
  };

  return (
    <div className="settings-row">
      <div className="settings-row__intro">
        <h2 className="t-h3">{t("settings.voice.title")}</h2>
        <p className="t-small">{t("settings.voice.body")}</p>
      </div>
      <form
        className="settings-row__control"
        onSubmit={(e) => {
          e.preventDefault();
          void save();
        }}
      >
        <label className="field">
          <span className="t-small">{t("settings.voice.engine")}</span>
          <select
            className="select"
            value={voice.engine}
            onChange={(e) => set({ engine: ENGINES.find((k) => k === e.target.value) ?? "edge" })}
          >
            {ENGINES.map((k) => (
              <option key={k} value={k}>
                {t(`engine.${k}`)}
              </option>
            ))}
          </select>
        </label>

        {voice.engine === "voxcpm" ? (
          <Notice>{t("settings.voice.voxcpmHint")}</Notice>
        ) : voice.engine === "edge" ? (
          <>
            <div className="models-grid">
              <label className="field">
                <span className="t-small">{t("settings.voice.voiceKm")}</span>
                <select className="select" value={voice.edge_voice_km} onChange={(e) => set({ edge_voice_km: e.target.value })}>
                  {edgeVoices("km").map((v) => (
                    <option key={v.id} value={v.id}>
                      {v.label}
                    </option>
                  ))}
                </select>
              </label>
              <label className="field">
                <span className="t-small">{t("settings.voice.voiceEn")}</span>
                <select className="select" value={voice.edge_voice_en} onChange={(e) => set({ edge_voice_en: e.target.value })}>
                  {edgeVoices("en").map((v) => (
                    <option key={v.id} value={v.id}>
                      {v.label}
                    </option>
                  ))}
                </select>
              </label>
              <label className="field">
                <span className="t-small">
                  {t("settings.voice.rate")} · {t("settings.voice.rateValue", { n: voice.rate > 0 ? `+${voice.rate}` : voice.rate })}
                </span>
                <input
                  type="range"
                  min={-50}
                  max={50}
                  step={5}
                  value={voice.rate}
                  onChange={(e) => set({ rate: Number(e.target.value) })}
                />
              </label>
              <label className="field">
                <span className="t-small">
                  {t("settings.voice.pitch")} · {t("settings.voice.pitchValue", { n: voice.pitch > 0 ? `+${voice.pitch}` : voice.pitch })}
                </span>
                <input
                  type="range"
                  min={-50}
                  max={50}
                  step={5}
                  value={voice.pitch}
                  onChange={(e) => set({ pitch: Number(e.target.value) })}
                />
              </label>
            </div>
            <p className="t-small">{t("settings.voice.edgeHint")}</p>
          </>
        ) : (
          <>
            <div className="models-grid">
              <label className="field">
                <span className="t-small">{t("settings.voice.geminiVoice")}</span>
                <select className="select" value={voice.gemini_voice} onChange={(e) => set({ gemini_voice: e.target.value })}>
                  {(catalog?.gemini ?? []).map(([name, character]) => (
                    <option key={name} value={name}>
                      {name} — {character}
                    </option>
                  ))}
                </select>
              </label>
              <label className="field">
                <span className="t-small">{t("settings.voice.geminiModel")}</span>
                <select className="select" value={voice.gemini_model} onChange={(e) => set({ gemini_model: e.target.value })}>
                  {ttsModels.map((m) => (
                    <option key={m} value={m}>
                      {m}
                    </option>
                  ))}
                </select>
              </label>
            </div>
            <label className="field">
              <span className="t-small">{t("settings.voice.direction")}</span>
              <input
                className="input"
                lang="en"
                maxLength={300}
                value={voice.direction}
                onChange={(e) => set({ direction: e.target.value })}
              />
              <span className="t-small">{t("settings.voice.directionHint")}</span>
            </label>
          </>
        )}

        <div className="row">
          <Button type="submit" disabled={!dirty}>
            {t("common.save")}
          </Button>
          {(["km", "en"] as const).map((lang) => (
            <Button
              key={lang}
              variant="pill"
              icon={<Play size={12} strokeWidth={1.5} />}
              disabled={previewing !== null}
              onClick={() => void preview(lang)}
            >
              {previewing === lang
                ? t("settings.voice.previewing")
                : lang === "km"
                  ? t("settings.voice.previewKm")
                  : t("settings.voice.previewEn")}
            </Button>
          ))}
        </div>
        {dirty && <Notice>{t("settings.voice.changeNote")}</Notice>}
      </form>
    </div>
  );
}
