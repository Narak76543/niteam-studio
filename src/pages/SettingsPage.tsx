import { ArrowDown, ArrowUp, ExternalLink, FolderOpen, KeyRound, Plus, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { Badge } from "../components/Badge";
import { Button, IconButton } from "../components/Button";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { ErrorPanel } from "../components/ErrorPanel";
import { ImageSettingsSection } from "../components/ImageSettingsSection";
import { LocalImagesSection } from "../components/LocalImagesSection";
import { Notice } from "../components/Notice";
import { PageHeader } from "../components/PageHeader";
import { RetryStatus } from "../components/RetryStatus";
import { SectionJumpBar } from "../components/SectionJumpBar";
import { useToast } from "../components/toast";
import { VideoSettingsSection } from "../components/VideoSettingsSection";
import { VoiceProfilesSection } from "../components/VoiceProfilesSection";
import { VoiceSettingsSection } from "../components/VoiceSettingsSection";
import { useI18n } from "../i18n/context";
import { api, errorMessage, onGeminiRetry } from "../lib/api";
import type { GeminiRetry, Language, ModelSettings, Settings } from "../lib/types";

const AI_STUDIO_KEY_URL = "https://aistudio.google.com/apikey";
const SINGLE_MODEL_FIELDS = ["tts", "tts_lite"] as const;
const MAX_TEXT_CHAIN = 6;
const ICON = { size: 16, strokeWidth: 1.5 } as const;

type TestState =
  | { kind: "idle" }
  | { kind: "testing" }
  | { kind: "ok"; model: string }
  | { kind: "error"; errorKind: string | null; details: string | null };

interface Props {
  settings: Settings;
  onSave: (s: Settings) => Promise<Settings>;
  onKeyChanged: () => Promise<void>;
  /** Re-read settings.json (Rust changed it, e.g. after installing a local model). */
  onReload: () => Promise<void>;
}

export function SettingsPage({ settings, onSave, onKeyChanged, onReload }: Props) {
  const sectionsRef = useRef<HTMLDivElement>(null);
  const { t } = useI18n();
  const toast = useToast();
  const [hasKey, setHasKey] = useState<boolean | null>(null);
  const [keyInput, setKeyInput] = useState("");
  const [test, setTest] = useState<TestState>({ kind: "idle" });
  const [confirmClear, setConfirmClear] = useState(false);
  const [models, setModels] = useState<ModelSettings>(settings.models);
  const [retry, setRetry] = useState<{ notice: GeminiRetry; at: number } | null>(null);

  useEffect(() => {
    const unlisten = onGeminiRetry((r) => {
      if (r.scope === "key_test") setRetry({ notice: r, at: Date.now() });
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    api
      .apiKeyExists()
      .then(setHasKey)
      .catch(() => setHasKey(false));
  }, []);

  const persist = async (next: Settings) => {
    try {
      await onSave(next);
      toast.show(t("common.saved"), "ok");
    } catch (e) {
      toast.show(errorMessage(e), "danger");
    }
  };

  const runTest = async () => {
    setTest({ kind: "testing" });
    setRetry(null);
    try {
      const r = await api.apiKeyTest();
      setTest(
        r.ok
          ? { kind: "ok", model: r.display_name ?? r.model ?? "" }
          : { kind: "error", errorKind: r.error_kind, details: r.error },
      );
    } catch (e) {
      setTest({ kind: "error", errorKind: "other", details: errorMessage(e) });
    } finally {
      setRetry(null);
    }
  };

  const saveKey = async () => {
    try {
      await api.apiKeySet(keyInput);
      setKeyInput("");
      setHasKey(true);
      toast.show(t("settings.key.savedToast"), "ok");
      await onKeyChanged();
      await runTest();
    } catch (e) {
      toast.show(errorMessage(e), "danger");
    }
  };

  const clearKey = async () => {
    setConfirmClear(false);
    try {
      await api.apiKeyClear();
      setHasKey(false);
      setTest({ kind: "idle" });
      toast.show(t("settings.key.clearedToast"), "ok");
      await onKeyChanged();
    } catch (e) {
      toast.show(errorMessage(e), "danger");
    }
  };

  const chooseFolder = async () => {
    try {
      const dir = await api.pickFolder();
      if (dir) await persist({ ...settings, output_dir: dir });
    } catch (e) {
      toast.show(errorMessage(e), "danger");
    }
  };

  const langOptions: { value: Language | null; label: string }[] = [
    { value: null, label: t("lang.auto") },
    { value: "km", label: "ខ្មែរ" },
    { value: "en", label: "English" },
  ];

  const chain = models.text_chain;
  const modelsDirty =
    SINGLE_MODEL_FIELDS.some((f) => models[f] !== settings.models[f]) ||
    JSON.stringify(chain) !== JSON.stringify(settings.models.text_chain) ||
    JSON.stringify(models.document) !== JSON.stringify(settings.models.document);
  const setChain = (next: string[]) => setModels({ ...models, text_chain: next });
  const moveModel = (i: number, delta: number) => {
    const next = [...chain];
    const [m] = next.splice(i, 1);
    if (m === undefined) return;
    next.splice(i + delta, 0, m);
    setChain(next);
  };

  return (
    <div className="page">
      <PageHeader breadcrumb={[{ label: t("projects.breadcrumb") }]} title={t("settings.title")} />

      <div className="settings-layout">
        <SectionJumpBar container={sectionsRef} label={t("settings.sections")} />
        <div className="settings" ref={sectionsRef}>
          {/* API key */}
          <div className="settings-row">
            <div className="settings-row__intro">
              <h2 className="t-h3">{t("settings.key.title")}</h2>
              <p className="t-small">{t("settings.key.body")}</p>
              <button className="link t-small row" onClick={() => void api.openExternal(AI_STUDIO_KEY_URL)}>
                {t("settings.key.getKey")} <ExternalLink size={12} strokeWidth={1.5} aria-hidden="true" />
              </button>
            </div>
            <div className="settings-row__control">
              <div className="row">
                <KeyRound {...ICON} aria-hidden="true" />
                {hasKey === null ? null : hasKey ? (
                  <Badge tone="ok">{t("settings.key.stored")}</Badge>
                ) : (
                  <Badge tone="warn">{t("settings.key.missing")}</Badge>
                )}
                {hasKey && (
                  <>
                    <Button variant="pill" disabled={test.kind === "testing"} onClick={() => void runTest()}>
                      {test.kind === "testing" ? t("settings.key.testing") : t("settings.key.test")}
                    </Button>
                    <Button variant="pill" onClick={() => setConfirmClear(true)}>
                      {t("settings.key.clear")}
                    </Button>
                  </>
                )}
              </div>
              <form
                className="row"
                onSubmit={(e) => {
                  e.preventDefault();
                  if (keyInput.trim()) void saveKey();
                }}
              >
                <input
                  className="input input--grow"
                  type="password"
                  autoComplete="off"
                  spellCheck={false}
                  aria-label={t("settings.key.placeholder")}
                  placeholder={t("settings.key.placeholder")}
                  value={keyInput}
                  onChange={(e) => setKeyInput(e.target.value)}
                />
                <Button type="submit" disabled={!keyInput.trim()}>
                  {t("settings.key.save")}
                </Button>
              </form>
              {test.kind === "testing" && retry && (
                <RetryStatus key={retry.at} retry={retry.notice} receivedAt={retry.at} />
              )}
              {test.kind === "ok" && <Notice tone="ok">{t("settings.key.ok", { model: test.model })}</Notice>}
              {test.kind === "error" && (
                <ErrorPanel kind={test.errorKind} details={test.details} onRetry={() => void runTest()} />
              )}
            </div>
          </div>

          <VoiceSettingsSection settings={settings} onSave={onSave} />

          <VoiceProfilesSection onReload={onReload} />

          {/* Remount when Rust changed the image settings (local install selects the provider). */}
          <ImageSettingsSection key={JSON.stringify(settings.image)} settings={settings} onSave={onSave} />

          <LocalImagesSection settings={settings} onSave={onSave} onReload={onReload} />

          <VideoSettingsSection settings={settings} onSave={onSave} />

          {/* UI language */}
          <div className="settings-row">
            <div className="settings-row__intro">
              <h2 className="t-h3">{t("settings.language.title")}</h2>
              <p className="t-small">{t("settings.language.body")}</p>
            </div>
            <div className="settings-row__control">
              <div className="segmented" role="group" aria-label={t("settings.language.title")}>
                {langOptions.map((o) => (
                  <button
                    key={o.value ?? "auto"}
                    type="button"
                    lang={o.value ?? undefined}
                    aria-pressed={settings.ui_language === o.value}
                    onClick={() => void persist({ ...settings, ui_language: o.value })}
                  >
                    {o.label}
                  </button>
                ))}
              </div>
            </div>
          </div>

          {/* Output folder */}
          <div className="settings-row">
            <div className="settings-row__intro">
              <h2 className="t-h3">{t("settings.output.title")}</h2>
              <p className="t-small">{t("settings.output.body")}</p>
            </div>
            <div className="settings-row__control">
              <span className="path">{settings.output_dir ?? t("settings.output.default")}</span>
              <div className="row">
                <Button variant="pill" icon={<FolderOpen size={12} strokeWidth={1.5} />} onClick={() => void chooseFolder()}>
                  {t("settings.output.choose")}
                </Button>
                {settings.output_dir && (
                  <Button variant="pill" onClick={() => void persist({ ...settings, output_dir: null })}>
                    {t("settings.output.reset")}
                  </Button>
                )}
              </div>
            </div>
          </div>

          {/* Models */}
          <div className="settings-row">
            <div className="settings-row__intro">
              <h2 className="t-h3">{t("settings.models.title")}</h2>
              <p className="t-small">{t("settings.models.body")}</p>
            </div>
            <form
              className="settings-row__control"
              onSubmit={(e) => {
                e.preventDefault();
                const text_chain = chain.map((m) => m.trim()).filter(Boolean);
                const document = models.document.map((m) => m.trim()).filter(Boolean);
                void persist({ ...settings, models: { ...models, text_chain, document } });
              }}
            >
              <div className="field">
                <span className="t-small">{t("settings.models.textChain")}</span>
                {chain.map((m, i) => (
                  <div key={i} className="row chain-row">
                    <span className="chain-row__num t-small">{i + 1}</span>
                    <input
                      className="input input--grow"
                      lang="en"
                      spellCheck={false}
                      aria-label={`${t("settings.models.textChain")} ${i + 1}`}
                      value={m}
                      onChange={(e) => setChain(chain.map((x, j) => (j === i ? e.target.value.trim() : x)))}
                    />
                    <IconButton
                      label={t("settings.models.moveUp")}
                      icon={<ArrowUp {...ICON} />}
                      disabled={i === 0}
                      onClick={() => moveModel(i, -1)}
                    />
                    <IconButton
                      label={t("settings.models.moveDown")}
                      icon={<ArrowDown {...ICON} />}
                      disabled={i === chain.length - 1}
                      onClick={() => moveModel(i, 1)}
                    />
                    <IconButton
                      label={t("settings.models.removeModel")}
                      icon={<X {...ICON} />}
                      disabled={chain.length <= 1}
                      onClick={() => setChain(chain.filter((_, j) => j !== i))}
                    />
                  </div>
                ))}
                <div className="row">
                  <Button
                    variant="pill"
                    icon={<Plus size={12} strokeWidth={1.5} />}
                    disabled={chain.length >= MAX_TEXT_CHAIN}
                    onClick={() => setChain([...chain, ""])}
                  >
                    {t("settings.models.addModel")}
                  </Button>
                </div>
                <p className="t-small">{t("settings.models.textChainHint")}</p>
              </div>
              <div className="models-grid">
                {SINGLE_MODEL_FIELDS.map((f) => (
                  <label key={f} className="field">
                    <span className="t-small">{t(`settings.models.${f}`)}</span>
                    <input
                      className="input"
                      lang="en"
                      spellCheck={false}
                      value={models[f]}
                      onChange={(e) => setModels({ ...models, [f]: e.target.value.trim() })}
                    />
                  </label>
                ))}
                <label className="field">
                  <span className="t-small">{t("settings.models.document")}</span>
                  <input
                    className="input"
                    lang="en"
                    spellCheck={false}
                    value={models.document.join(", ")}
                    onChange={(e) => setModels({ ...models, document: e.target.value.split(",").map((m) => m.trim()) })}
                  />
                  <span className="t-micro t-muted">{t("settings.models.documentHint")}</span>
                </label>
              </div>
              <div className="row">
                <Button type="submit" disabled={!modelsDirty}>
                  {t("common.save")}
                </Button>
              </div>
            </form>
          </div>
        </div>
      </div>

      {confirmClear && (
        <ConfirmDialog
          title={t("settings.key.clearTitle")}
          body={t("settings.key.clearBody")}
          confirmLabel={t("settings.key.clear")}
          danger
          onConfirm={() => void clearKey()}
          onCancel={() => setConfirmClear(false)}
        />
      )}
    </div>
  );
}
