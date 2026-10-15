import { useState } from "react";

import { Button } from "./Button";
import { useToast } from "./toast";
import { useI18n } from "../i18n/context";
import { api, errorMessage } from "../lib/api";
import type { Settings } from "../lib/types";

/** Must match `render::encoders::ORDER` in Rust. */
const ENCODERS = ["h264_qsv", "h264_amf", "h264_nvenc", "libx264"] as const;

interface Props {
  settings: Settings;
  onSave: (s: Settings) => Promise<Settings>;
}

/** Settings → Video: the H.264 encoder (detected once and cached, or chosen by hand). */
export function VideoSettingsSection({ settings, onSave }: Props) {
  const { t } = useI18n();
  const toast = useToast();
  const [detecting, setDetecting] = useState(false);
  const encoder = settings.render.encoder;

  const save = async (next: string | null) => {
    try {
      await onSave({ ...settings, render: { ...settings.render, encoder: next } });
      toast.show(t("common.saved"), "ok");
    } catch (e) {
      toast.show(errorMessage(e), "danger");
    }
  };

  const detect = async () => {
    setDetecting(true);
    try {
      const found = await api.encodersDetect();
      // Keep the app's copy of the settings in sync with what Rust cached.
      await onSave({ ...settings, render: { ...settings.render, encoder: found } });
      toast.show(t("settings.video.detected", { encoder: found }), "ok");
    } catch (e) {
      toast.show(errorMessage(e), "danger");
    } finally {
      setDetecting(false);
    }
  };

  return (
    <div className="settings-row">
      <div className="settings-row__intro">
        <h2 className="t-h3">{t("settings.video.title")}</h2>
        <p className="t-small">{t("settings.video.body")}</p>
      </div>
      <div className="settings-row__control">
        <label className="field">
          <span className="t-small">{t("settings.video.encoder")}</span>
          <select
            className="select"
            lang="en"
            value={encoder ?? ""}
            onChange={(e) => void save(e.target.value || null)}
          >
            <option value="">{t("settings.video.auto")}</option>
            {ENCODERS.map((e) => (
              <option key={e} value={e}>
                {t(`encoder.${e}`)}
              </option>
            ))}
          </select>
        </label>
        <div className="row">
          <Button variant="pill" disabled={detecting} onClick={() => void detect()}>
            {detecting ? t("settings.video.detecting") : t("settings.video.detect")}
          </Button>
        </div>
      </div>
    </div>
  );
}
