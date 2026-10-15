import { Cpu, Download, ExternalLink, Gauge, ShieldCheck, Square, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { Badge } from "./Badge";
import { Button } from "./Button";
import { ConfirmDialog } from "./ConfirmDialog";
import { Notice } from "./Notice";
import { ProgressBar } from "./ProgressBar";
import { useToast } from "./toast";
import { useI18n } from "../i18n/context";
import { api, errorMessage, onDownloadProgress } from "../lib/api";
import type { DownloadProgress, LocalProfileStatus, LocalStatus, Settings } from "../lib/types";

const ICON = { size: 14, strokeWidth: 1.5 } as const;

interface Props {
  settings: Settings;
  onSave: (s: Settings) => Promise<Settings>;
  /** Re-reads settings.json after Rust changed it (install selects the local provider). */
  onReload: () => Promise<void>;
}

function gb(mb: number): string {
  return (mb / 1024).toFixed(mb >= 10 * 1024 ? 0 : 1);
}

/** Settings → Local images (docs/08-local-gpu.md): GPU, model profiles, cool-down, benchmark. */
export function LocalImagesSection({ settings, onSave, onReload }: Props) {
  const { t } = useI18n();
  const toast = useToast();
  const [status, setStatus] = useState<LocalStatus | null>(null);
  const [progress, setProgress] = useState<DownloadProgress | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<LocalProfileStatus | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [cooldown, setCooldown] = useState({ every: settings.local.cooldown_every, seconds: settings.local.cooldown_seconds });

  const refresh = useCallback(async () => {
    try {
      setStatus(await api.localStatus());
    } catch (e) {
      toast.show(errorMessage(e), "danger");
    }
  }, [toast]);

  useEffect(() => {
    api
      .localStatus()
      .then(setStatus)
      .catch(() => setStatus(null));
  }, []);

  useEffect(() => {
    const unlisten = onDownloadProgress((p) => {
      setProgress(p);
      if (p.stage !== "running") {
        void refresh();
        if (p.stage === "done") {
          toast.show(t("local.installed"), "ok");
          void onReload();
        }
        if (p.stage === "failed" && p.error) toast.show(p.error, "danger");
      }
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [refresh, onReload, toast, t]);

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

  const install = (p: LocalProfileStatus) =>
    run(`install-${p.id}`, async () => {
      setProgress({ profile_id: p.id, stage: "running", done_mb: p.downloaded_mb, total_mb: p.total_mb, error: null });
      await api.localInstall(p.id);
    });

  const verify = (p: LocalProfileStatus) =>
    run(`verify-${p.id}`, async () => {
      const ok = await api.localVerify(p.id);
      toast.show(ok ? t("local.verifyOk") : t("local.verifyBad"), ok ? "ok" : "danger");
    });

  const remove = (p: LocalProfileStatus) =>
    run(`delete-${p.id}`, async () => {
      setConfirmDelete(null);
      await api.localDelete(p.id);
      await onReload();
    });

  const benchmark = () =>
    run("benchmark", async () => {
      const r = await api.localBenchmark();
      toast.show(t("local.benchmarkResult", { s: r.seconds_per_image.toFixed(1) }), "ok");
      await onReload();
    });

  const saveCooldown = async () => {
    try {
      await onSave({ ...settings, local: { ...settings.local, cooldown_every: cooldown.every, cooldown_seconds: cooldown.seconds } });
      toast.show(t("common.saved"), "ok");
    } catch (e) {
      toast.show(errorMessage(e), "danger");
    }
  };

  const gpu = status.gpu;
  const anyInstalled = status.profiles.some((p) => p.installed);
  const cooldownDirty = cooldown.every !== settings.local.cooldown_every || cooldown.seconds !== settings.local.cooldown_seconds;

  return (
    <div className="settings-row">
      <div className="settings-row__intro">
        <h2 className="t-h3">{t("local.title")}</h2>
        <p className="t-small">{t("local.body")}</p>
      </div>
      <div className="settings-row__control">
        <div className="row">
          <Cpu size={16} strokeWidth={1.5} aria-hidden="true" />
          {gpu ? (
            <>
              <span className="t-small">{gpu.name}</span>
              <Badge tone={status.capable ? "ok" : "neutral"}>{t("local.vram", { gb: gb(gpu.vram_mb) })}</Badge>
            </>
          ) : (
            <span className="t-small">{t("local.noGpu")}</span>
          )}
        </div>
        {!status.capable && <Notice>{t("local.notCapable")}</Notice>}
        {status.capable && !status.engine_present && <Notice tone="warn">{t("local.engineMissing")}</Notice>}

        {status.capable && status.profiles.length === 0 && <Notice>{t("local.noProfiles")}</Notice>}

        {status.capable &&
          status.profiles.map((p) => {
            const downloading = status.installing !== null && progress?.profile_id === p.id && progress.stage === "running";
            const pct = downloading && progress && progress.total_mb > 0 ? progress.done_mb / progress.total_mb : undefined;
            return (
              <div key={p.id} className="local-profile">
                <div className="row local-profile__head">
                  <strong className="t-small">{p.label}</strong>
                  <span className="t-small t-muted">
                    {t("local.size", { gb: gb(p.total_mb), vram: gb(p.min_vram_mb) })}
                  </span>
                  {p.installed ? (
                    <Badge tone="ok">{status.selected === p.id ? t("local.inUse") : t("local.installedBadge")}</Badge>
                  ) : !p.fits ? (
                    <Badge tone="warn">{t("local.tooBig")}</Badge>
                  ) : null}
                </div>
                <ul className="local-profile__files">
                  {p.files.map((f) => (
                    <li key={f.name} className="t-micro t-muted">
                      {f.name} · {gb(f.size_mb)} GB ·{" "}
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
                      {t("local.progress", { done: gb(progress?.done_mb ?? 0), total: gb(progress?.total_mb ?? p.total_mb) })}
                    </span>
                  </div>
                )}
                <div className="row">
                  {downloading ? (
                    <Button variant="pill" icon={<Square {...ICON} />} onClick={() => void api.localInstallCancel()}>
                      {t("local.stop")}
                    </Button>
                  ) : !p.installed ? (
                    <Button
                      variant="pill"
                      icon={<Download {...ICON} />}
                      disabled={!p.fits || status.installing !== null || !status.engine_present}
                      onClick={() => void install(p)}
                    >
                      {p.downloaded_mb > 0 ? t("local.resume") : t("local.install")}
                    </Button>
                  ) : (
                    <Button variant="pill" icon={<ShieldCheck {...ICON} />} disabled={busy !== null} onClick={() => void verify(p)}>
                      {busy === `verify-${p.id}` ? t("local.verifying") : t("local.verify")}
                    </Button>
                  )}
                  {(p.installed || p.downloaded_mb > 0) && !downloading && (
                    <Button variant="pill" icon={<Trash2 {...ICON} />} disabled={busy !== null} onClick={() => setConfirmDelete(p)}>
                      {t("common.delete")}
                    </Button>
                  )}
                </div>
              </div>
            );
          })}

        {anyInstalled && (
          <>
            <div className="row">
              <Button variant="pill" icon={<Gauge {...ICON} />} disabled={busy !== null} onClick={() => void benchmark()}>
                {busy === "benchmark" ? t("local.benchmarking") : t("local.benchmark")}
              </Button>
              {settings.local.seconds_per_image !== null && (
                <span className="t-small t-muted">{t("local.speed", { s: settings.local.seconds_per_image.toFixed(1) })}</span>
              )}
            </div>
            <div className="models-grid">
              <label className="field">
                <span className="t-small">{t("local.cooldownEvery")}</span>
                <input
                  className="input"
                  type="number"
                  min={0}
                  max={100}
                  value={cooldown.every}
                  onChange={(e) => setCooldown({ ...cooldown, every: Math.max(0, Math.min(100, Number(e.target.value) || 0)) })}
                />
              </label>
              <label className="field">
                <span className="t-small">{t("local.cooldownSeconds")}</span>
                <input
                  className="input"
                  type="number"
                  min={0}
                  max={300}
                  value={cooldown.seconds}
                  onChange={(e) => setCooldown({ ...cooldown, seconds: Math.max(0, Math.min(300, Number(e.target.value) || 0)) })}
                />
              </label>
            </div>
            <span className="t-small t-muted">
              {status.power.has_battery ? t("local.cooldownLaptop") : t("local.cooldownHint")}
            </span>
            <div className="row">
              <Button variant="pill" disabled={!cooldownDirty} onClick={() => void saveCooldown()}>
                {t("common.save")}
              </Button>
              {status.power.has_battery && cooldown.every === 0 && (
                <Button variant="pill" onClick={() => setCooldown({ every: 10, seconds: 20 })}>
                  {t("local.cooldownSuggest")}
                </Button>
              )}
            </div>
          </>
        )}
        <span className="t-micro t-muted">{t("local.engine", { version: status.engine_version || "—" })}</span>
      </div>

      {confirmDelete && (
        <ConfirmDialog
          title={t("local.deleteTitle", { name: confirmDelete.label })}
          body={t("local.deleteBody", { gb: gb(confirmDelete.total_mb) })}
          confirmLabel={t("common.delete")}
          danger
          onConfirm={() => void remove(confirmDelete)}
          onCancel={() => setConfirmDelete(null)}
        />
      )}
    </div>
  );
}
