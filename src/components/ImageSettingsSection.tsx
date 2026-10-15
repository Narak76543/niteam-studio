import { ExternalLink, KeyRound } from "lucide-react";
import { useEffect, useState } from "react";

import { Badge } from "./Badge";
import { Button } from "./Button";
import { ConfirmDialog } from "./ConfirmDialog";
import { ErrorPanel } from "./ErrorPanel";
import { Notice } from "./Notice";
import { useToast } from "./toast";
import { useI18n } from "../i18n/context";
import { api, errorMessage } from "../lib/api";
import type { ImageCatalog, ImageSettings, ProviderId, Settings } from "../lib/types";

const PROVIDERS: ProviderId[] = ["cloudflare_flux", "gemini_nano_banana", "local_sdcpp"];
const CLOUDFLARE_TOKEN_URL = "https://dash.cloudflare.com/profile/api-tokens";
const ICON = { size: 16, strokeWidth: 1.5 } as const;

type TestState =
  | { kind: "idle" }
  | { kind: "testing" }
  | { kind: "ok" }
  | { kind: "error"; errorKind: string | null; details: string | null };

interface Props {
  settings: Settings;
  onSave: (s: Settings) => Promise<Settings>;
}

function toProvider(v: string): ProviderId {
  return PROVIDERS.find((p) => p === v) ?? "cloudflare_flux";
}

