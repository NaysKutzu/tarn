<script lang="ts">
  import { t as tr } from '../i18n.svelte';
  import type { Track } from '../types';
  import Cover from './Cover.svelte';
  import { api } from '../api';
  import {
    summarize, rangeStart, fmtListen, RANGE_LABELS,
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
  const topArtist = $derived(s?.topArtists[0] ?? null);
  const topCover = $derived(topArtist ? artistCoverOf(topArtist.item) : null);

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

  function streakLabel(longest: number, current: number): string {
    if (current > 1) return `${longest} · ${current} now`;
    return String(longest);
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
    <section class="st-hero" class:has-art={!!topCover}>
      {#if topCover}
        <div class="st-hero-art" aria-hidden="true" style="--art: url({JSON.stringify(topCover)})"></div>
      {/if}
      <div class="st-hero-shade" aria-hidden="true"></div>

      <div class="st-hero-main">
        <div class="st-big">
          <span class="n">{fmtListen(s.secs)}</span>
          <span class="l">{tr('listened')}</span>
        </div>

        {#if topArtist}
          <div class="st-feat">
            <button
              type="button"
              class="st-feat-art"
              disabled={!topArtist.item.id}
              onclick={() => topArtist.item.id && onArtist(topArtist.item.id)}
              aria-label={topArtist.item.name}
            >
              <Cover src={topCover} size={132} round alt="" />
            </button>
            <div class="st-feat-txt">
              <button
                class="st-link big"
                type="button"
                disabled={!topArtist.item.id}
                onclick={() => topArtist.item.id && onArtist(topArtist.item.id)}
              >{topArtist.item.name}</button>
              <span class="st-feat-meta">{tr('{n} plays', { n: topArtist.plays })} · {fmtListen(topArtist.secs)}</span>
            </div>
          </div>
        {/if}
      </div>

      <ul class="st-kpis">
        <li><b>{s.plays.toLocaleString()}</b><span>{tr('plays')}</span></li>
        <li><b>{s.tracks.toLocaleString()}</b><span>{tr('songs')}</span></li>
        <li><b>{s.artists.toLocaleString()}</b><span>{tr('artists')}</span></li>
        <li><b>{streakLabel(s.longestStreak, s.currentStreak)}</b><span>{tr('day streak')}</span></li>
      </ul>
    </section>

    <div class="st-cols">
      <section>
        <h2 class="shelf-title">{tr('Top songs')}</h2>
        <ol class="st-list">
          {#each s.topTracks as t, i (t.item.id)}
            <li>
              <button type="button" class="st-row" class:top={i === 0} onclick={() => onPlay(s.topTracks.map((x) => x.item), i)}>
                <span class="rank">{i + 1}</span>
                <Cover src={t.item.cover} size={44} videoId={t.item.id} alt="" />
                <span class="txt">
                  <span class="clamp one title">{t.item.title}</span>
                  <span class="meta clamp one">{(t.item.artists ?? []).join(', ')}</span>
                </span>
                <span class="cnt">{t.plays}×</span>
              </button>
            </li>
          {/each}
        </ol>
      </section>
      <section>
        <h2 class="shelf-title">{tr('Top artists')}</h2>
        <ol class="st-list">
          {#each s.topArtists as a, i (a.item.name)}
            <li>
              <button type="button" class="st-row" class:top={i === 0} disabled={!a.item.id} onclick={() => a.item.id && onArtist(a.item.id)}>
                <span class="rank">{i + 1}</span>
                <Cover src={artistCoverOf(a.item)} size={44} round alt="" />
                <span class="txt"><span class="clamp one title">{a.item.name}</span></span>
                <span class="cnt">{fmtListen(a.secs)}</span>
              </button>
            </li>
          {/each}
        </ol>
      </section>
    </div>

    {#if s.topAlbums.length}
      <section class="st-albums-sec">
        <h2 class="shelf-title">{tr('Top albums')}</h2>
        <div class="st-albums">
          {#each s.topAlbums as a (a.item.name + a.item.artist)}
            <button type="button" class="st-album" disabled={!a.item.id} onclick={() => a.item.id && onAlbum(a.item.id)}>
              <span class="st-album-art">
                <Cover src={a.item.cover} size={140} alt="" />
              </span>
              <span class="clamp one title">{a.item.name}</span>
              <span class="meta clamp one">{a.item.artist} · {tr('{n} plays', { n: a.plays })}</span>
            </button>
          {/each}
        </div>
      </section>
    {/if}

    <div class="st-cols st-charts">
      <section>
        <h2 class="shelf-title">{tr('When you listen')}</h2>
        <div class="st-bars hours" aria-label={tr('Listening by hour')}>
          {#each s.byHour as v, h}
            <div class="bar" class:peak={v === hourMax && v > 0} title="{h}:00 · {fmtListen(v)}">
              <i style="height:{(v / hourMax) * 100}%"></i>
            </div>
          {/each}
        </div>
        <div class="st-axis"><span>0</span><span>6</span><span>12</span><span>18</span><span>23</span></div>
      </section>
      <section>
        <h2 class="shelf-title">{tr('By weekday')}</h2>
        <div class="st-bars days" aria-label={tr('Listening by weekday')}>
          {#each s.byDay as v, d}
            <div class="bar" class:peak={v === dayMax && v > 0} title="{DAYS[d]} · {fmtListen(v)}">
              <i style="height:{(v / dayMax) * 100}%"></i>
              <span>{DAYS[d]}</span>
            </div>
          {/each}
        </div>
        {#if s.busiestDay}
          <p class="st-busy">{tr('Biggest day')}: {niceDate(s.busiestDay.date)} · {fmtListen(s.busiestDay.secs)}</p>
        {/if}
      </section>
    </div>
  {/if}

  <p class="st-foot">
    {tr('Stored only on this device.')}
    <button class="st-link accent" type="button" onclick={clearAll}>{confirmClear ? tr('Click again to delete all history') : tr('Clear history')}</button>
  </p>
</div>

<style>
  .stats {
    display: flex;
    flex-direction: column;
    gap: 36px;
    padding-bottom: 40px;
  }

  .st-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    flex-wrap: wrap;
  }
  .st-head :global(.page-title) { margin: 0; }

  .st-ranges {
    display: flex;
    gap: 2px;
    padding: 3px;
    border-radius: 999px;
    background: var(--bg-elev);
    border: 1px solid var(--line);
  }
  .st-ranges button {
    border: 0;
    background: transparent;
    color: var(--text-2);
    font: inherit;
    font-size: 12.5px;
    font-weight: 500;
    padding: 7px 13px;
    border-radius: 999px;
    cursor: pointer;
    transition: background 160ms var(--ease), color 160ms var(--ease);
  }
  .st-ranges button:hover { color: var(--text); }
  .st-ranges button.on {
    background: var(--accent);
    color: var(--accent-fg);
  }
  .st-ranges button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .st-empty {
    padding: 28px;
    border-radius: 14px;
    background: var(--bg-elev);
    border: 1px solid var(--line);
    display: flex;
    gap: 14px;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
  }
  .st-empty p { margin: 0; color: var(--text-2); }

  .st-hero {
    position: relative;
    overflow: hidden;
    border-radius: 20px;
    padding: 28px 30px 22px;
    background: var(--bg-elev);
    border: 1px solid var(--line);
    display: flex;
    flex-direction: column;
    gap: 26px;
    isolation: isolate;
    animation: st-in 420ms var(--ease) both;
  }
  .st-hero-art {
    position: absolute;
    inset: -8% -4% auto auto;
    width: min(52%, 420px);
    aspect-ratio: 1;
    background-image: var(--art);
    background-size: cover;
    background-position: center;
    filter: blur(48px) saturate(1.25);
    opacity: 0.42;
    pointer-events: none;
    z-index: 0;
    transform: translate(8%, -12%);
  }
  .st-hero-shade {
    position: absolute;
    inset: 0;
    z-index: 0;
    pointer-events: none;
    background:
      radial-gradient(90% 120% at 0% 0%, rgba(var(--accent-rgb), 0.22), transparent 55%),
      linear-gradient(105deg, var(--bg-elev) 38%, transparent 78%);
  }
  .st-hero.has-art .st-hero-shade {
    background:
      radial-gradient(80% 100% at 100% 0%, transparent 20%, var(--bg-elev) 78%),
      linear-gradient(100deg, color-mix(in srgb, var(--bg-elev) 92%, transparent) 32%, transparent 72%);
  }

  .st-hero-main {
    position: relative;
    z-index: 1;
    display: grid;
    grid-template-columns: minmax(0, 1.1fr) minmax(0, 1fr);
    gap: 28px 36px;
    align-items: end;
  }

  .st-big {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }
  .st-big .n {
    font-size: clamp(36px, 5.8vw, 64px);
    font-weight: 800;
    line-height: 1;
    letter-spacing: -0.04em;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .st-big .l {
    font-size: 15px;
    font-weight: 500;
    color: var(--text-2);
    margin-top: 4px;
  }

  .st-feat {
    display: flex;
    align-items: center;
    gap: 18px;
    min-width: 0;
    justify-self: end;
  }
  .st-feat-art {
    flex: 0 0 auto;
    padding: 0;
    border: 0;
    background: none;
    border-radius: 50%;
    cursor: pointer;
    box-shadow: 0 18px 48px rgba(0, 0, 0, 0.45);
    transition: transform 220ms var(--ease);
  }
  .st-feat-art:hover:not(:disabled) { transform: scale(1.03); }
  .st-feat-art:disabled { cursor: default; }
  .st-feat-art:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 3px;
  }
  .st-feat-art :global(img),
  .st-feat-art :global(.ph) {
    display: block;
    border: 2px solid color-mix(in srgb, var(--text) 12%, transparent);
  }
  .st-feat-txt {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }
  .st-feat-meta {
    font-size: 13px;
    color: var(--text-2);
  }

  .st-kpis {
    position: relative;
    z-index: 1;
    list-style: none;
    margin: 0;
    padding: 16px 0 0;
    border-top: 1px solid var(--line);
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 12px 18px;
  }
  .st-kpis li {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .st-kpis b {
    font-size: 22px;
    font-weight: 700;
    letter-spacing: -0.02em;
    font-variant-numeric: tabular-nums;
    line-height: 1.15;
  }
  .st-kpis span {
    font-size: 12px;
    color: var(--text-3);
  }

  .st-link {
    border: 0;
    background: none;
    color: inherit;
    font: inherit;
    padding: 0;
    cursor: pointer;
    text-align: left;
  }
  .st-link:hover:not(:disabled) { color: var(--accent); }
  .st-link:disabled { cursor: default; }
  .st-link:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
    border-radius: 4px;
  }
  .st-link.big {
    font-size: clamp(22px, 3vw, 30px);
    font-weight: 750;
    letter-spacing: -0.03em;
    line-height: 1.15;
  }
  .st-link.accent { color: var(--accent); margin-left: 8px; }

  .st-cols {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
    gap: 32px 40px;
  }
  .st-cols :global(.shelf-title) { margin: 0 0 14px; }

  .st-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
  }
  .st-row {
    width: 100%;
    display: grid;
    grid-template-columns: 28px 44px minmax(0, 1fr) auto;
    gap: 12px;
    align-items: center;
    border: 0;
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: left;
    padding: 8px 10px;
    margin: 0 -10px;
    border-radius: 10px;
    cursor: pointer;
    transition: background 140ms var(--ease);
  }
  .st-row:hover:not(:disabled) { background: var(--bg-hover); }
  .st-row:disabled { cursor: default; }
  .st-row:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 0;
  }
  .st-row .rank {
    font-size: 13px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    text-align: right;
    color: var(--text-3);
  }
  .st-row.top .rank { color: var(--accent); }
  .st-row .txt {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .st-row .title { font-weight: 550; }
  .st-row .meta,
  .st-album .meta {
    font-size: 12px;
    line-height: 16px;
    color: var(--text-3);
  }
  .st-row .cnt {
    font-size: 12.5px;
    font-weight: 550;
    font-variant-numeric: tabular-nums;
    color: var(--text-2);
    padding-left: 8px;
  }
  .st-row :global(img.round),
  .st-row :global(.ph.round),
  .st-feat :global(img.round),
  .st-feat :global(.ph.round) {
    flex: 0 0 auto;
    background: color-mix(in srgb, var(--bg) 70%, var(--bg-elev));
  }

  .st-albums-sec :global(.shelf-title) { margin: 0 0 16px; }
  .st-albums {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(132px, 1fr));
    gap: 18px 16px;
  }
  .st-album {
    display: flex;
    flex-direction: column;
    gap: 8px;
    border: 0;
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: left;
    padding: 0;
    cursor: pointer;
    min-width: 0;
  }
  .st-album:disabled { cursor: default; }
  .st-album:hover:not(:disabled) .title { color: var(--accent); }
  .st-album:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 3px;
    border-radius: 8px;
  }
  .st-album-art {
    display: block;
    border-radius: 10px;
    overflow: hidden;
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.35);
    transition: transform 220ms var(--ease), box-shadow 220ms var(--ease);
  }
  .st-album:hover:not(:disabled) .st-album-art {
    transform: translateY(-2px);
    box-shadow: 0 14px 34px rgba(0, 0, 0, 0.42);
  }
  .st-album .title { font-weight: 550; font-size: 13px; }

  .st-charts :global(.shelf-title) { margin: 0 0 16px; }
  .st-bars {
    display: flex;
    align-items: flex-end;
    gap: 3px;
    height: 128px;
    padding: 12px 10px 0;
    border-radius: 14px;
    background: color-mix(in srgb, var(--bg-elev) 80%, transparent);
    border: 1px solid var(--line);
  }
  .st-bars .bar {
    flex: 1;
    height: 100%;
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .st-bars .bar i {
    display: block;
    width: 100%;
    min-height: 2px;
    border-radius: 4px 4px 1px 1px;
    background: color-mix(in srgb, var(--accent) 55%, var(--text-3));
    transition: height 280ms var(--ease), background 160ms var(--ease);
  }
  .st-bars .bar.peak i {
    background: var(--accent);
  }
  .st-bars.days {
    gap: 8px;
    height: 148px;
    padding-bottom: 10px;
  }
  .st-bars.days .bar span {
    font-size: 11px;
    font-weight: 500;
    color: var(--text-3);
  }
  .st-axis {
    display: flex;
    justify-content: space-between;
    font-size: 11px;
    color: var(--text-3);
    margin-top: 8px;
    padding: 0 4px;
  }
  .st-busy {
    margin: 12px 0 0;
    font-size: 12.5px;
    color: var(--text-2);
  }

  .st-foot {
    margin: 0;
    font-size: 12px;
    color: var(--text-3);
  }

  @keyframes st-in {
    from {
      opacity: 0;
      transform: translateY(8px);
    }
    to {
      opacity: 1;
      transform: none;
    }
  }

  @media (max-width: 720px) {
    .st-hero { padding: 22px 20px 18px; gap: 20px; }
    .st-hero-main { grid-template-columns: 1fr; gap: 22px; }
    .st-feat { justify-self: start; }
    .st-kpis { grid-template-columns: repeat(2, minmax(0, 1fr)); }
    .st-big .n { font-size: clamp(32px, 9vw, 48px); }
  }

  @media (prefers-reduced-motion: reduce) {
    .st-hero { animation: none; }
    .st-feat-art,
    .st-album-art,
    .st-bars .bar i { transition: none; }
  }
</style>
