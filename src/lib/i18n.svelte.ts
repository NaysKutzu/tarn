// Tiny gettext-style i18n: the English text is the key, so anything not
// translated yet simply shows in English. Add a language by dropping a
// `<code>.json` (English → translation) next to `ro.json` and listing it in
// DICTS / LOCALES.
import ro from './i18n/ro.json';

export type Locale = 'en' | 'ro';
/** What the user picked in settings: a fixed language or "follow the OS". */
export type LocalePref = 'system' | Locale;

const DICTS: Record<Locale, Record<string, string>> = { en: {}, ro };

/** Options for the settings picker. Language names stay in their own language. */
export function localeOptions(): { value: LocalePref; label: string }[] {
  return [
    { value: 'system', label: t('System default') },
    { value: 'en', label: 'English' },
    { value: 'ro', label: 'Română' },
  ];
}

const KEY = 'tarn.ui_language';

/** The OS / webview language if we have a dictionary for it, else English. */
export function systemLocale(): Locale {
  try {
    const langs = typeof navigator !== 'undefined'
      ? (navigator.languages?.length ? navigator.languages : [navigator.language])
      : [];
    for (const l of langs) {
      const base = String(l || '').toLowerCase().split(/[-_]/)[0];
      if (base in DICTS) return base as Locale;
    }
  } catch { /* ignore */ }
  return 'en';
}

export function resolveLocale(pref: string | null | undefined): Locale {
  if (pref === 'en' || pref === 'ro') return pref;
  return systemLocale();
}

function initial(): Locale {
  let stored: string | null = null;
  try { stored = localStorage.getItem(KEY); } catch { /* ignore */ }
  return resolveLocale(stored);
}

export const i18n = $state<{ locale: Locale }>({ locale: initial() });

/** Apply a preference ('system' | 'en' | 'ro'). Cached so the first paint is already right. */
export function setLocale(pref: string | null | undefined) {
  const p: LocalePref = pref === 'en' || pref === 'ro' ? pref : 'system';
  i18n.locale = resolveLocale(p);
  try { localStorage.setItem(KEY, p); } catch { /* ignore */ }
  if (typeof document !== 'undefined') document.documentElement.lang = i18n.locale;
}

/**
 * Translate `s` for the current UI language. `{name}` placeholders are
 * filled from `vars` after translating.
 */
export function t(s: string, vars?: Record<string, string | number>): string {
  const d = DICTS[i18n.locale];
  let out = (d && d[s]) || s;
  if (vars) {
    for (const [k, v] of Object.entries(vars)) out = out.split(`{${k}}`).join(String(v));
  }
  return out;
}

if (typeof document !== 'undefined') document.documentElement.lang = i18n.locale;
