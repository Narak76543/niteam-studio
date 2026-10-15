import { useEffect, useState } from "react";

import { Button } from "./Button";
import { useToast } from "./toast";
import { useI18n } from "../i18n/context";
import { api, errorMessage } from "../lib/api";
import type { VoiceProfile } from "../lib/types";

interface Props {
  profile: VoiceProfile;
  onSaved: () => void;
  onCancel: () => void;
}

/** Rename a Voice Profile and tune its style and quality (docs/09-voice-profiles.md). */
export function EditVoiceDialog({ profile, onSaved, onCancel }: Props) {
  const { t } = useI18n();
  const toast = useToast();
  const [name, setName] = useState(profile.name);
  const [style, setStyle] = useState(profile.style_hint);
  const [cfg, setCfg] = useState(profile.cfg_value);
  const [steps, setSteps] = useState(profile.inference_timesteps);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onCancel();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel]);

  const save = async () => {
    setSaving(true);
    try {
      await api.voiceProfileUpdate(profile.id, name, style, cfg, steps);
      toast.show(t("common.saved"), "ok");
      onSaved();
    } catch (e) {
      toast.show(errorMessage(e), "danger");
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="backdrop" onMouseDown={(e) => e.target === e.currentTarget && onCancel()}>
      <form
        className="dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="edit-voice-title"
        onSubmit={(e) => {
          e.preventDefault();
          void save();
        }}
      >
        <h2 id="edit-voice-title" className="t-h2">
          {t("voices.editTitle")}
        </h2>
        <label className="field">
          <span className="field__label">{t("voices.name")}</span>
          <input className="input" maxLength={60} value={name} onChange={(e) => setName(e.target.value)} />
        </label>
        <label className="field">
          <span className="field__label">{t("voices.style")}</span>
          <input className="input" lang="en" maxLength={200} value={style} onChange={(e) => setStyle(e.target.value)} />
          <span className="t-small">{t("voices.styleHint")}</span>
        </label>
        <label className="field">
          <span className="field__label">{t("voices.cfg", { n: cfg.toFixed(1) })}</span>
          <input type="range" min={1} max={5} step={0.5} value={cfg} onChange={(e) => setCfg(Number(e.target.value))} />
          <span className="t-small">{t("voices.cfgHint")}</span>
        </label>
        <label className="field">
          <span className="field__label">{t("voices.steps", { n: steps })}</span>
          <input type="range" min={4} max={30} step={1} value={steps} onChange={(e) => setSteps(Number(e.target.value))} />
          <span className="t-small">{t("voices.stepsHint")}</span>
        </label>
        <span className="t-small">{t("voices.editNote")}</span>
        <div className="dialog__actions">
          <Button onClick={onCancel}>{t("common.cancel")}</Button>
          <Button type="submit" variant="primary" disabled={saving || !name.trim()}>
            {t("common.save")}
          </Button>
        </div>
      </form>
    </div>
  );
}
