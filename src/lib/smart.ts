// Smart playlists: saved rule sets evaluated locally against your liked
// songs, downloads, recent plays and the local listen log. Nothing is
// written to YouTube Music; the list is rebuilt every time you open it.
import type { Track } from './types';
import type { Listen } from './stats';

export type SmartSource = 'liked' | 'history' | 'downloads';
export type SmartSort = 'plays' | 'recent' | 'bpm_asc' | 'bpm_desc' | 'title' | 'random';

export interface SmartRules {
  /** Where candidate songs come from (union). */
  sources: SmartSource[];
  bpmMin: number | null;
  bpmMax: number | null;
  /** Treat half / double tempo as a match (a 64 BPM track fits 120-130). */
  bpmFold: boolean;
  /** Only songs with zero plays in the local listen log. */
  neverPlayed: boolean;
  /** Minimum plays within `playsDays` (0 = no limit). */
  minPlays: number;
  /** Window for play counts / "top" sorting, in days (0 = all time). */
  playsDays: number;
  /** Not played in the last N days (0 = off). */
  notPlayedDays: number;
  /** Case-insensitive substring of any artist name. */
  artist: string;
  /** Duration bounds in minutes. */
  minMinutes: number | null;
  maxMinutes: number | null;
  sort: SmartSort;
  /** Keep at most this many songs (0 = all). */
  limit: number;
}

export interface SmartPlaylist {
  id: string;
  name: string;
  rules: SmartRules;
}

export const DEFAULT_RULES: SmartRules = {
  sources: ['liked', 'history', 'downloads'],
  bpmMin: null,
  bpmMax: null,
  bpmFold: false,
  neverPlayed: false,
  minPlays: 0,
  playsDays: 30,
  notPlayedDays: 0,
  artist: '',
  minMinutes: null,
  maxMinutes: null,
  sort: 'plays',
  limit: 50,
};

/** Ready-made starting points shown on the empty page. */
export function presets(): Omit<SmartPlaylist, 'id'>[] {
  return [
    { name: 'Top 50 this month', rules: { ...DEFAULT_RULES, sources: ['history'], minPlays: 1, playsDays: 30, sort: 'plays', limit: 50 } },
    { name: '120-130 BPM', rules: { ...DEFAULT_RULES, bpmMin: 120, bpmMax: 130, bpmFold: true, sort: 'bpm_asc', limit: 0 } },
    { name: 'Never played likes', rules: { ...DEFAULT_RULES, sources: ['liked'], neverPlayed: true, sort: 'random', limit: 100 } },
    { name: 'Forgotten favourites', rules: { ...DEFAULT_RULES, sources: ['history'], minPlays: 3, playsDays: 0, notPlayedDays: 60, sort: 'plays', limit: 50 } },
  ];
}

const KEY = 'tarn.smart_playlists';

export function loadSmart(): SmartPlaylist[] {
  try {
    const raw = JSON.parse(localStorage.getItem(KEY) ?? '[]');
    if (!Array.isArray(raw)) return [];
    return raw
      .filter((p) => p && typeof p.id === 'string' && typeof p.name === 'string')
      .map((p) => ({ id: p.id, name: p.name, rules: { ...DEFAULT_RULES, ...(p.rules ?? {}) } }));
  } catch {
    return [];
  }
}

export function saveSmart(list: SmartPlaylist[]) {
  try { localStorage.setItem(KEY, JSON.stringify(list)); } catch { /* ignore */ }
}

export function newId(): string {
  return `sp_${Date.now().toString(36)}${Math.random().toString(36).slice(2, 7)}`;
}

export interface Pool {
  liked: Track[];
  history: Track[];
  downloads: Track[];
}

export interface SmartContext {
  /** All local listens (any age). */
  listens: Listen[];
  /** Cached BPM per track id. */
  bpms: Record<string, number>;
  /** Unix seconds "now" (for tests). */
  now?: number;
  /** Seeded order for the random sort (for tests). */
  random?: () => number;
}

/** BPM range check, optionally accepting half / double time. */
export function bpmMatches(bpm: number, min: number | null, max: number | null, fold: boolean): boolean {
  const lo = min ?? 0;
  const hi = max ?? Infinity;
  const inRange = (b: number) => b >= lo && b <= hi;
  if (inRange(bpm)) return true;
  return fold && (inRange(bpm * 2) || inRange(bpm / 2));
}

