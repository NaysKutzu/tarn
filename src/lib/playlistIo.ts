import type { Track } from './types';

export interface ImportRow {
  title: string;
  artist: string;
  /** YouTube video id when the file already has one (our own export). */
  id?: string;
}

function cell(v: string): string {
  return /[",\n\r;]/.test(v) ? `"${v.replace(/"/g, '""')}"` : v;
}

/** Playlist as CSV: Title, Artist, Album, Duration (seconds), Video ID. */
export function tracksToCsv(tracks: Track[]): string {
  const head = ['Title', 'Artist', 'Album', 'Duration', 'Video ID'];
  const rows = tracks.map((t) => [
    t.title ?? '',
    (t.artists ?? []).join('; '),
    t.album ?? '',
    t.duration ? String(t.duration) : '',
    t.id ?? '',
  ]);
  return [head, ...rows].map((r) => r.map(cell).join(',')).join('\r\n') + '\r\n';
}

function splitRows(text: string, delim: string): string[][] {
  const rows: string[][] = [];
  let row: string[] = [];
  let cur = '';
  let quoted = false;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (quoted) {
      if (c === '"') {
        if (text[i + 1] === '"') { cur += '"'; i++; }
        else quoted = false;
      } else cur += c;
    } else if (c === '"') quoted = true;
    else if (c === delim) { row.push(cur); cur = ''; }
    else if (c === '\n' || c === '\r') {
      if (c === '\r' && text[i + 1] === '\n') i++;
      row.push(cur); cur = '';
      if (row.some((x) => x.trim())) rows.push(row);
      row = [];
    } else cur += c;
  }
  row.push(cur);
  if (row.some((x) => x.trim())) rows.push(row);
  return rows;
}

function pickDelimiter(first: string): string {
  const counts: Array<[string, number]> = [',', ';', '\t'].map((d) => [d, first.split(d).length - 1]);
  counts.sort((a, b) => b[1] - a[1]);
  return counts[0][1] > 0 ? counts[0][0] : ',';
}

const VIDEO_ID = /^[A-Za-z0-9_-]{11}$/;

/**
 * Accepts a CSV (our export, or Spotify exports such as Exportify: "Track Name",
 * "Artist Name(s)") or plain lines like "Artist - Title".
 */
export function parseImport(text: string): ImportRow[] {
  const clean = text.replace(/^﻿/, '').trim();
  if (!clean) return [];
  const firstLine = clean.split(/\r?\n/, 1)[0];
  const delim = pickDelimiter(firstLine);
  const rows = splitRows(clean, delim);
  if (!rows.length) return [];

  const header = rows[0].map((h) => h.trim().toLowerCase());
  // Names are tried in priority order: exact matches first, then prefixes
  // (never a URI/ID column, so "Track URI" is not mistaken for the title).
  const col = (...names: string[]) => {
    for (const n of names) {
      const i = header.indexOf(n);
      if (i >= 0) return i;
    }
    for (const n of names) {
      const i = header.findIndex((h) => h.startsWith(n) && !/\b(uri|url|id)\b/.test(h));
      if (i >= 0) return i;
    }
    return -1;
  };
  const ti = col('title', 'track name', 'song', 'name', 'track');
  const ai = col('artist', 'artist name(s)', 'artist name', 'artists');
  const ii = col('video id', 'videoid', 'id');

  if (ti >= 0 && (ai >= 0 || ii >= 0)) {
    const out: ImportRow[] = [];
    for (const r of rows.slice(1)) {
      const title = (r[ti] ?? '').trim();
      const artist = ai >= 0 ? (r[ai] ?? '').split(/[;,]/)[0].trim() : '';
      const rawId = ii >= 0 ? (r[ii] ?? '').trim() : '';
      if (!title && !rawId) continue;
      out.push({ title, artist, id: VIDEO_ID.test(rawId) ? rawId : undefined });
    }
    return out;
  }

  // Plain list: one song per line, "Artist - Title" or just a title.
  return clean
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter(Boolean)
    .map((l) => {
      const m = l.match(/^(.+?)\s+[-–—]\s+(.+)$/);
      return m ? { artist: m[1].trim(), title: m[2].trim() } : { artist: '', title: l };
    });
}
