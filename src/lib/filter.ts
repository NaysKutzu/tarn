import type { Track } from './types';

/** Lowercase, strip diacritics and punctuation so "Beyoncé" matches "beyonce". */
export function norm(s: string | null | undefined): string {
  return (s ?? '')
    .normalize('NFD')
    .replace(/[̀-ͯ]/g, '')
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\s]+/gu, ' ')
    .replace(/\s+/g, ' ')
    .trim();
}

/** Split a query into tokens; every token must appear somewhere (AND). */
export function tokens(q: string): string[] {
  const n = norm(q);
  return n ? n.split(' ') : [];
}

export function matches(hay: string, toks: string[]): boolean {
  if (!toks.length) return true;
  const h = norm(hay);
  return toks.every((t) => h.includes(t));
}

export function trackHay(t: Track): string {
  return `${t.title} ${t.artists.join(' ')} ${t.album ?? ''}`;
}

/** Filter tracks by title / artist / album. Empty query returns the input. */
export function filterTracks(tracks: Track[], q: string): Track[] {
  const toks = tokens(q);
  if (!toks.length) return tracks;
  return tracks.filter((t) => matches(trackHay(t), toks));
}

export function filterNamed<T extends { name: string }>(
  items: T[],
  q: string,
  extra?: (x: T) => string,
): T[] {
  const toks = tokens(q);
  if (!toks.length) return items;
  return items.filter((x) => matches(`${x.name} ${extra ? extra(x) : ''}`, toks));
}
