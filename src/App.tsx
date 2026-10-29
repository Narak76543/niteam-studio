import { getVersion } from "@tauri-apps/api/app";
import { useCallback, useEffect, useState } from "react";

import appIcon from "./assets/brand/app-icon.png";
import { Sidebar } from "./components/Sidebar";
import { ToastProvider } from "./components/ToastProvider";
import { UpdateDialog } from "./components/UpdateDialog";
import { detectLanguage } from "./i18n/context";
import { I18nProvider } from "./i18n/I18nProvider";
import { api, errorMessage } from "./lib/api";
import type { Route } from "./lib/route";
import { settingsPatch } from "./lib/settingsPatch";
import { useTheme } from "./lib/theme";
import { useUpdater } from "./lib/updater";
import type { Language, ProjectSummary, Settings } from "./lib/types";
import { NewProjectPage } from "./pages/NewProjectPage";
import { ProjectsPage } from "./pages/ProjectsPage";
import { RenderPage } from "./pages/RenderPage";
import { ScenesPage } from "./pages/ScenesPage";
import { ScriptPage } from "./pages/ScriptPage";
import { SettingsPage } from "./pages/SettingsPage";


/** Shown while settings load at start-up (usually a fraction of a second). */
function BootSplash() {
  return (
    <div className="boot" role="status" aria-busy="true">
      <img className="boot__logo" src={appIcon} alt="" />
      <span className="boot__bar" aria-hidden="true">
        <span />
      </span>
    </div>
  );
}

export default function App() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [bootError, setBootError] = useState<string | null>(null);
  const [route, setRoute] = useState<Route>({ name: "projects" });
  const [projects, setProjects] = useState<ProjectSummary[]>([]);
  const [hasKey, setHasKey] = useState(false);
  const [searchToken, setSearchToken] = useState(0);
  const [appVersion, setAppVersion] = useState("");
  const updater = useUpdater();

  useEffect(() => {
    void getVersion()
      .then(setAppVersion)
      .catch(() => setAppVersion(""));
  }, []);

  const refreshProjects = useCallback(async () => {
    try {
      setProjects(await api.projectList());
    } catch {
      // The projects page shows its own error on the next action.
    }
  }, []);

  const refreshKey = useCallback(async () => {
    try {
      setHasKey(await api.apiKeyExists());
    } catch {
      setHasKey(false);
    }
  }, []);

  useEffect(() => {
    api
      .settingsGet()
      .then(async (s) => {
        setSettings(s);
        const keyExists = await api.apiKeyExists().catch(() => false);
        setHasKey(keyExists);
        // First run: send the user to Settings to paste a key.
        if (!keyExists) setRoute({ name: "settings" });
      })
      .catch((e: unknown) => setBootError(errorMessage(e)));
    api
      .projectList()
      .then(setProjects)
      .catch(() => undefined);
  }, []);

  // Ctrl+K → focus project search.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setRoute({ name: "projects" });
        setSearchToken((n) => n + 1);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const navigate = useCallback(
    (r: Route) => {
      setRoute(r);
      void refreshProjects();
    },
    [refreshProjects],
  );

  const lang: Language = settings?.ui_language ?? detectLanguage();
  useTheme(settings?.theme ?? null);

  // Sections build `next` from this copy; only what they changed is sent and merged.
  const saveSettings = useCallback(
    async (next: Settings) => {
      const saved = await api.settingsSave(settingsPatch(settings, next));
      setSettings(saved);
      return saved;
    },
    [settings],
  );

  const reloadSettings = useCallback(async () => {
    try {
      setSettings(await api.settingsGet());
    } catch {
      // Keep the current copy; the next save reports any problem.
    }
  }, []);

  const toggleLanguage = useCallback(() => {
    if (!settings) return;
    void saveSettings({ ...settings, ui_language: lang === "km" ? "en" : "km" });
  }, [settings, lang, saveSettings]);

  if (bootError) {
    return (
      <I18nProvider lang={lang}>
        <div className="empty">
          <p>{bootError}</p>
        </div>
      </I18nProvider>
    );
  }
  if (!settings) return <BootSplash />;

  return (
    <I18nProvider lang={lang}>
      <ToastProvider>
        <div className="app">
          <Sidebar
            route={route}
            navigate={navigate}
            recent={projects}
            onSearch={() => {
              setRoute({ name: "projects" });
              setSearchToken((n) => n + 1);
            }}
            onToggleLanguage={toggleLanguage}
            compact={route.name === "render"}
            updater={updater}
          />
          <main className="shell">
            {route.name === "projects" && (
              <ProjectsPage
                projects={projects}
                hasKey={hasKey}
                navigate={navigate}
                onChanged={refreshProjects}
                searchToken={searchToken}
              />
            )}
            {route.name === "new" && (
              <NewProjectPage
                key={route.preset ?? "new"}
                initialPreset={route.preset}
                hasKey={hasKey}
                navigate={navigate}
              />
            )}
            {route.name === "script" && (
              <ScriptPage key={route.projectId} projectId={route.projectId} navigate={navigate} />
            )}
            {route.name === "scenes" && (
              <ScenesPage key={route.projectId} projectId={route.projectId} navigate={navigate} />
            )}
            {route.name === "render" && (
              <RenderPage key={route.projectId} projectId={route.projectId} navigate={navigate} />
            )}
            {route.name === "settings" && (
              <SettingsPage
                settings={settings}
                onSave={saveSettings}
                onKeyChanged={refreshKey}
                onReload={reloadSettings}
                updater={updater}
              />
            )}
          </main>
          {updater.dialogOpen && updater.update && <UpdateDialog updater={updater} currentVersion={appVersion} />}
        </div>
      </ToastProvider>
    </I18nProvider>
  );
}
