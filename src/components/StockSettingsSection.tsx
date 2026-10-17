import { ExternalLink, KeyRound } from "lucide-react";
import { useEffect, useState } from "react";

import { Badge } from "./Badge";
import { Button } from "./Button";
import { ErrorPanel } from "./ErrorPanel";
import { Notice } from "./Notice";
import { useToast } from "./toast";
import { useI18n } from "../i18n/context";
import { api, errorMessage } from "../lib/api";
import type { Settings, StockSettings } from "../lib/types";

const ICON = { size: 16, strokeWidth: 1.5 } as const;
const KEY_PAGES = {
  pixabay: "https://pixabay.com/api/docs/",
  pexels: "https://www.pexels.com/api/",
} as const;

type KeySource = keyof typeof KEY_PAGES;
type TestState =
  | { kind: "idle" }
  | { kind: "testing" }
  | { kind: "ok"; results: number }
  | { kind: "error"; errorKind: string | null; details: string | null };

/** One stock key: stored badge, test, clear, paste field, where to get one. */
function StockKeyRow({ source }: { source: KeySource }) {
  const { t } = useI18n();
  const toast = useToast();
  const [hasKey, setHasKey] = useState<boolean | null>(null);
  const [input, setInput] = useState("");
  const [test, setTest] = useState<TestState>({ kind: "idle" });

  useEffect(() => {
    api
      .stockKeyExists(source)
      .then(setHasKey)
      .catch(() => setHasKey(false));
  }, [source]);

  const runTest = async () => {
    setTest({ kind: "testing" });
    try {
      const r = await api.stockKeyTest(source);
      setTest(r.ok ? { kind: "ok", results: r.results } : { kind: "error", errorKind: r.error_kind, details: r.details });
    } catch (e) {
      setTest({ kind: "error", errorKind: null, details: errorMessage(e) });
    }
  };

  const save = async () => {
    try {
      await api.stockKeySet(source, input);
      setInput("");
      setHasKey(true);
      toast.show(t("settings.stock.keySaved"), "ok");
      await runTest();
    } catch (e) {
      toast.show(errorMessage(e), "danger");
    }
  };

  const clear = async () => {
    try {
      await api.stockKeyClear(source);
      setHasKey(false);
      setTest({ kind: "idle" });
      toast.show(t("settings.stock.keyCleared"), "ok");
    } catch (e) {
      toast.show(errorMessage(e), "danger");
    }
  };

  return (
    <div className="field">
      <div className="row">
        <KeyRound {...ICON} aria-hidden="true" />
        <span className="t-small">{t(`settings.stock.key.${source}`)}</span>
        {hasKey === null ? null : hasKey ? (
          <Badge tone="ok">{t("settings.stock.keyStored")}</Badge>
        ) : (
          <Badge tone="neutral">{t("settings.stock.keyMissing")}</Badge>
        )}
        {hasKey && (
          <>
            <Button variant="pill" disabled={test.kind === "testing"} onClick={() => void runTest()}>
              {test.kind === "testing" ? t("settings.key.testing") : t("settings.stock.test")}
            </Button>
            <Button variant="pill" onClick={() => void clear()}>
              {t("settings.stock.keyClear")}
            </Button>
          </>
        )}
      </div>
      <form
        className="row"
        onSubmit={(e) => {
          e.preventDefault();
          if (input.trim()) void save();
        }}
      >
        <input
          className="input input--grow"
          type="password"
          autoComplete="off"
          spellCheck={false}
          aria-label={t(`settings.stock.keyPlaceholder.${source}`)}
          placeholder={t(`settings.stock.keyPlaceholder.${source}`)}
          value={input}
          onChange={(e) => setInput(e.target.value)}
        />
        <Button type="submit" disabled={!input.trim()}>
          {t("settings.key.save")}
        </Button>
      </form>
      <button className="link t-small row" onClick={() => void api.openExternal(KEY_PAGES[source])}>
        {t(`settings.stock.getKey.${source}`)} <ExternalLink size={12} strokeWidth={1.5} aria-hidden="true" />
      </button>
      {test.kind === "ok" && <Notice tone="ok">{t("settings.stock.testOk", { count: test.results })}</Notice>}
      {test.kind === "error" && <ErrorPanel kind={test.errorKind} details={test.details} onRetry={() => void runTest()} />}
    </div>
  );
}

interface Props {
  settings: Settings;
  onSave: (s: Settings) => Promise<Settings>;
}

const SOURCES = ["commons", "openverse", "pixabay", "pexels"] as const;

/** Settings → Stock photos & videos (docs/11-real-world-visuals.md): sources, keys, CC BY-SA. */
export function StockSettingsSection({ settings, onSave }: Props) {
  const { t } = useI18n();
  const toast = useToast();
  const stock = settings.stock;

  const set = async (patch: Partial<StockSettings>) => {
    try {
      await onSave({ ...settings, stock: { ...stock, ...patch } });
    } catch (e) {
      toast.show(errorMessage(e), "danger");
    }
  };

  return (
    <div className="settings-row">
      <div className="settings-row__intro">
        <h2 className="t-h3">{t("settings.stock.title")}</h2>
        <p className="t-small">{t("settings.stock.body")}</p>
      </div>
      <div className="settings-row__control">
        {SOURCES.map((s) => (
          <label key={s} className="panel-switch">
            <span>
              <span className="t-small">{t(`settings.stock.source.${s}`)}</span>
              <span className="t-micro t-muted">{t(`settings.stock.sourceHint.${s}`)}</span>
            </span>
            <input type="checkbox" role="switch" checked={stock[s]} onChange={(e) => void set({ [s]: e.target.checked })} />
          </label>
        ))}

        <StockKeyRow source="pixabay" />
        <Notice>{t("settings.stock.pixabayFull")}</Notice>
        <StockKeyRow source="pexels" />

        <label className="panel-switch">
          <span>
            <span className="t-small">{t("settings.stock.bySa")}</span>
            <span className="t-micro t-muted">{t("settings.stock.bySaHint")}</span>
          </span>
          <input type="checkbox" role="switch" checked={stock.allow_by_sa} onChange={(e) => void set({ allow_by_sa: e.target.checked })} />
        </label>
        {stock.allow_by_sa && <Notice tone="warn">{t("settings.stock.bySaWarn")}</Notice>}
      </div>
    </div>
  );
}
