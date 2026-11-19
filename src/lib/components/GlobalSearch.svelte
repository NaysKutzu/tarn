<script lang="ts">
  import { t as tr } from '../i18n.svelte';
  import { liveSearch, peekSearch, cacheKey } from '../search-cache';
  import type { SearchResults, ShelfItem, Track, ArtistCard, PlaylistCard } from '../types';
  import { itemToTrack } from '../cover';
  import { artistsLine, fmtTime } from '../format';
  import Cover from './Cover.svelte';
  import Hi from './Hi.svelte';
  import { Search01Icon, PlayIcon, ArrowRight01Icon, HistoryIcon, Cancel01Icon } from '../icons';

  let {
    value = $bindable(''),
    recent = [] as string[],
    onSeeAll,
    onPlay,
    onOpen,
    onContext,
    onRemoveRecent,
    onRecord,
  }: {
    value?: string;
    recent?: string[];
    onSeeAll: (q: string) => void;
    onPlay: (track: Track, queue: Track[]) => void;
    onOpen: (item: ShelfItem) => void;
    onContext?: (e: MouseEvent, item: ShelfItem) => void;
    onRemoveRecent?: (q: string) => void;
    onRecord?: (q: string) => void;
  } = $props();

  function songItem(t: Track): ShelfItem {
    return {
      kind: t.is_video ? 'video' : 'track', id: t.id, title: t.title, subtitle: null, cover: t.cover,
      artists: t.artists, artist_ids: t.artist_ids ?? [], album: t.album ?? null, album_id: t.album_id ?? null,
      duration: t.duration ?? null, playlist_id: null, color: null, browse_id: null, params: null,
      year: null, album_type: null, track_count: null, is_video: !!t.is_video,
    };
  }

  let open = $state(false);
  let suggestions = $state<string[]>([]);
  let results = $state<SearchResults | null>(null);
  let inflight = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined;
  let gen = 0;
  let root: HTMLDivElement | undefined = $state();
  let bodyEl: HTMLDivElement | undefined = $state();
  let inputEl: HTMLInputElement | undefined = $state();
  let active = $state(-1);

  const SUG_MAX = 4;
  const SONG_MAX = 4;
  const ARTIST_MAX = 3;
  const ALBUM_MAX = 3;

  let sugList = $derived(suggestions.slice(0, SUG_MAX));
  let songList = $derived((results?.songs ?? []).slice(0, SONG_MAX));
  let artistList = $derived((results?.artists ?? []).slice(0, ARTIST_MAX));
  let albumList = $derived((results?.albums ?? []).slice(0, ALBUM_MAX));
  let top = $derived(results?.top ?? null);
  let recentList = $derived(recent.slice(0, 8));
  let showRecent = $derived(open && !value.trim() && recentList.length > 0);
  let busy = $derived(inflight && !!value.trim());

  type Hit =
    | { kind: 'recent'; q: string }
    | { kind: 'sug'; q: string }
    | { kind: 'top' }
    | { kind: 'song'; i: number }
    | { kind: 'artist'; i: number }
    | { kind: 'album'; i: number }
    | { kind: 'all' };

  let hits = $derived.by((): Hit[] => {
    const out: Hit[] = [];
    if (!value.trim()) {
      for (const q of recentList) out.push({ kind: 'recent', q });
      return out;
    }
    for (const q of sugList) out.push({ kind: 'sug', q });
    if (top) out.push({ kind: 'top' });
    for (let i = 0; i < songList.length; i++) out.push({ kind: 'song', i });
    for (let i = 0; i < artistList.length; i++) out.push({ kind: 'artist', i });
    for (let i = 0; i < albumList.length; i++) out.push({ kind: 'album', i });
    if (out.length) out.push({ kind: 'all' });
    return out;
  });

  let hasDrop = $derived(open && (hits.length > 0 || busy));

  function apply(hit: { suggestions: string[]; results: SearchResults | null }) {
    suggestions = hit.suggestions ?? [];
    results = hit.results;
    active = -1;
  }

  function query(q: string) {
    value = q;
    clearTimeout(timer);
    const t = q.trim();
    if (!t) {
      suggestions = [];
      results = null;
      inflight = false;
      active = -1;
      return;
    }
    const cached = peekSearch(t);
    if (cached) apply(cached);
    const my = ++gen;
    inflight = !cached;
    timer = setTimeout(() => {
      liveSearch(t)
        .then((hit) => {
          if (my !== gen || cacheKey(value) !== cacheKey(t)) return;
          apply(hit);
        })
        .catch(() => {})
        .finally(() => {
          if (my === gen) inflight = false;
        });
    }, 180);
  }

  function seeAll(q?: string) {
    const qq = (q ?? value).trim();
    if (!qq) return;
    value = qq;
    open = false;
    onRecord?.(qq);
    onSeeAll(qq);
  }

  function runInPlace(q: string) {
    const qq = q.trim();
    if (!qq) return;
    open = true;
    onRecord?.(qq);
    query(qq);
  }

  function playSong(t: Track, ev?: Event) {
    ev?.stopPropagation();
    ev?.preventDefault();
    const queue = results?.songs?.length ? results.songs : [t];
    if (value.trim()) onRecord?.(value.trim());
    onPlay(t, queue);
    open = false;
  }

  function openItem(item: ShelfItem, ev?: Event) {
    ev?.stopPropagation();
    ev?.preventDefault();
    open = false;
    if (value.trim()) onRecord?.(value.trim());
    if (item.kind === 'track' || item.kind === 'video') {
      playSong(itemToTrack(item), ev);
      return;
    }
    onOpen(item);
  }

  function asArtist(a: ArtistCard): ShelfItem {
    return {
      kind: 'artist', id: a.id, title: a.name, subtitle: null, cover: a.cover,
      artists: [], artist_ids: [a.id], album: null, album_id: null, duration: null,
      playlist_id: null, color: null, browse_id: a.id, params: null, year: null,
      album_type: null, track_count: null, is_video: false,
    };
  }
  function asAlbum(a: PlaylistCard): ShelfItem {
    return {
      kind: 'album', id: a.id, title: a.name, subtitle: a.subtitle ?? null, cover: a.cover,
      artists: [], artist_ids: [], album: a.name, album_id: a.id, duration: null,
      playlist_id: null, color: null, browse_id: a.id, params: null, year: a.year ?? null,
      album_type: a.album_type ?? null, track_count: null, is_video: false,
    };
  }

  function kindLabel(k: string | undefined): string {
    switch (k) {
      case 'track': return 'Song';
      case 'video': return 'Video';
      case 'artist': return 'Artist';
      case 'album': return 'Album';
      case 'playlist': return 'Playlist';
      default: return k ? k[0].toUpperCase() + k.slice(1) : '';
    }
  }

  function activate(h: Hit) {
    switch (h.kind) {
      case 'recent':
      case 'sug': runInPlace(h.q); break;
      case 'top': if (top) openItem(top); break;
      case 'song': playSong(songList[h.i]); break;
      case 'artist': open = false; if (value.trim()) onRecord?.(value.trim()); onOpen(asArtist(artistList[h.i])); break;
      case 'album': open = false; if (value.trim()) onRecord?.(value.trim()); onOpen(asAlbum(albumList[h.i])); break;
      case 'all': seeAll(); break;
    }
  }

  function dropRecent(q: string, ev: Event) {
    ev.stopPropagation();
    ev.preventDefault();
    onRemoveRecent?.(q);
  }

  function clearValue(ev: Event) {
    ev.stopPropagation();
    value = '';
    suggestions = [];
    results = null;
    active = -1;
    inputEl?.focus();
  }

  function move(delta: number) {
    if (!hits.length) return;
    const n = hits.length;
    active = active < 0 ? (delta > 0 ? 0 : n - 1) : (active + delta + n) % n;
    queueMicrotask(() => {
      const el = bodyEl?.querySelector<HTMLElement>(`[data-hit="${active}"]`);
      el?.scrollIntoView({ block: 'nearest' });
    });
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'ArrowDown') {
      if (hasDrop) {
        e.preventDefault();
        move(1);
      }
    } else if (e.key === 'ArrowUp') {
      if (hasDrop) {
        e.preventDefault();
        move(-1);
      }
    } else if (e.key === 'Enter') {
      e.preventDefault();
      if (hasDrop && active >= 0 && hits[active]) activate(hits[active]);
      else seeAll();
    } else if (e.key === 'Escape') {
      if (value) {
        clearValue(e);
      } else {
        open = false;
        inputEl?.blur();
      }
    }
  }

  function isOn(kind: Hit['kind'], i?: number, q?: string): boolean {
    if (active < 0) return false;
    const h = hits[active];
    if (!h || h.kind !== kind) return false;
    if (kind === 'song' || kind === 'artist' || kind === 'album') {
      return (h as { i: number }).i === i;
    }
    if (kind === 'recent' || kind === 'sug') {
      return (h as { q: string }).q === q;
    }
    return true;
  }

  function hitIndex(kind: Hit['kind'], i?: number, q?: string): number {
    return hits.findIndex((h) => {
      if (h.kind !== kind) return false;
      if (kind === 'song' || kind === 'artist' || kind === 'album') {
        return (h as { i: number }).i === i;
      }
      if (kind === 'recent' || kind === 'sug') {
        return (h as { q: string }).q === q;
      }
      return true;
    });
  }

  $effect(() => {
    if (!open) return;
    const onDoc = (e: PointerEvent) => {
      if (root && !root.contains(e.target as Node)) open = false;
    };
    document.addEventListener('pointerdown', onDoc, true);
    return () => document.removeEventListener('pointerdown', onDoc, true);
  });
