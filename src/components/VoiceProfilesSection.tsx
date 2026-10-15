import { Copy, Cpu, Download, ExternalLink, PencilLine, Play, Plus, ShieldCheck, Square, Star, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { Badge } from "./Badge";
import { Button } from "./Button";
import { ConfirmDialog } from "./ConfirmDialog";
import { EditVoiceDialog } from "./EditVoiceDialog";
import { NewVoiceDialog } from "./NewVoiceDialog";
import { Notice } from "./Notice";
import { ProgressBar } from "./ProgressBar";
import { useToast } from "./toast";
import { useI18n } from "../i18n/context";
import { api, errorMessage, onDownloadProgress } from "../lib/api";
import { playAudio } from "../lib/audioPlayer";
import type { DownloadProgress, VoiceProfile, VoicesStatus } from "../lib/types";

const ICON = { size: 14, strokeWidth: 1.5 } as const;

interface Props {
  /** Re-reads settings.json after Rust changed it (default voice). */
  onReload: () => Promise<void>;
}

function gb(mb: number): string {
  return (mb / 1024).toFixed(1);
}

/** Settings → Local voices (docs/09-voice-profiles.md): VoxCPM2 model + Voice Profiles. */
export function VoiceProfilesSection({ onReload }: Props) {
  const { t } = useI18n();
  const toast = useToast();
  const [status, setStatus] = useState<VoicesStatus | null>(null);
  const [progress, setProgress] = useState<DownloadProgress | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [editing, setEditing] = useState<VoiceProfile | null>(null);
  const [confirm, setConfirm] = useState<{ kind: "model" } | { kind: "profile"; profile: VoiceProfile } | null>(null);

  const refresh = useCallback(async () => {
    try {
      setStatus(await api.voicesStatus());
    } catch (e) {
      toast.show(errorMessage(e), "danger");
    }
  }, [toast]);

  useEffect(() => {
    api
      .voicesStatus()
      .then(setStatus)
      .catch(() => setStatus(null));
  }, []);

  const modelId = status?.model?.id;
  useEffect(() => {
    const unlisten = onDownloadProgress((p) => {
      if (p.profile_id !== modelId) return;
      setProgress(p);
      if (p.stage !== "running") {
        void refresh();
        if (p.stage === "done") toast.show(t("voices.installed"), "ok");
        if (p.stage === "failed" && p.error) toast.show(p.error, "danger");
      }
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [modelId, refresh, toast, t]);

  if (!status) return null;

  const run = async (key: string, fn: () => Promise<unknown>) => {
    setBusy(key);
    try {
      await fn();
    } catch (e) {
      toast.show(errorMessage(e), "danger");
    } finally {
      setBusy(null);
      await refresh();
    }
  };

  const model = status.model;
  const downloading = model !== null && status.installing !== null && progress?.profile_id === model.id && progress.stage === "running";
  const pct = downloading && progress && progress.total_mb > 0 ? progress.done_mb / progress.total_mb : undefined;
  const ready = status.capable && status.engine_present && model?.installed === true;

  const install = () =>
    run("install", async () => {
      if (!model) return;
      setProgress({ profile_id: model.id, stage: "running", done_mb: model.downloaded_mb, total_mb: model.total_mb, error: null });
      await api.voicesModelInstall();
    });

  const listen = (p: VoiceProfile) =>
    run(`preview-${p.id}`, async () => {
      const bytes = await api.voiceProfilePreview(p.id);
      await playAudio(`voice-${p.id}`, bytes, "audio/wav");
    });

  const setDefault = (p: VoiceProfile) =>
    run(`default-${p.id}`, async () => {
      await api.voiceProfileSetDefault(p.id);
      await onReload();
    });

  const onConfirm = () => {
    const c = confirm;
    setConfirm(null);
    if (!c) return;
    if (c.kind === "model") void run("delete-model", () => api.voicesModelDelete());
    else
      void run(`delete-${c.profile.id}`, async () => {
        await api.voiceProfileDelete(c.profile.id);
        await onReload();
      });
  };

  return (
    <div className="settings-row">
      <div className="settings-row__intro">
        <h2 className="t-h3">{t("voices.title")}</h2>
        <p className="t-small">{t("voices.body")}</p>
      </div>
      <div className="settings-row__control">
        <div className="row">
          <Cpu size={16} strokeWidth={1.5} aria-hidden="true" />
          {status.gpu ? (
            <>
              <span className="t-small">{status.gpu.name}</span>
              <Badge tone={status.capable ? "ok" : "neutral"}>{t("local.vram", { gb: gb(status.gpu.vram_mb) })}</Badge>
            </>
          ) : (
            <span className="t-small">{t("local.noGpu")}</span>
          )}
        </div>
        {!status.capable && <Notice>{t("voices.notCapable", { gb: gb(model?.min_vram_mb ?? 5500) })}</Notice>}
        {status.capable && !status.engine_present && <Notice tone="warn">{t("voices.engineMissing")}</Notice>}

        {status.capable && model && (
          <div className="local-profile">
            <div className="row local-profile__head">
              <strong className="t-small">{model.label}</strong>
              <span className="t-small t-muted">{t("local.size", { gb: gb(model.total_mb), vram: gb(model.min_vram_mb) })}</span>
              {model.installed && <Badge tone="ok">{t("local.installedBadge")}</Badge>}
            </div>
            <ul className="local-profile__files">
              {model.files.map((f) => (
                <li key={f.name} className="t-micro t-muted">
                  {f.name} ·{" "}
                  <button className="link t-micro" onClick={() => void api.openExternal(f.license_url)}>
                    {f.license} <ExternalLink size={10} strokeWidth={1.5} aria-hidden="true" />
                  </button>
                </li>
              ))}
            </ul>
            {downloading && (
              <div className="field">
                <ProgressBar label={t("local.downloading")} value={pct} />
                <span className="t-micro t-muted">
                  {t("local.progress", { done: gb(progress?.done_mb ?? 0), total: gb(progress?.total_mb ?? model.total_mb) })}
                </span>
              </div>
            )}
            <div className="row">
              {downloading ? (
                <Button variant="pill" icon={<Square {...ICON} />} onClick={() => void api.localInstallCancel()}>
                  {t("local.stop")}
                </Button>
              ) : !model.installed ? (
                <Button
                  variant="pill"
                  icon={<Download {...ICON} />}
                  disabled={status.installing !== null || !status.engine_present}
                  onClick={() => void install()}
                >
                  {model.downloaded_mb > 0 ? t("local.resume") : t("local.install")}
                </Button>
              ) : (
                <Button
                  variant="pill"
                  icon={<ShieldCheck {...ICON} />}
                  disabled={busy !== null}
                  onClick={() =>
                    void run("verify", async () => {
                      const ok = await api.voicesModelVerify();
                      toast.show(ok ? t("local.verifyOk") : t("local.verifyBad"), ok ? "ok" : "danger");
                    })
                  }
                >
                  {busy === "verify" ? t("local.verifying") : t("local.verify")}
                </Button>
              )}
              {(model.installed || model.downloaded_mb > 0) && !downloading && (
                <Button variant="pill" icon={<Trash2 {...ICON} />} disabled={busy !== null} onClick={() => setConfirm({ kind: "model" })}>
                  {t("common.delete")}
                </Button>
              )}
            </div>
          </div>
        )}

        {ready && (
          <>
            {status.profiles.length === 0 ? (
              <p className="t-small t-muted">{t("voices.empty")}</p>
            ) : (
              <ul className="voice-list">
                {status.profiles.map((p) => {
                  const isDefault = status.default_profile === p.id;
                  return (
                    <li key={p.id} className="voice-list__item">
                      <div className="row">
                        <strong className="t-small">{p.name}</strong>
                        <Badge>{t(`voices.mode.${p.mode}`)}</Badge>
                        <span className="t-micro t-muted">{p.language === "km" ? t("lang.km") : t("lang.en")}</span>
                        {isDefault && <Badge tone="ok">{t("voices.default")}</Badge>}
                      </div>
                      {p.style_hint && <span className="t-micro t-muted">{p.style_hint}</span>}
                      <div className="row">
                        <Button variant="pill" icon={<Play {...ICON} />} disabled={busy !== null} onClick={() => void listen(p)}>
                          {busy === `preview-${p.id}` ? t("voices.speaking") : t("voices.listen")}
                        </Button>
                        {!isDefault && (
                          <Button variant="pill" icon={<Star {...ICON} />} disabled={busy !== null} onClick={() => void setDefault(p)}>
                            {t("voices.makeDefault")}
                          </Button>
                        )}
                        <Button variant="pill" icon={<PencilLine {...ICON} />} disabled={busy !== null} onClick={() => setEditing(p)}>
                          {t("voices.edit")}
                        </Button>
                        <Button
                          variant="pill"
                          icon={<Copy {...ICON} />}
                          disabled={busy !== null}
                          onClick={() => void run(`dup-${p.id}`, () => api.voiceProfileDuplicate(p.id))}
                        >
                          {t("voices.duplicate")}
                        </Button>
                        <Button
                          variant="pill"
                          icon={<Trash2 {...ICON} />}
                          disabled={busy !== null}
                          onClick={() => setConfirm({ kind: "profile", profile: p })}
                        >
                          {t("common.delete")}
                        </Button>
                      </div>
                    </li>
                  );
                })}
              </ul>
            )}
            <div className="row">
              <Button icon={<Plus {...ICON} />} disabled={busy !== null} onClick={() => setCreating(true)}>
                {t("voices.new")}
              </Button>
            </div>
          </>
        )}
        <span className="t-micro t-muted">{t("voices.engine", { version: status.engine_version || "—" })}</span>
      </div>

      {creating && (
        <NewVoiceDialog
          onCreated={() => {
            setCreating(false);
            void refresh();
            void onReload();
          }}
          onCancel={() => setCreating(false)}
        />
      )}
      {editing && (
        <EditVoiceDialog
          profile={editing}
          onSaved={() => {
            setEditing(null);
            void refresh();
          }}
          onCancel={() => setEditing(null)}
        />
      )}
      {confirm && (
        <ConfirmDialog
          title={confirm.kind === "model" ? t("voices.deleteModelTitle") : t("voices.deleteTitle", { name: confirm.profile.name })}
          body={confirm.kind === "model" ? t("voices.deleteModelBody") : t("voices.deleteBody")}
          confirmLabel={t("common.delete")}
          danger
          onConfirm={onConfirm}
          onCancel={() => setConfirm(null)}
        />
      )}
    </div>
  );
}
