<script lang="ts">
  import { t as tr } from '../i18n.svelte';
  import type { Track } from '../types';
  import { fmtTime, artistsLine } from '../format';
  import Cover from './Cover.svelte';
  import Hi from './Hi.svelte';
  import { MoreHorizontalIcon, PlayIcon, Download04Icon } from '../icons';
  import { api } from '../api';
  import { off } from '../offline.svelte';

  let {
    tracks,
    currentId = null,
    playing = false,
    showAlbum = true,
    showCover = true,
    numbered = true,
    sortable = false,
    sortId = '',
    onPlay,
    onArtist,
    onAlbum,
    onContext,
    onMore,
    fallbackCover = null,
  }: {
    tracks: Track[];
    currentId?: string | null;
    playing?: boolean;
    showAlbum?: boolean;
    showCover?: boolean;
    numbered?: boolean;
    /** Clickable column headers. onPlay then gets the sorted list as 3rd arg. */
    sortable?: boolean;
    /** Changes when the list is a different one (resets the sort). */
    sortId?: string;
    onPlay: (t: Track, i: number, list: Track[]) => void;
    onArtist?: (id: string, name: string) => void;
    onAlbum?: (id: string) => void;
    onContext: (e: MouseEvent, t: Track, i: number) => void;
    onMore?: (e: MouseEvent, t: Track, i: number) => void;
    fallbackCover?: string | null;
  } = $props();

  // Prewarm resolve for rows the user can already see.
  $effect(() => {
    if (off.offline) return;
    for (const t of tracks.slice(0, 8)) {
      if (t.id) api.prefetch(t.id);
    }
  });

  // Sorting: click a header to go ascending, again for descending, again to
  // go back to the original (playlist) order.
  type SortKey = 'title' | 'artist' | 'album' | 'duration';
  let sort = $state<{ key: SortKey; dir: 1 | -1 } | null>(null);

  $effect(() => {
    void sortId;
    sort = null;
  });

  function toggleSort(key: SortKey) {
    if (!sort || sort.key !== key) sort = { key, dir: 1 };
    else if (sort.dir === 1) sort = { key, dir: -1 };
    else sort = null;
  }

  function sortValue(t: Track, key: SortKey): string | number | null {
    switch (key) {
      case 'title': return t.title || null;
      case 'artist': return t.artists?.[0] || null;
      case 'album': return t.album || null;
      case 'duration': return t.duration || null;
    }
  }

  let ordered = $derived.by(() => {
    if (!sortable || !sort) return tracks;
    const { key, dir } = sort;
    // Stable sort on a copy; tracks without a value always go last.
    return tracks
      .map((t, i) => ({ t, i, v: sortValue(t, key) }))
      .sort((a, b) => {
        if (a.v === null && b.v === null) return a.i - b.i;
        if (a.v === null) return 1;
        if (b.v === null) return -1;
        const c = typeof a.v === 'number' && typeof b.v === 'number'
          ? a.v - b.v
          : String(a.v).localeCompare(String(b.v), undefined, { sensitivity: 'base', numeric: true });
        return c ? c * dir : a.i - b.i;
      })
      .map((x) => x.t);
  });

  function arrow(key: SortKey): string {
    return sort?.key === key ? (sort.dir === 1 ? ' ▲' : ' ▼') : '';
  }

  // Render in chunks; load more as the user scrolls.
  const CHUNK = 60;
  let limit = $state(CHUNK);
  let visible = $derived(ordered.slice(0, limit));
  let sentinel = $state<HTMLElement | undefined>();

  $effect(() => {
    limit;
    if (!sentinel) return;
    const io = new IntersectionObserver(
      (es) => {
        if (es.some((e) => e.isIntersecting) && limit < ordered.length) {
          limit = Math.min(limit + CHUNK, ordered.length);
        }
      },
      { rootMargin: '800px' },
    );
    io.observe(sentinel);
    return () => io.disconnect();
  });
</script>