</script>

<div class="gs" class:open class:has-drop={hasDrop} bind:this={root}>
  <div class="gs-field">
    <span class="gs-ico" aria-hidden="true"><Hi icon={Search01Icon} size={15} /></span>
    <input
      id="q"
      bind:this={inputEl}
      placeholder={tr('Search')}
      role="combobox"
      aria-autocomplete="list"
      aria-expanded={hasDrop}
      aria-controls="gs-list"
      aria-activedescendant={active >= 0 ? `gs-hit-${active}` : undefined}
      autocomplete="off"
      spellcheck="false"
      bind:value
      oninput={(e) => query(e.currentTarget.value)}
      onfocus={() => (open = true)}
      onkeydown={onKey}
    />
    {#if busy}
      <span class="gs-spin" aria-hidden="true"></span>
    {:else if value}
      <button class="gs-clear" type="button" title={tr('Clear')} aria-label={tr('Clear')} onclick={clearValue}>
        <Hi icon={Cancel01Icon} size={14} />
      </button>
    {:else}
      <kbd class="gs-key">/</kbd>
    {/if}
  </div>

  {#if hasDrop}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="gs-drop"
      id="gs-list"
      role="listbox"
      tabindex="-1"
      onmousedown={(e) => e.preventDefault()}
    >
      <div class="gs-body" bind:this={bodyEl}>
        {#if showRecent}
          <div class="gs-label">{tr('Recent searches')}</div>
          {#each recentList as s}
            {@const idx = hitIndex('recent', undefined, s)}
            <div
              class="gs-recent"
              class:on={isOn('recent', undefined, s)}
              role="option"
              id="gs-hit-{idx}"
              data-hit={idx}
              aria-selected={isOn('recent', undefined, s)}
            >
              <button class="gs-text" type="button" onclick={() => runInPlace(s)}>
                <span class="gs-row-ico"><Hi icon={HistoryIcon} size={15} /></span>
                <span class="gs-t">{s}</span>
              </button>
              <button class="gs-x" type="button" title={tr('Remove')} aria-label={tr('Remove')} onclick={(e) => dropRecent(s, e)}>
                <Hi icon={Cancel01Icon} size={13} />
              </button>
            </div>
          {/each}
        {:else}
          {#if sugList.length}
            <div class="gs-sec">
              {#each sugList as s}
                {@const idx = hitIndex('sug', undefined, s)}
                <button
                  class="gs-text"
                  class:on={isOn('sug', undefined, s)}
                  type="button"
                  role="option"
                  id="gs-hit-{idx}"
                  data-hit={idx}
                  aria-selected={isOn('sug', undefined, s)}
                  onclick={() => runInPlace(s)}
                >
                  <span class="gs-row-ico"><Hi icon={Search01Icon} size={15} /></span>
                  <span class="gs-t">{s}</span>
                </button>
              {/each}
            </div>
          {/if}

          {#if top}
            {@const idx = hitIndex('top')}
            <div class="gs-label">{tr('Top result')}</div>
            <button
              class="gs-top"
              class:on={isOn('top')}
              class:artist={top.kind === 'artist'}
              type="button"
              role="option"
              id="gs-hit-{idx}"
              data-hit={idx}
              aria-selected={isOn('top')}
              onclick={(e) => openItem(top, e)}
              oncontextmenu={(e) => onContext?.(e, top)}
            >
              <span class="gs-cov top">
                <Cover src={top.cover} size={72} round={top.kind === 'artist'} alt="" />
                <span class="play-ov"><Hi icon={PlayIcon} size={24} /></span>
              </span>
              <span class="gs-meta">
                <span class="gs-t">{top.title}</span>
                <span class="gs-s">{kindLabel(top.kind)}{top.subtitle || artistsLine(top.artists) ? ` · ${top.subtitle || artistsLine(top.artists)}` : ''}</span>
              </span>
            </button>
          {/if}

          {#if songList.length}
            <div class="gs-label">{tr('Songs')}</div>
            {#each songList as t, i}
              {@const idx = hitIndex('song', i)}
              <button
                class="gs-row"
                class:on={isOn('song', i)}
                type="button"
                role="option"
                id="gs-hit-{idx}"
                data-hit={idx}
                aria-selected={isOn('song', i)}
                onclick={(e) => playSong(t, e)}
                oncontextmenu={(e) => onContext?.(e, songItem(t))}
              >
                <span class="gs-cov">
                  <Cover src={t.cover} size={40} alt="" />
                  <span class="play-ov"><Hi icon={PlayIcon} size={16} /></span>
                </span>
                <span class="gs-meta">
                  <span class="gs-t">{t.title}</span>
                  <span class="gs-s">{artistsLine(t.artists)}</span>
                </span>
                {#if t.duration}
                  <span class="gs-dur">{fmtTime(t.duration)}</span>
                {/if}
              </button>
            {/each}
          {/if}

          {#if artistList.length}
            <div class="gs-label">{tr('Artists')}</div>
            {#each artistList as a, i}
              {@const idx = hitIndex('artist', i)}
              <button
                class="gs-row"
                class:on={isOn('artist', i)}
                type="button"
                role="option"
                id="gs-hit-{idx}"
                data-hit={idx}
                aria-selected={isOn('artist', i)}
                onclick={() => { open = false; if (value.trim()) onRecord?.(value.trim()); onOpen(asArtist(a)); }}
                oncontextmenu={(e) => onContext?.(e, asArtist(a))}
              >
                <span class="gs-cov round">
                  <Cover src={a.cover} size={40} round alt="" />
                </span>
                <span class="gs-meta">
                  <span class="gs-t">{a.name}</span>
                  <span class="gs-s">{tr('Artist')}</span>
                </span>
              </button>
            {/each}
          {/if}

          {#if albumList.length}
            <div class="gs-label">{tr('Albums')}</div>
            {#each albumList as a, i}
              {@const idx = hitIndex('album', i)}
              <button
                class="gs-row"
                class:on={isOn('album', i)}
                type="button"
                role="option"
                id="gs-hit-{idx}"
                data-hit={idx}
                aria-selected={isOn('album', i)}
                onclick={() => { open = false; if (value.trim()) onRecord?.(value.trim()); onOpen(asAlbum(a)); }}
                oncontextmenu={(e) => onContext?.(e, asAlbum(a))}
              >
                <span class="gs-cov">
                  <Cover src={a.cover} size={40} alt="" />
                </span>
                <span class="gs-meta">
                  <span class="gs-t">{a.name}</span>
                  <span class="gs-s">{a.subtitle || a.album_type || 'Album'}{a.year ? ` · ${a.year}` : ''}</span>
                </span>
              </button>
            {/each}
          {/if}

          {#if !busy && !hits.length}
            <div class="gs-empty">{tr("We couldn't find that.")}</div>
          {/if}
        {/if}
      </div>

      {#if !showRecent && value.trim() && (top || songList.length || sugList.length)}
        <button
          class="gs-all"
          class:on={isOn('all')}
          type="button"
          role="option"
          id="gs-hit-{hitIndex('all')}"
          data-hit={hitIndex('all')}
          aria-selected={isOn('all')}
          onclick={() => seeAll()}
        >
          {tr('See all results')} <Hi icon={ArrowRight01Icon} size={15} />
        </button>
      {/if}
    </div>
  {/if}
</div>
