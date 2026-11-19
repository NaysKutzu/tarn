<script lang="ts">
  import { t as tr } from '../i18n.svelte';
  import type { Track } from '../types';
  import Cover from './Cover.svelte';
  import { api } from '../api';
  import {
    summarize, rangeStart, fmtListen, listenerType, RANGE_LABELS,
    type Listen, type StatsRange, type Summary,
  } from '../stats';

  const COVER_CACHE_KEY = 'tarn.artistCovers.v1';
  const COVER_CACHE_MAX = 200;

  function readCoverCache(): Record<string, string> {
    try {
      const raw = localStorage.getItem(COVER_CACHE_KEY);
      if (!raw) return {};
      const v = JSON.parse(raw) as Record<string, string>;
      return v && typeof v === 'object' ? v : {};
    } catch {
      return {};
    }
  }

  function writeCoverCache(map: Record<string, string>) {
    try {
      const entries = Object.entries(map);
      const trimmed =
        entries.length > COVER_CACHE_MAX ? Object.fromEntries(entries.slice(-COVER_CACHE_MAX)) : map;
      localStorage.setItem(COVER_CACHE_KEY, JSON.stringify(trimmed));
    } catch {
      /* quota / private mode */
    }
  }

  let {
    enabled,
    onPlay,
    onArtist,
    onAlbum,
    onEnable,
  }: {
    enabled: boolean;
    onPlay: (tracks: Track[], index: number) => void;
    onArtist: (id: string) => void;
    onAlbum: (id: string) => void;
    onEnable: () => void;
  } = $props();

  let range = $state<StatsRange>('month');
  let listens = $state<Listen[] | null>(null);
  let error = $state('');
  let confirmClear = $state(false);
  /** Real YTM artist portraits keyed by channel/browse id (not album art). */
  let artistCovers = $state<Record<string, string>>(readCoverCache());
  let coverGen = 0;

  async function load() {
    error = '';
    try {
      listens = await api.getListens(rangeStart(range));
    } catch (e) {
      error = String(e);
      listens = [];
    }
  }
  $effect(() => {
    void range;
    void load();
  });

  const s: Summary | null = $derived(listens ? summarize(listens) : null);
  const hourMax = $derived(s ? Math.max(1, ...s.byHour) : 1);
  const dayMax = $derived(s ? Math.max(1, ...s.byDay) : 1);
  const DAYS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];

  function artistCoverOf(a: { id: string | null; cover: string | null }): string | null {
    if (a.id && artistCovers[a.id]) return artistCovers[a.id];
    return a.cover;
  }

  /** Prefer channel portraits over the first track's album/video thumb. */
  async function enrichArtistCovers(summary: Summary) {
    const gen = ++coverGen;
    const need = summary.topArtists
      .map((a) => a.item)
      .filter((a) => a.id && !artistCovers[a.id!]);
    if (!need.length) return;

    // Library cards already carry channel avatars - fill those first, free.
    try {
      const lib = (await api.libraryCached().catch(() => null))
        ?? (await api.libraryLite().catch(() => null));
      if (gen === coverGen && lib?.artists?.length) {
        const next = { ...artistCovers };
        let hit = false;
        for (const a of need) {
          const id = a.id!;
          const card = lib.artists.find((x) => x.id === id && x.cover);
          if (card?.cover) {
            next[id] = card.cover;
            hit = true;
          }
        }
        if (hit) {
          artistCovers = next;
          writeCoverCache(next);
        }
      }
    } catch {
      /* offline / unsigned */
    }

    const still = need.filter((a) => a.id && !artistCovers[a.id!]);
    if (!still.length || gen !== coverGen) return;

    await Promise.all(
      still.map(async (a) => {
        const id = a.id!;
        try {
          let cover = await api.artistCover(id);
          // No browse id portrait - try search card avatar by exact name.
          if (!cover && a.name) {
            const res = await api.searchAll(a.name).catch(() => null);
            const match = res?.artists?.find(
              (x) => x.cover && x.name.toLowerCase() === a.name.toLowerCase(),
            ) ?? res?.artists?.find((x) => x.cover);
            cover = match?.cover ?? null;
          }
          if (gen !== coverGen || !cover) return;
          artistCovers = { ...artistCovers, [id]: cover };
          writeCoverCache(artistCovers);
        } catch {
          /* keep album-art fallback */
        }
      }),
    );
  }

  $effect(() => {
    if (s && s.plays > 0) void enrichArtistCovers(s);
  });

  async function clearAll() {
    if (!confirmClear) {
      confirmClear = true;
      setTimeout(() => (confirmClear = false), 3000);
      return;
    }
    confirmClear = false;
    await api.clearListens().catch(() => {});
    await load();
  }

  function niceDate(k: string): string {
    const [y, m, d] = k.split('-').map(Number);
    return new Date(y, m - 1, d).toLocaleDateString(undefined, { weekday: 'long', month: 'long', day: 'numeric' });
  }
