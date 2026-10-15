import { useEffect, useState } from "react";

import { useToast } from "./toast";
import { useI18n } from "../i18n/context";
import { api, errorMessage } from "../lib/api";
import type { Project, VoicesStatus } from "../lib/types";

interface Props {
  project: Project;
  disabled: boolean;
  onChange: (p: Project) => void;
}

/** Scenes page: which local Voice Profile narrates this project. Shown only when the voice
 * engine is VoxCPM2 and at least one profile exists. */
export function ProjectVoicePicker({ project, disabled, onChange }: Props) {
  const { t } = useI18n();
  const toast = useToast();
  const [status, setStatus] = useState<VoicesStatus | null>(null);
  const [local, setLocal] = useState(false);

  useEffect(() => {
    api
      .settingsGet()
      .then((s) => setLocal(s.voice.engine === "voxcpm"))
      .catch(() => setLocal(false));
    api
      .voicesStatus()
      .then(setStatus)
      .catch(() => setStatus(null));
  }, []);

  if (!local || !status || status.profiles.length === 0) return null;

  const fallback = status.profiles.find((p) => p.id === status.default_profile);
  // A deleted voice falls back to the default (as in the voice job).
  const value = status.profiles.some((p) => p.id === project.voice_profile) ? (project.voice_profile ?? "") : "";

  const change = async (id: string) => {
    try {
      onChange(await api.projectSetVoiceProfile(project.id, id === "" ? null : id));
      setStatus(await api.voicesStatus());
    } catch (e) {
      toast.show(errorMessage(e), "danger");
    }
  };

  return (
    <label className="row">
      <span className="t-small">{t("voices.projectVoice")}</span>
      <select className="select select--compact" value={value} disabled={disabled} onChange={(e) => void change(e.target.value)}>
        <option value="">{t("voices.useDefault", { name: fallback?.name ?? "—" })}</option>
        {status.profiles.map((p) => (
          <option key={p.id} value={p.id}>
            {p.name}
          </option>
        ))}
      </select>
    </label>
  );
}
