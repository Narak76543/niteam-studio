import { useCallback, useEffect, useState } from "react";

import { Sidebar } from "./components/Sidebar";
import { ToastProvider } from "./components/ToastProvider";
import { detectLanguage } from "./i18n/context";
import { I18nProvider } from "./i18n/I18nProvider";
import { api, errorMessage } from "./lib/api";
import type { Route } from "./lib/route";
import type { Language, ProjectSummary, Settings } from "./lib/types";
import { NewProjectPage } from "./pages/NewProjectPage";
import { ProjectsPage } from "./pages/ProjectsPage";
import { ScenesPage } from "./pages/ScenesPage";
import { ScriptPage } from "./pages/ScriptPage";
import { SettingsPage } from "./pages/SettingsPage";

export default function App() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [bootError, setBootError] = useState<string | null>(null);
  const [route, setRoute] = useState<Route>({ name: "projects" });
  const [projects, setProjects] = useState<ProjectSummary[]>([]);
  const [hasKey, setHasKey] = useState(false);
  const [searchToken, setSearchToken] = useState(0);

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

  const saveSettings = useCallback(async (next: Settings) => {
    const saved = await api.settingsSave(next);
    setSettings(saved);
    return saved;
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
  if (!settings) return null;

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
            {route.name === "settings" && (
              <SettingsPage settings={settings} onSave={saveSettings} onKeyChanged={refreshKey} />
            )}
          </main>
        </div>
      </ToastProvider>
    </I18nProvider>
  );
}
