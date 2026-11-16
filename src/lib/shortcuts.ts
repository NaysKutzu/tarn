// Configurable keyboard shortcuts. In-app combos use `KeyboardEvent.key`
// (so they follow the keyboard layout); global hotkeys use
// `KeyboardEvent.code` because the OS registers physical keys.

export type ShortcutAction =
  | 'play_pause' | 'next' | 'prev' | 'seek_forward' | 'seek_back'
  | 'volume_up' | 'volume_down' | 'like' | 'search' | 'show_window';

export const ACTION_LABELS: Record<ShortcutAction, string> = {
  play_pause: 'Play / pause',
  next: 'Next track',
  prev: 'Previous track',
  seek_forward: 'Seek forward 5s',
  seek_back: 'Seek back 5s',
  volume_up: 'Volume up',
  volume_down: 'Volume down',
  like: 'Like current track',
  search: 'Search',
  show_window: 'Show / hide Tarn',
};

/** In-app shortcuts, defaults = the keys Tarn always had. */
export const APP_ACTIONS: ShortcutAction[] = [
  'play_pause', 'next', 'prev', 'seek_forward', 'seek_back', 'volume_up', 'volume_down', 'like', 'search',
];
export const APP_DEFAULTS: Record<string, string> = {
  play_pause: 'Space',
  next: 'Ctrl+ArrowRight',
  prev: 'Ctrl+ArrowLeft',
  seek_forward: 'ArrowRight',
  seek_back: 'ArrowLeft',
  volume_up: '',
  volume_down: '',
  like: '',
  search: '/',
};

/** System-wide hotkeys (desktop). None are set by default. */
export const GLOBAL_ACTIONS: ShortcutAction[] = [
  'play_pause', 'next', 'prev', 'seek_forward', 'seek_back', 'volume_up', 'volume_down', 'like', 'show_window',
];

const MOD_KEYS = new Set(['Control', 'Alt', 'Shift', 'Meta', 'AltGraph', 'OS', 'Hyper', 'Super']);

function mods(e: KeyboardEvent, withShift: boolean): string[] {
  const m: string[] = [];
  if (e.ctrlKey) m.push('Ctrl');
  if (e.altKey) m.push('Alt');
  if (withShift && e.shiftKey) m.push('Shift');
  if (e.metaKey) m.push('Cmd');
  return m;
}

/** Combo for in-app matching, or null for a bare modifier press. */
export function appCombo(e: KeyboardEvent): string | null {
  if (MOD_KEYS.has(e.key)) return null;
  let key = e.key;
  if (key === ' ' || key === 'Spacebar') key = 'Space';
  // Printable symbols already include Shift (e.g. "/" is Shift+7 on some
  // layouts), so only letters and named keys keep it.
  const printable = key.length === 1;
  const letter = printable && /[a-z]/i.test(key);
  if (letter) key = key.toUpperCase();
  return [...mods(e, !printable || letter), key].join('+');
}

/** Accelerator for a global hotkey (physical key), or null. */
export function globalCombo(e: KeyboardEvent): string | null {
  if (MOD_KEYS.has(e.key) || !e.code) return null;
  return [...mods(e, true), e.code].join('+');
}

/** Effective in-app map: defaults overlaid with the user's changes. */
export function appShortcuts(saved: Record<string, string> | undefined): Record<string, string> {
  return { ...APP_DEFAULTS, ...(saved ?? {}) };
}

/** Action bound to `combo` in `map`, if any. An exact match wins; else
 *  Shift / Cmd are ignored, as the old fixed keys were (Shift+→ seeked). */
export function actionFor(map: Record<string, string>, combo: string): string | null {
  for (const [a, c] of Object.entries(map)) if (c && c === combo) return a;
  const loose = combo.split('+').filter((p, i, all) => i === all.length - 1 || (p !== 'Shift' && p !== 'Cmd')).join('+');
  if (loose === combo) return null;
  for (const [a, c] of Object.entries(map)) if (c && c === loose) return a;
  return null;
}

const NICE: Record<string, string> = {
  ArrowRight: '→', ArrowLeft: '←', ArrowUp: '↑', ArrowDown: '↓', Space: 'Space',
  Comma: ',', Period: '.', Slash: '/', Backslash: '\\', Minus: '-', Equal: '=',
  BracketLeft: '[', BracketRight: ']', Semicolon: ';', Quote: "'", Backquote: '`',
};

/** Human label for a combo ("Ctrl+Alt+KeyP" → "Ctrl + Alt + P"). */
export function prettyCombo(combo: string, mac = false): string {
  if (!combo) return 'Not set';
  return combo
    .split('+')
    .map((p) => {
      if (p === 'Cmd') return mac ? '⌘' : 'Super';
      if (p === 'Alt' && mac) return '⌥';
      if (NICE[p]) return NICE[p];
      const k = /^Key([A-Z])$/.exec(p) ?? /^Digit(\d)$/.exec(p) ?? /^Numpad(\d)$/.exec(p);
      return k ? k[1] : p;
    })
    .join(' + ');
}
