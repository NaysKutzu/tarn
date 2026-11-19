// Unit checks for smart playlist rules. Run: node --experimental-strip-types scripts/test-smart.ts
import { evaluate, DEFAULT_RULES, bpmMatches } from '../src/lib/smart.ts';
const tr = (id: string, artists = ['A'], duration = 200) => ({ id, title: id, artists, artist_ids: [], album: null, album_id: null, duration, cover: null, is_video: false });
const now = 1_700_000_000;
const L = (id: string, daysAgo: number) => ({ at: now - daysAgo * 86400, secs: 100, track: tr(id) });
const pool = { liked: [tr('a'), tr('b', ['Bob']), tr('c')], history: [], downloads: [tr('d')] };
const ctx = { listens: [L('a', 1), L('a', 2), L('b', 40), L('e', 3)], bpms: { a: 125, b: 64, c: 90 }, now };
const ids = (r: any) => evaluate({ ...DEFAULT_RULES, ...r }, pool, ctx).map((t) => t.id).join(',');
const eq = (a: string, b: string) => { if (a !== b) { console.error('FAIL', a, '!=', b); process.exit(1); } };
eq(ids({ limit: 0 }), 'a,e,b,c,d');
eq(ids({ neverPlayed: true }), 'c,d');
eq(ids({ bpmMin: 120, bpmMax: 130 }), 'a');
eq(ids({ bpmMin: 120, bpmMax: 130, bpmFold: true, sort: 'bpm_asc' }), 'b,a');
eq(ids({ minPlays: 1, playsDays: 30 }), 'a,e');
eq(ids({ artist: 'bo' }), 'b');
eq(ids({ notPlayedDays: 30, sources: ['history'] }), 'b');
eq(ids({ sources: ['liked'], limit: 2 }), 'a,b');
eq(String(bpmMatches(250, 120, 130, true)), 'true');
console.log('smart ok');
