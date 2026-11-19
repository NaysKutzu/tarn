import type { Track } from '../lib/types';

export type RepeatMode = 'off' | 'all' | 'one';

export interface RemoteState {
  track: Track | null;
  playing: boolean;
  position: number;
  duration: number;
  volume: number;
  liked: boolean;
  disliked: boolean;
  shuffle: boolean;
  repeat: RepeatMode;
  index: number;
  queue: Track[];
  queue_start: number;
}

export interface LyricLine {
  text: string;
  start_ms: number | null;
}

export interface Lyrics {
  lines: LyricLine[];
  synced: boolean;
}

function readToken(): string {
  const fromHash = decodeURIComponent(location.hash.slice(1));
  if (fromHash) {
    try {
      sessionStorage.setItem('tarn-token', fromHash);
    } catch {
      /* ignore */
    }
    history.replaceState(null, '', location.pathname);
    return fromHash;
  }
  try {
    return sessionStorage.getItem('tarn-token') || '';
  } catch {
    return '';
  }
}

export const token = readToken();

export async function api<T = unknown>(path: string, body?: unknown): Promise<T> {
  const r = await fetch(path, body
    ? {
        method: 'POST',
        headers: { 'X-Tarn-Token': token, 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      }
    : { headers: { 'X-Tarn-Token': token } });
  if (r.status === 401) {
    throw new Error('Wrong or expired link. Scan the QR code in Tarn again.');
  }
  if (r.status === 503) throw new Error('Tarn is in offline mode');
  if (!r.ok) throw new Error('Tarn answered ' + r.status);
  return (r.status === 204 ? null : r.json()) as Promise<T>;
}

export function cmd(c: string, extra?: Record<string, unknown>) {
  return api('/api/cmd', { cmd: c, ...(extra || {}) });
}

export function coverUrl(t: Track | null | undefined): string {
  const c = t?.cover;
  if (!c || !/^https:/.test(c)) return '';
  // Safe for CSS url('…'). Reject anything that could break out.
  if (/['"\\]|[\s<>]/.test(c)) return '';
  return c;
}
