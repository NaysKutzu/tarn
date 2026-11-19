// Turns the raw local listen log into "Wrapped"-style numbers. Everything
// is computed in local time, on the device.
import type { Track } from './types';

export interface Listen {
  /** Unix seconds when the track started. */
  at: number;
  /** Seconds actually played. */
  secs: number;
  track: Track;
}

export type StatsRange = 'week' | 'month' | 'year' | 'all';

export const RANGE_LABELS: Record<StatsRange, string> = {
  week: 'Last 7 days',
  month: 'Last 30 days',
  year: 'This year',
  all: 'All time',
};

/** Unix seconds where a range starts. */
export function rangeStart(r: StatsRange, now = new Date()): number {
  const day = 86400;
  const t = Math.floor(now.getTime() / 1000);
  if (r === 'week') return t - 7 * day;
  if (r === 'month') return t - 30 * day;
  if (r === 'year') return Math.floor(new Date(now.getFullYear(), 0, 1).getTime() / 1000);
  return 0;
}

export interface Ranked<T> { item: T; plays: number; secs: number }

export interface Summary {
  plays: number;
  secs: number;
  tracks: number;
  artists: number;
  topTracks: Ranked<Track>[];
  topArtists: Ranked<{ name: string; id: string | null; cover: string | null }>[];
  topAlbums: Ranked<{ name: string; id: string | null; cover: string | null; artist: string }>[];
  /** Seconds per local hour 0-23. */
  byHour: number[];
  /** Seconds per weekday, Monday first. */
  byDay: number[];
  longestStreak: number;
  currentStreak: number;
  busiestDay: { date: string; secs: number } | null;
}

function dayKey(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}

function rank<T>(m: Map<string, Ranked<T>>, n: number): Ranked<T>[] {
  return [...m.values()].sort((a, b) => b.plays - a.plays || b.secs - a.secs).slice(0, n);
}

export function summarize(listens: Listen[], now = new Date()): Summary {
  const tracks = new Map<string, Ranked<Track>>();
  const artists = new Map<string, Ranked<{ name: string; id: string | null; cover: string | null }>>();
  const albums = new Map<string, Ranked<{ name: string; id: string | null; cover: string | null; artist: string }>>();
  const byHour = new Array(24).fill(0);
  const byDay = new Array(7).fill(0);
  const days = new Map<string, number>();
  let secs = 0;
  for (const l of listens) {
    const t = l.track;
    secs += l.secs;
    const d = new Date(l.at * 1000);
    byHour[d.getHours()] += l.secs;
    byDay[(d.getDay() + 6) % 7] += l.secs;
    const k = dayKey(d);
    days.set(k, (days.get(k) ?? 0) + l.secs);
    const tr = tracks.get(t.id) ?? { item: t, plays: 0, secs: 0 };
    tr.plays++; tr.secs += l.secs; tr.item = t;
    tracks.set(t.id, tr);
    (t.artists ?? []).forEach((name, i) => {
      if (!name) return;
      const key = name.toLowerCase();
      const a = artists.get(key) ?? { item: { name, id: t.artist_ids?.[i] ?? null, cover: null }, plays: 0, secs: 0 };
      a.plays++; a.secs += l.secs;
      if (!a.item.id && t.artist_ids?.[i]) a.item.id = t.artist_ids[i];
      a.item.cover ??= t.cover ?? null;
      artists.set(key, a);
    });
    if (t.album) {
      const key = t.album_id ?? `${t.album}\u0000${t.artists?.[0] ?? ''}`;
      const a = albums.get(key) ?? { item: { name: t.album, id: t.album_id ?? null, cover: t.cover ?? null, artist: t.artists?.[0] ?? '' }, plays: 0, secs: 0 };
      a.plays++; a.secs += l.secs;
      a.item.cover ??= t.cover ?? null;
      albums.set(key, a);
    }
  }
  // Streaks over local calendar days.
  const sorted = [...days.keys()].sort();
  let longest = 0, run = 0, prev: Date | null = null;
  for (const k of sorted) {
    const [y, m, dd] = k.split('-').map(Number);
    const d = new Date(y, m - 1, dd);
    run = prev && Math.round((d.getTime() - prev.getTime()) / 86400000) === 1 ? run + 1 : 1;
    longest = Math.max(longest, run);
    prev = d;
  }
  let current = 0;
  const cur = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  if (!days.has(dayKey(cur))) cur.setDate(cur.getDate() - 1);
  while (days.has(dayKey(cur))) { current++; cur.setDate(cur.getDate() - 1); }
  let busiest: { date: string; secs: number } | null = null;
  for (const [date, s] of days) if (!busiest || s > busiest.secs) busiest = { date, secs: s };
  return {
    plays: listens.length,
    secs,
    tracks: tracks.size,
    artists: artists.size,
    topTracks: rank(tracks, 10),
    topArtists: rank(artists, 10),
    topAlbums: rank(albums, 6),
    byHour,
    byDay,
    longestStreak: longest,
    currentStreak: current,
    busiestDay: busiest,
  };
}

/** "3 h 12 min", "45 min". */
export function fmtListen(secs: number): string {
  const m = Math.round(secs / 60);
  if (m < 60) return `${m} min`;
  const h = Math.floor(m / 60);
  return m % 60 ? `${h} h ${m % 60} min` : `${h} h`;
}
