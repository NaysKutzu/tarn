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

const GLASS_KEY = 'tarn.glass';

function clampGlass(n: number | null | undefined): number {
  if (n == null || !Number.isFinite(n)) return 0;
  return Math.max(0, Math.min(100, Math.round(n)));
}

/** Paint CSS glass strength (0 = opaque). Does not touch the native window effect. */
function paintGlass(level: number) {
  const root = document.documentElement;
  if (level <= 0) {
    delete root.dataset.glass;
    root.style.removeProperty('--glass-a');
    root.style.removeProperty('--glass-mix');
    root.style.removeProperty('--glass-mix-main');
    return;
  }
  root.dataset.glass = '1';
  // Mix % of theme surface into transparent (higher = more solid). Floor keeps text readable.
  const mix = Math.round(Math.max(32, 90 - (level / 100) * 52));
  const a = Math.max(0.28, mix / 100);
  root.style.setProperty('--glass-a', String(a));
  root.style.setProperty('--glass-mix', `${mix}%`);
  root.style.setProperty('--glass-mix-main', `${Math.max(24, mix - 14)}%`);
}

/**
 * Desktop translucent chrome (Mica / Acrylic / macOS vibrancy).
 * `level` is 0..=100 - 0 turns glass off.
 */
export async function applyTransparency(level: number | boolean | null | undefined) {
  const n = typeof level === 'boolean' ? (level ? 70 : 0) : clampGlass(level);
  try { localStorage.setItem(GLASS_KEY, String(n)); } catch { /* ignore */ }
  paintGlass(n);

  try {
    const { getCurrentWindow, Effect, EffectState } = await import('@tauri-apps/api/window');
    const win = getCurrentWindow();
    if (n > 0) {
      await win.setEffects({
        effects: [Effect.Mica, Effect.TabbedDark, Effect.Acrylic, Effect.HudWindow, Effect.Sidebar],
        state: EffectState.Active,
      });
    } else {
      await win.clearEffects();
    }
  } catch {
    /* unsupported platform or missing permission */
  }
}

export function initTransparency() {
  let n = 0;
  try {
    const raw = localStorage.getItem(GLASS_KEY);
    if (raw === '1') n = 70;
    else if (raw === '0' || raw == null) n = 0;
    else n = clampGlass(Number(raw));
  } catch { /* ignore */ }
  paintGlass(n);
}

const FLOAT_KEY = 'tarn.float';

/** Detached chrome panels with gaps and rounded corners. */
export function applyFloating(on: boolean | null | undefined) {
  const enabled = !!on;
  try { localStorage.setItem(FLOAT_KEY, enabled ? '1' : '0'); } catch { /* ignore */ }
  const root = document.documentElement;
  if (enabled) root.dataset.float = '1';
  else delete root.dataset.float;
}

export function initFloating() {
  let on = false;
  try { on = localStorage.getItem(FLOAT_KEY) === '1'; } catch { /* ignore */ }
  applyFloating(on);
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
