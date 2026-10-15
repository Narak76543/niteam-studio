import { createContext, useContext } from "react";

import en from "./en.json";
import km from "./km.json";
import type { Language } from "../lib/types";

export type MessageKey = keyof typeof en;
export type Messages = Record<MessageKey, string>;

// Compile-time check: km.json must define every key of en.json.
export const MESSAGES: Record<Language, Messages> = { en, km };

export type TFn = (key: MessageKey, vars?: Record<string, string | number>) => string;

export interface I18nValue {
  lang: Language;
  t: TFn;
}

export const I18nContext = createContext<I18nValue | null>(null);

export function useI18n(): I18nValue {
  const ctx = useContext(I18nContext);
  if (!ctx) throw new Error("useI18n must be used inside <I18nProvider>");
  return ctx;
}

export function translate(lang: Language, key: MessageKey, vars?: Record<string, string | number>): string {
  const template = MESSAGES[lang][key];
  if (!vars) return template;
  return template.replace(/\{(\w+)\}/g, (m, name: string) =>
    name in vars ? String(vars[name]) : m,
  );
}

/** Khmer UI when the Windows display language is Khmer (WebView2 reports it). */
export function detectLanguage(): Language {
  const langs = navigator.languages.length ? navigator.languages : [navigator.language];
  return langs.some((l) => l.toLowerCase().startsWith("km")) ? "km" : "en";
}
