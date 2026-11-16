import type { Track } from './types';

/**
 * Scrobbling to Last.fm and/or ListenBrainz, straight from the UI.
 * Settings live in localStorage. The Last.fm password is only used once to get
 * a session key and is never stored.
 */

export interface ScrobbleConfig {
  lastfm: { apiKey: string; secret: string; session: string; user: string };
  listenbrainz: { token: string };
}

const KEY = 'tarn.scrobble.v1';
const empty = (): ScrobbleConfig => ({
  lastfm: { apiKey: '', secret: '', session: '', user: '' },
  listenbrainz: { token: '' },
});

export function loadConfig(): ScrobbleConfig {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return empty();
    const p = JSON.parse(raw);
    const e = empty();
    return {
      lastfm: { ...e.lastfm, ...(p.lastfm ?? {}) },
      listenbrainz: { ...e.listenbrainz, ...(p.listenbrainz ?? {}) },
    };
  } catch {
    return empty();
  }
}

export function saveConfig(c: ScrobbleConfig) {
  try { localStorage.setItem(KEY, JSON.stringify(c)); } catch { /* storage unavailable */ }
}

/* ---------- MD5 (Last.fm signs requests with it) ---------- */

export function md5(input: string): string {
  const msg = new TextEncoder().encode(input);
  const n = msg.length;
  const total = (((n + 8) >>> 6) + 1) << 6;
  const buf = new Uint8Array(total);
  buf.set(msg);
  buf[n] = 0x80;
  const dv = new DataView(buf.buffer);
  dv.setUint32(total - 8, (n * 8) >>> 0, true);
  dv.setUint32(total - 4, Math.floor((n * 8) / 4294967296), true);

  const S = [7, 12, 17, 22, 5, 9, 14, 20, 4, 11, 16, 23, 6, 10, 15, 21];
  const K = new Uint32Array(64);
  for (let i = 0; i < 64; i++) K[i] = Math.floor(Math.abs(Math.sin(i + 1)) * 4294967296) >>> 0;

  let a0 = 0x67452301, b0 = 0xefcdab89, c0 = 0x98badcfe, d0 = 0x10325476;
  const M = new Uint32Array(16);
  for (let off = 0; off < total; off += 64) {
    for (let i = 0; i < 16; i++) M[i] = dv.getUint32(off + i * 4, true);
    let A = a0, B = b0, C = c0, D = d0;
    for (let i = 0; i < 64; i++) {
      let F: number, g: number;
      if (i < 16) { F = (B & C) | (~B & D); g = i; }
      else if (i < 32) { F = (D & B) | (~D & C); g = (5 * i + 1) % 16; }
      else if (i < 48) { F = B ^ C ^ D; g = (3 * i + 5) % 16; }
      else { F = C ^ (B | ~D); g = (7 * i) % 16; }
      F = (F + A + K[i] + M[g]) >>> 0;
      A = D; D = C; C = B;
      const s = S[(i >> 4) * 4 + (i % 4)];
      B = (B + ((F << s) | (F >>> (32 - s)))) >>> 0;
    }
    a0 = (a0 + A) >>> 0; b0 = (b0 + B) >>> 0; c0 = (c0 + C) >>> 0; d0 = (d0 + D) >>> 0;
  }
  const out = new DataView(new ArrayBuffer(16));
  out.setUint32(0, a0, true); out.setUint32(4, b0, true);
  out.setUint32(8, c0, true); out.setUint32(12, d0, true);
  let hex = '';
  for (let i = 0; i < 16; i++) hex += out.getUint8(i).toString(16).padStart(2, '0');
  return hex;
}

/* ---------- Last.fm ---------- */

const LFM = 'https://ws.audioscrobbler.com/2.0/';

async function lfmCall(
  cfg: ScrobbleConfig['lastfm'],
  params: Record<string, string>,
  withSession: boolean,
): Promise<any> {
  const p: Record<string, string> = { ...params, api_key: cfg.apiKey };
  if (withSession) p.sk = cfg.session;
  const sig = Object.keys(p)
    .sort()
    .map((k) => k + p[k])
    .join('');
  const body = new URLSearchParams({ ...p, api_sig: md5(sig + cfg.secret), format: 'json' });
  const res = await fetch(LFM, { method: 'POST', body });
  const json = await res.json().catch(() => ({}));
  if (json.error) throw new Error(json.message || `Last.fm error ${json.error}`);
  if (!res.ok) throw new Error(`Last.fm HTTP ${res.status}`);
  return json;
}