</script>

<div class="stats">
  <div class="st-head">
    <h1 class="page-title">{tr('Your stats')}</h1>
    <div class="st-ranges" role="tablist" aria-label={tr('Period')}>
      {#each Object.entries(RANGE_LABELS) as [id, label]}
        <button type="button" role="tab" aria-selected={range === id} class:on={range === id} onclick={() => (range = id as StatsRange)}>{tr(label)}</button>
      {/each}
    </div>
  </div>

  {#if !enabled}
    <div class="st-empty">
      <p>{tr('Listening stats are off, so nothing new is recorded.')}</p>
      <button class="btn" type="button" onclick={onEnable}>{tr('Turn on')}</button>
    </div>
  {/if}

  {#if error}
    <p class="st-empty">Couldn't read your history: {error}</p>
  {:else if s && s.plays === 0}
    <div class="st-empty">
      <p>{tr('No listens in this period yet. Songs count after 30 seconds (or half of a short song).')}</p>
    </div>
  {:else if s}
    <section class="st-hero">
      <div class="st-big">
        <span class="n">{Math.round(s.secs / 60).toLocaleString()}</span>
        <span class="l">minutes listened</span>
      </div>
      <div class="st-chips">
        <div><b>{s.plays.toLocaleString()}</b><span>{tr('plays')}</span></div>
        <div><b>{s.tracks.toLocaleString()}</b><span>{tr('songs')}</span></div>
        <div><b>{s.artists.toLocaleString()}</b><span>artists</span></div>
        <div><b>{s.longestStreak}</b><span>day streak{s.currentStreak > 1 ? ` · ${s.currentStreak} now` : ''}</span></div>
      </div>
      <div class="st-type">{listenerType(s.byHour)}</div>
    </section>

    {#if s.topArtists[0]}
      {@const a = s.topArtists[0]}
      <section class="st-top-artist">
        <Cover src={artistCoverOf(a.item)} size={96} round alt="" />
        <div>
          <span class="t-meta">{tr('Top artist')}</span>
          <button class="st-link big" type="button" disabled={!a.item.id} onclick={() => a.item.id && onArtist(a.item.id)}>{a.item.name}</button>
          <span class="t-meta">{tr('{n} plays', { n: a.plays })} · {fmtListen(a.secs)}</span>
        </div>
      </section>
    {/if}

    <div class="st-cols">
      <section>
        <h2>{tr('Top songs')}</h2>
        <ol class="st-list">
          {#each s.topTracks as t, i (t.item.id)}
            <li>
              <button type="button" class="st-row" onclick={() => onPlay(s.topTracks.map((x) => x.item), i)}>
                <span class="rank">{i + 1}</span>
                <Cover src={t.item.cover} size={40} videoId={t.item.id} alt="" />
                <span class="txt">
                  <span class="clamp one">{t.item.title}</span>
                  <span class="t-meta clamp one">{(t.item.artists ?? []).join(', ')}</span>
                </span>
                <span class="t-meta cnt">{t.plays}×</span>
              </button>
            </li>
          {/each}
        </ol>
      </section>
      <section>
        <h2>{tr('Top artists')}</h2>
        <ol class="st-list">
          {#each s.topArtists as a, i (a.item.name)}
            <li>
              <button type="button" class="st-row" disabled={!a.item.id} onclick={() => a.item.id && onArtist(a.item.id)}>
                <span class="rank">{i + 1}</span>
                <Cover src={artistCoverOf(a.item)} size={40} round alt="" />
                <span class="txt"><span class="clamp one">{a.item.name}</span></span>
                <span class="t-meta cnt">{fmtListen(a.secs)}</span>
              </button>
            </li>
          {/each}
        </ol>
      </section>
    </div>

    {#if s.topAlbums.length}
      <section>
        <h2>{tr('Top albums')}</h2>
        <div class="st-albums">
          {#each s.topAlbums as a (a.item.name + a.item.artist)}
            <button type="button" class="st-album" disabled={!a.item.id} onclick={() => a.item.id && onAlbum(a.item.id)}>
              <Cover src={a.item.cover} size={120} alt="" />
              <span class="clamp one">{a.item.name}</span>
              <span class="t-meta clamp one">{a.item.artist} · {tr('{n} plays', { n: a.plays })}</span>
            </button>
          {/each}
        </div>
      </section>
    {/if}

    <div class="st-cols">
      <section>
        <h2>{tr('When you listen')}</h2>
        <div class="st-bars hours" aria-label={tr('Listening by hour')}>
          {#each s.byHour as v, h}
            <div class="bar" title="{h}:00 · {fmtListen(v)}"><i style="height:{(v / hourMax) * 100}%"></i></div>
          {/each}
        </div>
        <div class="st-axis"><span>0</span><span>6</span><span>12</span><span>18</span><span>23</span></div>
      </section>
      <section>
        <h2>{tr('By weekday')}</h2>
        <div class="st-bars days" aria-label={tr('Listening by weekday')}>
          {#each s.byDay as v, d}
            <div class="bar" title="{DAYS[d]} · {fmtListen(v)}"><i style="height:{(v / dayMax) * 100}%"></i><span>{DAYS[d]}</span></div>
          {/each}
        </div>
        {#if s.busiestDay}
          <p class="t-meta" style="margin-top:10px">Biggest day: {niceDate(s.busiestDay.date)} · {fmtListen(s.busiestDay.secs)}</p>
        {/if}
      </section>
    </div>
  {/if}

  <p class="t-meta st-foot">
    {tr('Stored only on this device.')}
    <button class="st-link" type="button" onclick={clearAll}>{confirmClear ? tr('Click again to delete all history') : tr('Clear history')}</button>
  </p>
</div>

<style>
  .stats { display: flex; flex-direction: column; gap: 22px; padding-bottom: 32px; }
  .st-head { display: flex; align-items: center; justify-content: space-between; gap: 16px; flex-wrap: wrap; }
  .st-head :global(.page-title) { margin: 0; }
  .st-ranges { display: flex; gap: 4px; padding: 3px; border-radius: 999px; background: var(--bg-elev); }
  .st-ranges button {
    border: 0; background: transparent; color: var(--text); opacity: 0.7;
    font: inherit; font-size: 12.5px; padding: 6px 12px; border-radius: 999px; cursor: pointer;
  }
  .st-ranges button.on { background: var(--accent); color: var(--accent-fg); opacity: 1; }
  .st-empty { padding: 28px; border-radius: var(--radius); background: var(--bg-elev); display: flex; gap: 14px; align-items: center; justify-content: space-between; flex-wrap: wrap; }
  .st-empty p { margin: 0; }
  h2 { font-size: 17px; margin: 0 0 10px; }
  .st-hero {
    position: relative; overflow: hidden; border-radius: 18px; padding: 28px 30px;
    background:
      radial-gradient(120% 140% at 0% 0%, rgba(var(--accent-rgb), 0.55), transparent 60%),
      radial-gradient(90% 120% at 100% 100%, rgba(var(--accent-rgb), 0.25), transparent 65%),
      var(--bg-elev);
    display: grid; grid-template-columns: 1fr auto; gap: 18px; align-items: end;
  }
  .st-big { display: flex; flex-direction: column; }
  .st-big .n { font-size: clamp(44px, 7vw, 76px); font-weight: 800; line-height: 1; letter-spacing: -0.03em; }
  .st-big .l { font-size: 15px; opacity: 0.85; margin-top: 6px; }
  .st-chips { display: grid; grid-template-columns: repeat(2, minmax(90px, auto)); gap: 10px 22px; }
  .st-chips div { display: flex; flex-direction: column; }
  .st-chips b { font-size: 22px; }
  .st-chips span { font-size: 12px; opacity: 0.75; }
  .st-type {
    position: absolute; top: 16px; right: 18px; font-size: 12px; font-weight: 600;
    padding: 4px 10px; border-radius: 999px; background: rgba(0, 0, 0, 0.28);
  }
  .st-top-artist { display: flex; gap: 18px; align-items: center; }
  .st-top-artist > div { display: flex; flex-direction: column; gap: 2px; }
  .st-top-artist :global(img.round),
  .st-row :global(img.round),
  .st-top-artist :global(.ph.round),
  .st-row :global(.ph.round) {
    flex: 0 0 auto;
    background: var(--bg-elev);
  }
  .st-link { border: 0; background: none; color: inherit; font: inherit; padding: 0; cursor: pointer; text-align: left; }
  .st-link:hover:not(:disabled) { text-decoration: underline; }
  .st-link.big { font-size: 28px; font-weight: 800; letter-spacing: -0.02em; }
  .st-foot .st-link { color: var(--accent); margin-left: 6px; }
  .st-cols { display: grid; grid-template-columns: repeat(auto-fit, minmax(280px, 1fr)); gap: 22px; }
  .st-list { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 2px; }
  .st-row {
    width: 100%; display: grid; grid-template-columns: 22px 40px 1fr auto; gap: 12px; align-items: center;
    border: 0; background: transparent; color: inherit; font: inherit; text-align: left;
    padding: 6px 8px; border-radius: 8px; cursor: pointer;
  }
  .st-row:hover:not(:disabled) { background: var(--bg-hover); }
  .st-row:disabled { cursor: default; }
  .st-row .rank { opacity: 0.55; font-variant-numeric: tabular-nums; text-align: right; }
  .st-row .txt { display: flex; flex-direction: column; min-width: 0; }
  .st-row .cnt { font-variant-numeric: tabular-nums; }
  .st-albums { display: grid; grid-template-columns: repeat(auto-fill, minmax(130px, 1fr)); gap: 16px; }
  .st-album {
    display: flex; flex-direction: column; gap: 6px; border: 0; background: transparent; color: inherit;
    font: inherit; text-align: left; padding: 0; cursor: pointer; min-width: 0;
  }
  .st-album:disabled { cursor: default; }
  .st-bars { display: flex; align-items: flex-end; gap: 3px; height: 120px; }
  .st-bars .bar { flex: 1; height: 100%; display: flex; flex-direction: column; justify-content: flex-end; align-items: center; gap: 4px; }
  .st-bars .bar i { display: block; width: 100%; min-height: 2px; border-radius: 3px 3px 0 0; background: rgba(var(--accent-rgb), 0.85); }
  .st-bars.days { gap: 8px; height: 140px; }
  .st-bars.days .bar span { font-size: 11px; opacity: 0.6; }
  .st-axis { display: flex; justify-content: space-between; font-size: 11px; opacity: 0.55; margin-top: 4px; }
  @media (max-width: 640px) {
    .st-hero { grid-template-columns: 1fr; padding: 22px; }
  }
</style>