/** Settings → Images (docs/07-image-generation.md §1): provider, Cloudflare credentials, steps, models. */
export function ImageSettingsSection({ settings, onSave }: Props) {
  const { t } = useI18n();
  const toast = useToast();
  const [image, setImage] = useState<ImageSettings>(settings.image);
  const [catalog, setCatalog] = useState<ImageCatalog | null>(null);
  const [hasToken, setHasToken] = useState<boolean | null>(null);
  const [tokenInput, setTokenInput] = useState("");
  const [test, setTest] = useState<TestState>({ kind: "idle" });
  const [confirmClear, setConfirmClear] = useState(false);
  /** A local model profile is installed and selected (Settings → Local images). */
  const [localReady, setLocalReady] = useState(false);

  useEffect(() => {
    api
      .localStatus()
      .then((s) => setLocalReady(s.profiles.some((p) => p.installed && p.id === s.selected)))
      .catch(() => setLocalReady(false));
    api
      .imagesCatalog()
      .then(setCatalog)
      .catch(() => setCatalog(null));
    api
      .cloudflareTokenExists()
      .then(setHasToken)
      .catch(() => setHasToken(false));
  }, []);

  const dirty = JSON.stringify(image) !== JSON.stringify(settings.image);
  const set = (patch: Partial<ImageSettings>) => setImage({ ...image, ...patch });
  const geminiModels = Array.from(new Set([...(catalog?.gemini_models ?? []), image.gemini_model]));
  const usesCloudflare = image.provider === "cloudflare_flux" || image.key_scene_provider === "cloudflare_flux";
  const usesGemini = image.provider === "gemini_nano_banana" || image.key_scene_provider === "gemini_nano_banana";

  const save = async (): Promise<boolean> => {
    try {
      const saved = await onSave({
        ...settings,
        image: { ...image, cloudflare_account_id: image.cloudflare_account_id.trim() },
      });
      setImage(saved.image);
      toast.show(t("common.saved"), "ok");
      return true;
    } catch (e) {
      toast.show(errorMessage(e), "danger");
      return false;
    }
  };

  const runTest = async () => {
    // The test uses the saved account ID.
    if (dirty && !(await save())) return;
    setTest({ kind: "testing" });
    try {
      const r = await api.cloudflareTest();
      setTest(r.ok ? { kind: "ok" } : { kind: "error", errorKind: r.error_kind, details: r.error });
    } catch (e) {
      setTest({ kind: "error", errorKind: "other", details: errorMessage(e) });
    }
  };

  const saveToken = async () => {
    try {
      await api.cloudflareTokenSet(tokenInput);
      setTokenInput("");
      setHasToken(true);
      toast.show(t("settings.images.tokenSavedToast"), "ok");
      if (image.cloudflare_account_id.trim()) await runTest();
    } catch (e) {
      toast.show(errorMessage(e), "danger");
    }
  };

  const clearToken = async () => {
    setConfirmClear(false);
    try {
      await api.cloudflareTokenClear();
      setHasToken(false);
      setTest({ kind: "idle" });
      toast.show(t("settings.images.tokenClearedToast"), "ok");
    } catch (e) {
      toast.show(errorMessage(e), "danger");
    }
  };

  return (
    <div className="settings-row">
      <div className="settings-row__intro">
        <h2 className="t-h3">{t("settings.images.title")}</h2>
        <p className="t-small">{t("settings.images.body")}</p>
        {catalog && (
          <button className="link t-small row" onClick={() => void api.openExternal(catalog.flux_license_url)}>
            {t("settings.images.license")} <ExternalLink size={12} strokeWidth={1.5} aria-hidden="true" />
          </button>
        )}
      </div>
      <div className="settings-row__control">
        <form
          className="settings-row__control"
          onSubmit={(e) => {
            e.preventDefault();
            void save();
          }}
        >
          <div className="models-grid">
            <label className="field">
              <span className="t-small">{t("settings.images.provider")}</span>
              <select className="select" value={image.provider} onChange={(e) => set({ provider: toProvider(e.target.value) })}>
                {PROVIDERS.map((p) => (
                  <option key={p} value={p} disabled={p === "local_sdcpp" && !localReady}>
                    {t(`provider.${p}`)}
                  </option>
                ))}
              </select>
            </label>
            <label className="field">
              <span className="t-small">{t("settings.images.keyScene")}</span>
              <select
                className="select"
                value={image.key_scene_provider ?? ""}
                onChange={(e) => set({ key_scene_provider: e.target.value ? toProvider(e.target.value) : null })}
              >
                <option value="">{t("settings.images.keySceneSame")}</option>
                {PROVIDERS.map((p) => (
                  <option key={p} value={p} disabled={p === "local_sdcpp" && !localReady}>
                    {t(`provider.${p}`)}
                  </option>
                ))}
              </select>
            </label>
          </div>
          <span className="t-small">{t("settings.images.keySceneHint")}</span>
          {!localReady && <span className="t-small t-muted">{t("settings.images.localHint")}</span>}

          {usesCloudflare && (
            <div className="models-grid">
              <label className="field">
                <span className="t-small">{t("settings.images.accountId")}</span>
                <input
                  className="input"
                  lang="en"
                  spellCheck={false}
                  autoComplete="off"
                  value={image.cloudflare_account_id}
                  onChange={(e) => set({ cloudflare_account_id: e.target.value })}
                />
              </label>
              <label className="field">
                <span className="t-small">
                  {t("settings.images.steps")} · {image.steps}
                </span>
                <input
                  type="range"
                  min={4}
                  max={8}
                  step={1}
                  value={image.steps}
                  onChange={(e) => set({ steps: Number(e.target.value) })}
                />
              </label>
              <label className="field">
                <span className="t-small">{t("settings.images.concurrency")}</span>
                <select
                  className="select"
                  value={image.cloudflare_concurrency}
                  onChange={(e) => set({ cloudflare_concurrency: Number(e.target.value) })}
                >
                  {[1, 2, 3, 4].map((n) => (
                    <option key={n} value={n}>
                      {n}
                    </option>
                  ))}
                </select>
              </label>
            </div>
          )}
          {usesCloudflare && <span className="t-small">{t("settings.images.stepsHint")}</span>}

          {usesGemini && (
            <>
              <label className="field">
                <span className="t-small">{t("settings.images.geminiModel")}</span>
                <select className="select" value={image.gemini_model} onChange={(e) => set({ gemini_model: e.target.value })}>
                  {geminiModels.map((m) => (
                    <option key={m} value={m}>
                      {m}
                    </option>
                  ))}
                </select>
              </label>
              <Notice tone="warn">{t("settings.images.geminiPaid")}</Notice>
            </>
          )}

          <div className="row">
            <Button type="submit" disabled={!dirty}>
              {t("common.save")}
            </Button>
          </div>
          {dirty && <Notice>{t("settings.images.changeNote")}</Notice>}
        </form>

        {usesCloudflare && (
          <>
            <div className="row">
              <KeyRound {...ICON} aria-hidden="true" />
              <span className="t-small">{t("settings.images.token")}</span>
              {hasToken === null ? null : hasToken ? (
                <Badge tone="ok">{t("settings.images.tokenStored")}</Badge>
              ) : (
                <Badge tone="warn">{t("settings.images.tokenMissing")}</Badge>
              )}
              {hasToken && (
                <>
                  <Button variant="pill" disabled={test.kind === "testing"} onClick={() => void runTest()}>
                    {test.kind === "testing" ? t("settings.key.testing") : t("settings.images.test")}
                  </Button>
                  <Button variant="pill" onClick={() => setConfirmClear(true)}>
                    {t("settings.images.tokenClear")}
                  </Button>
                </>
              )}
            </div>
            <form
              className="row"
              onSubmit={(e) => {
                e.preventDefault();
                if (tokenInput.trim()) void saveToken();
              }}
            >
              <input
                className="input input--grow"
                type="password"
                autoComplete="off"
                spellCheck={false}
                aria-label={t("settings.images.tokenPlaceholder")}
                placeholder={t("settings.images.tokenPlaceholder")}
                value={tokenInput}
                onChange={(e) => setTokenInput(e.target.value)}
              />
              <Button type="submit" disabled={!tokenInput.trim()}>
                {t("settings.key.save")}
              </Button>
            </form>
            <button className="link t-small row" onClick={() => void api.openExternal(CLOUDFLARE_TOKEN_URL)}>
              {t("settings.images.getToken")} <ExternalLink size={12} strokeWidth={1.5} aria-hidden="true" />
            </button>
            {test.kind === "ok" && <Notice tone="ok">{t("settings.images.testOk")}</Notice>}
            {test.kind === "error" && (
              <ErrorPanel kind={test.errorKind} details={test.details} onRetry={() => void runTest()} />
            )}
          </>
        )}
      </div>

      {confirmClear && (
        <ConfirmDialog
          title={t("settings.images.tokenClearTitle")}
          body={t("settings.images.tokenClearBody")}
          confirmLabel={t("settings.images.tokenClear")}
          danger
          onConfirm={() => void clearToken()}
          onCancel={() => setConfirmClear(false)}
        />
      )}
    </div>
  );
}
