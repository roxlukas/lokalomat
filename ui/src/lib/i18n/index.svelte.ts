import pl from '../../../locales/pl.json';
import en from '../../../locales/en.json';
import { interpolate, lookup, type Dictionary, type Params } from './translate';

export const LOCALES = ['pl', 'en'] as const;
export type Locale = (typeof LOCALES)[number];

const dictionaries: Record<Locale, Dictionary> = { pl, en };

/** Adds (deep-merges) the texts of an edition, e.g. Lokalomat Pro's own (ADR 0035). */
export function registerMessages(locale: Locale, messages: Dictionary): void {
  const merge = (into: Dictionary, from: Dictionary) => {
    for (const [k, v] of Object.entries(from)) {
      const existing = into[k];
      if (typeof v === 'string' || typeof existing !== 'object' || existing === undefined) into[k] = v;
      else merge(existing, v);
    }
  };
  // The bundled dictionaries are shared JSON modules: merge into a copy.
  dictionaries[locale] = structuredClone(dictionaries[locale]);
  merge(dictionaries[locale], messages);
}

let current = $state<Locale>('pl');

export function getLocale(): Locale {
  return current;
}

export function setLocale(locale: Locale): void {
  current = locale;
  if (typeof document !== 'undefined') document.documentElement.lang = locale;
}

/** Translates `key` in the current locale, falling back to Polish, then to the key itself. */
export function t(key: string, params?: Params): string {
  const text = lookup(dictionaries[current], key) ?? lookup(dictionaries.pl, key);
  if (text === undefined) {
    if (import.meta.env.DEV) console.warn(`[i18n] missing key: ${key}`);
    return key;
  }
  return interpolate(text, params);
}
