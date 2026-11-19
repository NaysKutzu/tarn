// App colour theme: dark (default), light, OLED black, or follow the system.
// The choice is mirrored to localStorage so the first paint (and the mini
// player window) uses it before settings load.

export type Theme = 'dark' | 'light' | 'oled' | 'system';

const KEY = 'tarn.theme';
let current: Theme = 'dark';
let mq: MediaQueryList | null = null;

function resolve(t: Theme): 'dark' | 'light' | 'oled' {
  if (t !== 'system') return t;
  return mq && !mq.matches ? 'light' : 'dark';
}

function paint() {
  const eff = resolve(current);
  const root = document.documentElement;
  if (eff === 'dark') delete root.dataset.theme;
  else root.dataset.theme = eff;
}

export function applyTheme(t: string | null | undefined) {
  current = t === 'light' || t === 'oled' || t === 'system' ? t : 'dark';
  try { localStorage.setItem(KEY, current); } catch { /* private mode */ }
  paint();
}

export function initTheme() {
  try {
    mq = window.matchMedia('(prefers-color-scheme: dark)');
    mq.addEventListener('change', () => { if (current === 'system') paint(); });
  } catch { mq = null; }
  let saved: string | null = null;
  try { saved = localStorage.getItem(KEY); } catch { /* ignore */ }
  applyTheme(saved);
}

/** Preset accents for the fixed-accent picker. */
export const ACCENTS = ['#e0a45a', '#e86a5c', '#e45c9a', '#a77bf3', '#5b8def', '#3fb5c9', '#4cbf7a', '#c9c24a'];

/** "#rrggbb" → [r, g, b], or null if it isn't a colour. */
export function hexRgb(hex: string | null | undefined): [number, number, number] | null {
  const m = /^#?([0-9a-f]{6})$/i.exec((hex ?? '').trim());
  if (!m) return null;
  const n = parseInt(m[1], 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}
