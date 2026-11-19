import type { Home } from './types';
import { sizedCover } from './cover';

/** Cap per shelf so localStorage stays under quota on fat YTM feeds. */
const MAX_ITEMS = 36;
const WARM_COVERS = 48;

export function homeCacheKey(account?: string | null): string {
  return `tarn.home.v2.${account || 'anon'}`;
}

export function clearHomeCaches() {
  try {
    const drop: string[] = [];
    for (let i = 0; i < localStorage.length; i++) {
      const k = localStorage.key(i);
      if (k && (k.startsWith('tarn.home.') || k === 'tarn.home.v1')) drop.push(k);
    }
    for (const k of drop) localStorage.removeItem(k);
  } catch {}
}

export function readHomeCache(account?: string | null): Home | null {
  try {
    const raw = localStorage.getItem(homeCacheKey(account));
    if (!raw) return null;
    const v = JSON.parse(raw);
    const h = v && v.home ? (v.home as Home) : (v as Home);
    // Never serve a logged-out (or other-account) feed when signed in.
    if (account && h && h.signed_in === false) return null;
    return h;
  } catch {
    return null;
  }
}

function trimHome(h: Home): Home {
  return {
    ...h,
    shelves: h.shelves.map((s) =>
      s.items.length <= MAX_ITEMS ? s : { ...s, items: s.items.slice(0, MAX_ITEMS) },
    ),
  };
}

export function writeHomeCache(h: Home, account?: string | null) {
  const key = homeCacheKey(account);
  const payload = JSON.stringify({ at: Date.now(), account: key, home: trimHome(h) });
  try {
    localStorage.setItem(key, payload);
  } catch {
    // Quota: drop older keys, then retry with a tighter trim.
    try {
      clearHomeCaches();
      const tight: Home = {
        ...h,
        shelves: h.shelves.slice(0, 10).map((s) => ({
          ...s,
          items: s.items.slice(0, 16),
        })),
      };
      localStorage.setItem(key, JSON.stringify({ at: Date.now(), account: key, home: tight }));
    } catch {}
  }
}

/** Reject the logged-out 3-shelf race (Throwback / Quick picks / …) as a “fresh” Home. */
export function homeFeedOk(h: Home | null | undefined): boolean {
  if (!h?.shelves?.length) return false;
  if (h.shelves.length >= 6) return true;
  return h.shelves.some((s) => {
    const t = s.title.toLowerCase();
    return (
      t.includes('listen again') ||
      t.includes('mixed for you') ||
      t.includes('forgotten') ||
      t.includes('covers and remix') ||
      t.includes('daily discover') ||
      t.includes('new release')
    );
  });
}

/** Prefer keeping a richer cached feed over a thin live miss. */
export function isHomeUpgrade(next: Home, prev: Home | null | undefined): boolean {
  if (!prev?.shelves?.length) return homeFeedOk(next) || !!next.shelves?.length;
  if (!homeFeedOk(next) && homeFeedOk(prev)) return false;
  if (prev.shelves.length >= 8 && next.shelves.length <= 4) return false;
  if (next.shelves.length + 2 < prev.shelves.length) return false;
  return true;
}

/** Cheap identity for “did the feed actually change?” before animating. */
export function homeFingerprint(h: Home): string {
  const chips = (h.chips ?? []).map((c) => c.title).join('\u001f');
  const shelves = (h.shelves ?? [])
    .map((s) => `${s.title}\u001e${s.items.map((i) => i.id || i.title).join(',')}`)
    .join('\u001f');
  return `${chips}\u001d${shelves}\u001d${h.signed_in ? 1 : 0}`;
}

/** Kick browser HTTP cache for the first shelf covers so restart paint isn’t blank tiles. */
export function warmHomeCovers(h: Home | null | undefined) {
  if (!h?.shelves?.length || typeof Image === 'undefined') return;
  let n = 0;
  for (const s of h.shelves) {
    for (const it of s.items) {
      if (!it.cover || n >= WARM_COVERS) return;
      const u = sizedCover(it.cover, 304, 304);
      if (!u) continue;
      const img = new Image();
      img.decoding = 'async';
      img.src = u;
      n++;
    }
  }
}
