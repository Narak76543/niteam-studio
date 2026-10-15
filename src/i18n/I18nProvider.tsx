import { useEffect, useMemo, type ReactNode } from "react";

import { I18nContext, translate, type I18nValue, type MessageKey } from "./context";
import type { Language } from "../lib/types";

export function I18nProvider({ lang, children }: { lang: Language; children: ReactNode }) {
  useEffect(() => {
    document.documentElement.lang = lang;
  }, [lang]);

  const value = useMemo<I18nValue>(
    () => ({
      lang,
      t: (key: MessageKey, vars?: Record<string, string | number>) => translate(lang, key, vars),
    }),
    [lang],
  );

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}