<table class="tbl">
  <thead>
    <tr>
      {#if numbered}<th style="width:36px">#</th>{/if}
      {#if sortable}
        <th aria-sort={sort?.key === 'title' ? (sort.dir === 1 ? 'ascending' : 'descending') : 'none'}>
          <button class="sort" type="button" onclick={() => toggleSort('title')}>{tr('Title')}{arrow('title')}</button>
          <button class="sort" type="button" onclick={() => toggleSort('artist')}>{tr('Artist')}{arrow('artist')}</button>
        </th>
        {#if showAlbum}
          <th class="al" aria-sort={sort?.key === 'album' ? (sort.dir === 1 ? 'ascending' : 'descending') : 'none'}>
            <button class="sort" type="button" onclick={() => toggleSort('album')}>{tr('Album')}{arrow('album')}</button>
          </th>
        {/if}
        <th style="width:72px">
          <button class="sort" type="button" onclick={() => toggleSort('duration')}>{tr('Time')}{arrow('duration')}</button>
        </th>
      {:else}
        <th>{tr('Title')}</th>
        {#if showAlbum}<th class="al">{tr('Album')}</th>{/if}
        <th style="width:56px"></th>
      {/if}
      <th style="width:48px"></th>
    </tr>
  </thead>
  <tbody>
    {#each visible as t, i}
      <tr
        class="trow {t.id === currentId ? 'on' : ''}"
        class:gone={off.offline && !off.playable.has(t.id)}
        ondblclick={() => onPlay(t, i, ordered)}
        oncontextmenu={(e) => onContext(e, t, i)}
        onmouseenter={() => t.id && !off.offline && api.prefetch(t.id)}
      >
        {#if numbered}
          <td class="idx">
            {#if t.id === currentId && playing}
              <span style="color:rgb(var(--accent-rgb))">●</span>
            {:else}
              {i + 1}
            {/if}
          </td>
        {/if}
        <td>
          <div style="display:flex;align-items:center;gap:12px;min-width:0">
            {#if showCover}
              <button class="cov" onclick={() => onPlay(t, i, ordered)} title={tr('Play')} type="button">
                <Cover src={t.cover || fallbackCover} size={40} alt="" />
                <span class="play-ov"><Hi icon={PlayIcon} size={16} /></span>
              </button>
            {/if}
            <div style="min-width:0">
              <div class="tn clamp one">{t.title}</div>
              <div class="ta">
                {#each t.artists as a, k}
                  {#if k > 0}<span>, </span>{/if}
                  <button
                    class="linkish"
                    type="button"
                    onclick={() => onArtist?.(t.artist_ids[k] || '', a)}
                  >{a}</button>
                {/each}
              </div>
            </div>
          </div>
        </td>
        {#if showAlbum}
          <td class="al">
            {#if t.album}
              <button
                class="linkish"
                type="button"
                onclick={() => t.album_id && onAlbum?.(t.album_id)}
              >{t.album}</button>
            {/if}
          </td>
        {/if}
        <td class="dur">
          {#if off.downloaded.has(t.id)}<span class="dl" title={tr('Downloaded')}><Hi icon={Download04Icon} size={13} /></span>{/if}
          {t.duration ? fmtTime(t.duration) : ''}
        </td>
        <td class="acts">
          <button
            class="ibtn"
            style="width:36px;height:36px;min-width:36px;min-height:36px"
            type="button"
            onclick={(e) => onMore?.(e, t, i) ?? onContext(e, t, i)}
            title={tr('More')}
          >
            <Hi icon={MoreHorizontalIcon} size={18} />
          </button>
        </td>
      </tr>
    {/each}
    {#if limit < ordered.length}
      <tr bind:this={sentinel}><td colspan="5" style="height:1px;padding:0"></td></tr>
    {/if}
  </tbody>
</table>

<style>
  .sort {
    font: inherit;
    letter-spacing: inherit;
    text-transform: inherit;
    color: inherit;
    padding: 0;
    margin-right: 14px;
    cursor: pointer;
  }
  .sort:hover { color: var(--text); }
</style>