/** Log in with username + password; returns the session key. */
export async function lastfmLogin(
  apiKey: string,
  secret: string,
  user: string,
  password: string,
): Promise<{ session: string; user: string }> {
  const cfg = { apiKey, secret, session: '', user };
  const j = await lfmCall(cfg, { method: 'auth.getMobileSession', username: user, password }, false);
  const s = j?.session;
  if (!s?.key) throw new Error('Last.fm did not return a session');
  return { session: s.key as string, user: (s.name as string) || user };
}

/* ---------- ListenBrainz ---------- */

const LB = 'https://api.listenbrainz.org/1/submit-listens';

async function lbSubmit(token: string, type: 'single' | 'playing_now', t: Track, listenedAt?: number) {
  const meta = {
    track_name: t.title,
    artist_name: t.artists.join(', '),
    ...(t.album ? { release_name: t.album } : {}),
    additional_info: {
      media_player: 'Tarn',
      submission_client: 'Tarn',
      music_service: 'music.youtube.com',
      origin_url: t.id ? `https://music.youtube.com/watch?v=${t.id}` : undefined,
      duration_ms: t.duration ? t.duration * 1000 : undefined,
    },
  };
  const payload = [{ ...(listenedAt ? { listened_at: listenedAt } : {}), track_metadata: meta }];
  const res = await fetch(LB, {
    method: 'POST',
    headers: { Authorization: `Token ${token}`, 'Content-Type': 'application/json' },
    body: JSON.stringify({ listen_type: type, payload }),
  });
  if (!res.ok) throw new Error(`ListenBrainz HTTP ${res.status}`);
}

export async function listenbrainzCheck(token: string): Promise<string> {
  const res = await fetch('https://api.listenbrainz.org/1/validate-token', {
    headers: { Authorization: `Token ${token}` },
  });
  const j = await res.json().catch(() => ({}));
  if (!j.valid) throw new Error(j.message || 'Invalid token');
  return (j.user_name as string) || '';
}

/* ---------- The tracker ---------- */

function primaryArtist(t: Track): string {
  return t.artists[0] || '';
}

export class Scrobbler {
  private id: string | null = null;
  private startedAt = 0; // unix seconds
  private played = 0;
  private lastPos = 0;
  private sent = false;
  private cfg: ScrobbleConfig = loadConfig();

  reload() { this.cfg = loadConfig(); }

  private active() {
    return {
      lfm: !!(this.cfg.lastfm.apiKey && this.cfg.lastfm.secret && this.cfg.lastfm.session),
      lb: !!this.cfg.listenbrainz.token,
    };
  }

  /** Call on every player state change. */
  update(track: Track | null, position: number, duration: number, playing: boolean) {
    const act = this.active();
    if (!act.lfm && !act.lb) return;
    if (!track?.id || !track.title) return;

    const newTrack = track.id !== this.id;
    // Same track started over (repeat one / seek to the start after scrobbling).
    const restarted = !newTrack && this.sent && position + 5 < this.lastPos && position < 5;
    if (newTrack || restarted) {
      this.id = track.id;
      this.startedAt = Math.floor(Date.now() / 1000);
      this.played = 0;
      this.lastPos = position;
      this.sent = false;
      if (playing) this.nowPlaying(track, duration);
    }

    if (playing) {
      const dt = position - this.lastPos;
      // Real playback advances in small steps; bigger jumps are seeks.
      if (dt > 0 && dt <= 3) this.played += dt;
    }
    this.lastPos = position;

    // Last.fm rules: track longer than 30s, played half of it or 4 minutes.
    if (!this.sent && duration > 30 && this.played >= Math.min(duration / 2, 240)) {
      this.sent = true;
      this.scrobble(track, duration);
    }
  }

  private nowPlaying(t: Track, duration: number) {
    const { lfm, lb } = this.active();
    if (lfm) {
      lfmCall(this.cfg.lastfm, {
        method: 'track.updateNowPlaying',
        artist: primaryArtist(t),
        track: t.title,
        ...(t.album ? { album: t.album } : {}),
        ...(duration > 0 ? { duration: String(Math.round(duration)) } : {}),
      }, true).catch(() => {});
    }
    if (lb) lbSubmit(this.cfg.listenbrainz.token, 'playing_now', t).catch(() => {});
  }

  private scrobble(t: Track, duration: number) {
    const { lfm, lb } = this.active();
    const at = this.startedAt;
    if (lfm) {
      lfmCall(this.cfg.lastfm, {
        method: 'track.scrobble',
        'artist[0]': primaryArtist(t),
        'track[0]': t.title,
        'timestamp[0]': String(at),
        ...(t.album ? { 'album[0]': t.album } : {}),
        ...(duration > 0 ? { 'duration[0]': String(Math.round(duration)) } : {}),
      }, true).catch(() => {});
    }
    if (lb) lbSubmit(this.cfg.listenbrainz.token, 'single', t, at).catch(() => {});
  }
}
