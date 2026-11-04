// Minimal, typed i18n. Norwegian Bokmål is the default; English is the
// fallback for any missing key. Strings use {name} placeholders and
// "one|other" plural forms selected by the `count` variable.

import { create } from "zustand";
import { en, type Dict } from "./en";
import { nb } from "./nb";
import type { UserError } from "@/lib/types";

export type Locale = "nb" | "en";
export const DEFAULT_LOCALE: Locale = "nb";
export const LOCALES: { id: Locale; name: string }[] = [
  { id: "nb", name: "Norsk (bokmål)" },
  { id: "en", name: "English" },
];

const dicts: Record<Locale, Dict> = { nb, en };
/** BCP 47 tags for Intl date/number formatting. */
export const INTL_LOCALE: Record<Locale, string> = { nb: "nb-NO", en: "en-GB" };

type Leaves<T, P extends string = ""> = {
  [K in keyof T & string]: T[K] extends string ? `${P}${K}` : Leaves<T[K], `${P}${K}.`>;
}[keyof T & string];
export type Key = Leaves<Dict>;
export type Vars = Record<string, string | number>;

export function isLocale(v: unknown): v is Locale {
  return v === "nb" || v === "en";
}

const saved = (() => {
  try {
    const v = localStorage.getItem("sky.locale");
    return isLocale(v) ? v : null;
  } catch {
    return null;
  }
})();

export const useLocale = create<{ locale: Locale; setLocale(l: Locale): void }>((set) => ({
  locale: saved ?? DEFAULT_LOCALE,
  setLocale: (locale) => {
    try {
      localStorage.setItem("sky.locale", locale);
    } catch {
      /* private mode */
    }
    document.documentElement.lang = locale;
    set({ locale });
  },
}));

function lookup(dict: Dict, key: string): string | undefined {
  let v: unknown = dict;
  for (const part of key.split(".")) v = (v as Record<string, unknown> | undefined)?.[part];
  return typeof v === "string" ? v : undefined;
}

export function format(template: string, vars?: Vars, locale: Locale = DEFAULT_LOCALE): string {
  let s = template;
  if (s.includes("|")) {
    const [one, other] = s.split("|");
    s = Number(vars?.count) === 1 ? one : other;
  }
  return s.replace(/\{(\w+)\}/g, (_, k) => {
    const v = vars?.[k];
    if (v == null) return `{${k}}`;
    return typeof v === "number" ? v.toLocaleString(INTL_LOCALE[locale]) : v;
  });
}

/** Translate outside React (current locale). */
export function t(key: Key, vars?: Vars): string {
  const locale = useLocale.getState().locale;
  return format(lookup(dicts[locale], key) ?? lookup(en, key) ?? key, vars, locale);
}

/** Translate inside React; re-renders when the language changes. */
export function useT() {
  useLocale((s) => s.locale);
  return t;
}

export function intlLocale(): string {
  return INTL_LOCALE[useLocale.getState().locale];
}

/** Show storage-area names in the user's language ("Photos" → "Bilder"). */
function translateParams(d: Dict, params?: Record<string, string>): Record<string, string> | undefined {
  if (!params?.area) return params;
  const name = (d.areas as Record<string, string>)[params.area];
  return name ? { ...params, area: name } : params;
}

/** Localize an error from Sky Core using its stable code / rule. */
export function localizeError(e: UserError): { title: string; message: string } {
  const locale = useLocale.getState().locale;
  const d = dicts[locale];
  const byCode = (d.errors as Record<string, { title: string; message?: string }>)[e.code];
  const rule = e.rule ? (d.rules as Record<string, string>)[e.rule] : undefined;
  return {
    title: byCode?.title ?? e.title,
    message: rule ? format(rule, translateParams(d, e.params), locale) : (byCode?.message ?? e.message),
  };
}