/** Evaluate rules → ordered tracks. Pure, so it can be unit-tested. */
export function evaluate(rules: SmartRules, pool: Pool, ctx: SmartContext): Track[] {
  const now = ctx.now ?? Math.floor(Date.now() / 1000);
  const since = rules.playsDays > 0 ? now - rules.playsDays * 86400 : 0;

  // Candidates, de-duplicated, first source wins (keeps metadata of likes).
  const seen = new Map<string, Track>();
  const order: string[] = [];
  const add = (t: Track) => {
    if (!t?.id || seen.has(t.id)) return;
    seen.set(t.id, t);
    order.push(t.id);
  };
  for (const s of rules.sources) for (const t of pool[s] ?? []) add(t);
  // "history" also means everything in the listen log.
  if (rules.sources.includes('history')) for (const l of ctx.listens) add(l.track);

  const playsAll = new Map<string, number>();
  const playsWin = new Map<string, number>();
  const lastAt = new Map<string, number>();
  for (const l of ctx.listens) {
    const id = l.track.id;
    playsAll.set(id, (playsAll.get(id) ?? 0) + 1);
    if (l.at >= since) playsWin.set(id, (playsWin.get(id) ?? 0) + 1);
    if ((lastAt.get(id) ?? 0) < l.at) lastAt.set(id, l.at);
  }

  const artist = rules.artist.trim().toLowerCase();
  const needBpm = rules.bpmMin != null || rules.bpmMax != null;
  const out = order
    .map((id) => seen.get(id)!)
    .filter((t) => {
      if (rules.neverPlayed && (playsAll.get(t.id) ?? 0) > 0) return false;
      if (rules.minPlays > 0 && (playsWin.get(t.id) ?? 0) < rules.minPlays) return false;
      if (rules.notPlayedDays > 0 && (lastAt.get(t.id) ?? 0) > now - rules.notPlayedDays * 86400) return false;
      if (artist && !t.artists.some((a) => a.toLowerCase().includes(artist))) return false;
      const mins = (t.duration ?? 0) / 60;
      if (rules.minMinutes != null && (!t.duration || mins < rules.minMinutes)) return false;
      if (rules.maxMinutes != null && (!t.duration || mins > rules.maxMinutes)) return false;
      if (needBpm) {
        const b = ctx.bpms[t.id];
        if (b == null || !bpmMatches(b, rules.bpmMin, rules.bpmMax, rules.bpmFold)) return false;
      }
      return true;
    });

  const idx = new Map(order.map((id, i) => [id, i]));
  const bpm = (t: Track) => ctx.bpms[t.id] ?? Infinity;
  switch (rules.sort) {
    case 'plays':
      out.sort((a, b) => (playsWin.get(b.id) ?? 0) - (playsWin.get(a.id) ?? 0) || idx.get(a.id)! - idx.get(b.id)!);
      break;
    case 'recent':
      out.sort((a, b) => (lastAt.get(b.id) ?? 0) - (lastAt.get(a.id) ?? 0) || idx.get(a.id)! - idx.get(b.id)!);
      break;
    case 'bpm_asc':
      out.sort((a, b) => bpm(a) - bpm(b));
      break;
    case 'bpm_desc':
      out.sort((a, b) => (ctx.bpms[b.id] ?? -1) - (ctx.bpms[a.id] ?? -1));
      break;
    case 'title':
      out.sort((a, b) => a.title.localeCompare(b.title));
      break;
    case 'random': {
      const r = ctx.random ?? Math.random;
      for (let i = out.length - 1; i > 0; i--) {
        const j = Math.floor(r() * (i + 1));
        [out[i], out[j]] = [out[j], out[i]];
      }
      break;
    }
  }
  return rules.limit > 0 ? out.slice(0, rules.limit) : out;
}

/** Short human summary of the rules, e.g. "120-130 BPM · never played". */
export function describe(r: SmartRules, t: (s: string, v?: Record<string, string | number>) => string): string {
  const parts: string[] = [];
  if (r.bpmMin != null || r.bpmMax != null) {
    parts.push(r.bpmMin != null && r.bpmMax != null
      ? t('{a}-{b} BPM', { a: r.bpmMin, b: r.bpmMax })
      : r.bpmMin != null ? t('≥ {a} BPM', { a: r.bpmMin }) : t('≤ {b} BPM', { b: r.bpmMax ?? 0 }));
  }
  if (r.neverPlayed) parts.push(t('never played'));
  if (r.minPlays > 0) parts.push(r.playsDays > 0
    ? t('{n}+ plays in {d} days', { n: r.minPlays, d: r.playsDays })
    : t('{n}+ plays', { n: r.minPlays }));
  if (r.notPlayedDays > 0) parts.push(t('not played in {d} days', { d: r.notPlayedDays }));
  if (r.artist.trim()) parts.push(t('artist: {a}', { a: r.artist.trim() }));
  if (r.limit > 0) parts.push(t('max {n}', { n: r.limit }));
  return parts.join(' · ') || t('All songs');
}
