import type { Track } from './types';

const LIKES_KEY = 'tarn.likes.v1';
const RECENTS_KEY = 'tarn.recents.v1';
const MAX_LIKES = 500;
const MAX_RECENTS = 80;

function read<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    if (!raw) return fallback;
    return JSON.parse(raw) as T;
  } catch {
    return fallback;
  }
}

function write(key: string, v: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(v));
  } catch {
    /* quota */
  }
}

export function localLikes(): Track[] {
  const v = read<Track[]>(LIKES_KEY, []);
  return Array.isArray(v) ? v.filter((t) => t && t.id) : [];
}

export function isLocalLiked(id: string): boolean {
  return !!id && localLikes().some((t) => t.id === id);
}

export function setLocalLiked(track: Track, on: boolean): Track[] {
  let list = localLikes().filter((t) => t.id !== track.id);
  if (on) list = [track, ...list].slice(0, MAX_LIKES);
  write(LIKES_KEY, list);
  return list;
}

export function localRecents(): Track[] {
  const v = read<Track[]>(RECENTS_KEY, []);
  return Array.isArray(v) ? v.filter((t) => t && t.id) : [];
}

export function recordRecent(track: Track): Track[] {
  if (!track?.id) return localRecents();
  const list = [track, ...localRecents().filter((t) => t.id !== track.id)].slice(0, MAX_RECENTS);
  write(RECENTS_KEY, list);
  return list;
}

const SEARCH_HIST_KEY = 'tarn.search_hist.v1';
const MAX_SEARCH_HIST = 12;

export function localSearchHistory(): string[] {
  const v = read<unknown>(SEARCH_HIST_KEY, []);
  if (!Array.isArray(v)) return [];
  return v
    .map((q) => (typeof q === 'string' ? q.trim() : ''))
    .filter(Boolean)
    .slice(0, MAX_SEARCH_HIST);
}

export function recordSearch(q: string): string[] {
  const qq = q.trim();
  if (!qq) return localSearchHistory();
  const key = qq.toLowerCase();
  const list = [qq, ...localSearchHistory().filter((x) => x.toLowerCase() !== key)].slice(0, MAX_SEARCH_HIST);
  write(SEARCH_HIST_KEY, list);
  return list;
}

export function removeSearch(q: string): string[] {
  const key = q.trim().toLowerCase();
  const list = localSearchHistory().filter((x) => x.toLowerCase() !== key);
  write(SEARCH_HIST_KEY, list);
  return list;
}

export function clearSearchHistory(): string[] {
  write(SEARCH_HIST_KEY, []);
  return [];
}

const SIDE_PANEL_KEY = 'tarn.side_panel.v1';
export type SidePanelId = 'queue' | 'lyrics' | 'related';

/** Last open side panel tab (null = closed). Survives restarts. */
export function loadSidePanel(): SidePanelId | null {
  const v = read<unknown>(SIDE_PANEL_KEY, null);
  return v === 'queue' || v === 'lyrics' || v === 'related' ? v : null;
}

export function saveSidePanel(id: SidePanelId | null) {
  write(SIDE_PANEL_KEY, id);
}
