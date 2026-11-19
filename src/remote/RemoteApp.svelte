<script lang="ts">
  import { onMount } from 'svelte';
  import Hi from '../lib/components/Hi.svelte';
  import IconBtn from '../lib/components/IconBtn.svelte';
  import Slider from '../lib/components/Slider.svelte';
  import { fmtTime, artistsLine } from '../lib/format';
  import type { Track } from '../lib/types';
  import {
    PlayIcon,
    PauseIcon,
    PreviousIcon,
    NextIcon,
    ShuffleIcon,
    RepeatIcon,
    RepeatOne01Icon,
    FavouriteIcon,
    FavouriteFilledIcon,
    ThumbsDownIcon,
    VolumeHighIcon,
    VolumeMute01Icon,
    Search01Icon,
    Add01Icon,
    PlayListAddIcon,
    Queue02Icon,
  } from '../lib/icons';
  import {
    api,
    cmd,
    coverUrl,
    token,
    type Lyrics,
    type RemoteState,
  } from './api';

  type Tab = 's' | 'q' | 'l';

  let st = $state<RemoteState | null>(null);
  let at = $state(Date.now());
  let error = $state('');
  /** Token missing/rejected. Keep saying so, stop pretending we're still connecting. */
  let linkDead = $state(false);
  let tab = $state<Tab>('q');
  let query = $state('');
  let hits = $state<Track[]>([]);
  let searchBusy = $state(false);
  let searchMsg = $state('Find a song and play it on Tarn');
  let lyr = $state<Lyrics | null>(null);
  let lyrId = $state('');
  let seekPreview = $state<number | null>(null);
  let clock = $state(Date.now());

  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  let searchGen = 0;
  let pollIv: ReturnType<typeof setInterval> | undefined;

  function killLink(msg: string) {
    linkDead = true;
    error = msg;
    st = null;
    if (pollIv != null) {
      clearInterval(pollIv);
      pollIv = undefined;
    }
  }

  function isLinkError(msg: string) {
    return /wrong or expired|scan the qr|open this page by scanning/i.test(msg);
  }

  let pos = $derived.by(() => {
    if (!st) return 0;
    if (seekPreview != null) return seekPreview;
    void clock;
    const live = st.playing ? (Date.now() - at) / 1000 : 0;
    return Math.min(st.duration || 0, st.position + live);
  });

  let cover = $derived(coverUrl(st?.track));

  async function poll() {
    if (linkDead) return;
    try {
      st = await api<RemoteState>('/api/state');
      at = Date.now();
      error = '';
      if (st.track && st.track.id !== lyrId) {
        lyrId = st.track.id;
        lyr = null;
        if (tab === 'l') void loadLyrics();
      }
    } catch (e) {
      const msg = e instanceof Error ? e.message : 'Lost connection to Tarn';
      if (isLinkError(msg)) killLink(msg);
      else error = msg;
    }
  }

  async function run(c: string, extra?: Record<string, unknown>) {
    if (linkDead) return;
    try {
      await cmd(c, extra);
      setTimeout(poll, 120);
    } catch (e) {
      const msg = e instanceof Error ? e.message : 'Command failed';
      if (isLinkError(msg)) killLink(msg);
      else error = msg;
    }
  }

  async function runSearch(q: string) {
    const my = ++searchGen;
    q = q.trim();
    if (!q) {
      hits = [];
      searchMsg = 'Find a song and play it on Tarn';
      searchBusy = false;
      return;
    }
    searchBusy = true;
    searchMsg = 'Searching…';
    try {
      const rows = await api<Track[]>('/api/search?q=' + encodeURIComponent(q));
      if (my !== searchGen) return;
      hits = Array.isArray(rows) ? rows : [];
      searchMsg = hits.length ? '' : 'No songs found';
    } catch (e) {
      if (my !== searchGen) return;
      hits = [];
      const msg = e instanceof Error ? e.message : 'Search failed';
      if (isLinkError(msg)) killLink(msg);
      searchMsg = msg;
    } finally {
      if (my === searchGen) searchBusy = false;
    }
  }

  async function loadLyrics() {
    try {
      lyr = await api<Lyrics>('/api/lyrics');
    } catch (e) {
      lyr = { lines: [], synced: false };
      const msg = e instanceof Error ? e.message : 'Could not load lyrics';
      if (isLinkError(msg)) killLink(msg);
      else error = msg;
    }
  }

  function setTab(next: Tab) {
    tab = next;
    if (next === 'l' && !lyr) void loadLyrics();
  }

  function onQueryInput(v: string) {
    query = v;
    clearTimeout(searchTimer);
    searchTimer = setTimeout(() => runSearch(v), 280);
  }

  onMount(() => {
    if (!token) {
      killLink('Wrong link. Open this page by scanning the QR code in Tarn.');
      return;
    }
    void poll();
    pollIv = setInterval(() => {
      if (!document.hidden) void poll();
    }, 1000);
    let raf = 0;
    const tick = () => {
      clock = Date.now();
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => {
      if (pollIv != null) clearInterval(pollIv);
      cancelAnimationFrame(raf);
    };
  });

  let statusLine = $derived.by(() => {
    if (st?.track) return artistsLine(st.track.artists);
    if (st) return 'Pick something from Search';
    if (linkDead || error) return error || 'Wrong or expired link';
    return 'Connecting…';
  });

  let lyricIndex = $derived.by(() => {
    if (!lyr?.synced || !lyr.lines.length) return -1;
    const ms = pos * 1000;
    let cur = -1;
    for (let i = 0; i < lyr.lines.length; i++) {
      const s = lyr.lines[i].start_ms;
      if (s != null && s <= ms) cur = i;
    }
    return cur;
  });

  $effect(() => {
    const i = lyricIndex;
    if (tab !== 'l' || i < 0) return;
    const el = document.getElementById('ly-' + i);
    el?.scrollIntoView({ block: 'center', behavior: 'smooth' });
  });
</script>

{#if cover}
  <div class="bg" style="background-image:url('{cover}')" aria-hidden="true"></div>
{/if}
<div class="shade" aria-hidden="true"></div>

<div class="shell">
  <header class="brand">Tarn</header>

  <section class="now">
    <div class="art" style={cover ? `background-image:url('${cover}')` : ''} aria-hidden="true"></div>
    <div class="meta">
      <h1 class="title">{st?.track?.title ?? (st ? 'Nothing playing' : linkDead ? 'Link is wrong' : 'Tarn')}</h1>
      <p class="artist" class:dead={linkDead || (!!error && !st)}>{statusLine}</p>
    </div>

    <div class="seek-row">
      <span class="t">{fmtTime(pos)}</span>
      <Slider
        value={pos}
        max={st?.duration || 0}
        disabled={!st?.track || !(st.duration > 0)}
        label="Seek"
        format={fmtTime}
        oninput={(v) => (seekPreview = v)}
        onchange={(v) => {
          seekPreview = null;
          if (st) {
            st.position = v;
            at = Date.now();
          }
          void run('seek', { value: v });
        }}
        onpreview={(v) => {
          if (v == null) seekPreview = null;
        }}
      />
      <span class="t">{fmtTime(st?.duration)}</span>
    </div>

    <div class="ctrls">
      <IconBtn icon={ShuffleIcon} size={22} on={!!st?.shuffle} btnSize="lg" title="Shuffle" onclick={() => run('shuffle')} />
      <IconBtn icon={PreviousIcon} size={26} btnSize="lg" title="Previous" onclick={() => run('prev')} />
      <IconBtn
        icon={PlayIcon}
        altIcon={PauseIcon}
        showAlt={!!st?.playing}
        size={28}
        btnSize="xl"
        title={st?.playing ? 'Pause' : 'Play'}
        disabled={!st?.track}
        onclick={() => run('toggle')}
      />
      <IconBtn icon={NextIcon} size={26} btnSize="lg" title="Next" onclick={() => run('next')} />
      <IconBtn
        icon={RepeatIcon}
        altIcon={RepeatOne01Icon}
        showAlt={st?.repeat === 'one'}
        size={22}
        on={!!st && st.repeat !== 'off'}
        btnSize="lg"
        title="Repeat"
        onclick={() => run('repeat')}
      />
    </div>

    <div class="tools">
      <div class="likes">
        <IconBtn
          icon={FavouriteIcon}
          altIcon={FavouriteFilledIcon}
          showAlt={!!st?.liked}
          size={22}
          on={!!st?.liked}
          title="Like"
          disabled={!st?.track}
          onclick={() => run('like')}
        />
        <IconBtn
          icon={ThumbsDownIcon}
          size={22}
          on={!!st?.disliked}
          danger
          title="Dislike"
          disabled={!st?.track}
          onclick={() => run('dislike')}
        />
      </div>
      <div class="vol">
        <Hi icon={(st?.volume ?? 1) < 0.01 ? VolumeMute01Icon : VolumeHighIcon} size={20} />
        <Slider
          value={st?.volume ?? 1}
          max={1}
          step={0.01}
          live
          label="Volume"
          format={(v) => `${Math.round(v * 100)}%`}
          onchange={(v) => {
            if (st) st.volume = v;
            void run('volume', { value: v });
          }}
        />
      </div>
    </div>
  </section>

  <section class="panel">
    <div class="tabs" role="tablist">
      <button type="button" role="tab" class:on={tab === 'q'} aria-selected={tab === 'q'} onclick={() => setTab('q')}>
        Queue{#if (st?.queue?.length ?? 0) > 1}<span class="tab-n">{st!.queue.length - 1}</span>{/if}
      </button>
      <button type="button" role="tab" class:on={tab === 's'} aria-selected={tab === 's'} onclick={() => setTab('s')}>Search</button>
      <button type="button" role="tab" class:on={tab === 'l'} aria-selected={tab === 'l'} onclick={() => setTab('l')}>Lyrics</button>
    </div>

    {#if tab === 'q'}
      <div class="pane" role="tabpanel">
        {#if st?.queue?.length}
          <ul class="list">
            {#each st.queue as t, i}
              <li>
                <button
                  type="button"
                  class="row"
                  class:cur={i === 0}
                  onclick={() => run('play_index', { value: st!.queue_start + i })}
                >
                  {#if i === 0}
                    <span class="now-tag">Now</span>
                  {:else}
                    <span class="n">{st.queue_start + i + 1}</span>
                  {/if}
                  <div class="thumb" style={coverUrl(t) ? `background-image:url('${coverUrl(t)}')` : ''}></div>
                  <div class="tx">
                    <div class="tt">{t.title}</div>
                    <div class="sub">{artistsLine(t.artists)}</div>
                  </div>
                </button>
              </li>
            {/each}
          </ul>
        {:else}
          <div class="empty">
            <div class="empty-ico" aria-hidden="true"><Hi icon={Queue02Icon} size={28} /></div>
            <h2 class="empty-t">{st?.track ? 'Nothing up next' : 'Queue is empty'}</h2>
            <p class="empty-s">
              {st?.track
                ? 'Add songs from Search and they’ll line up after this one.'
                : 'Find something to play. It shows up here while you control Tarn from your phone.'}
            </p>
            <button type="button" class="empty-cta" onclick={() => setTab('s')}>
              <Hi icon={Search01Icon} size={18} />
              Search songs
            </button>
          </div>
        {/if}
      </div>
    {:else if tab === 's'}
      <div class="pane" role="tabpanel">
        <label class="search">
          <Hi icon={Search01Icon} size={18} />
          <input
            type="search"
            placeholder="Search songs"
            enterkeyhint="search"
            autocomplete="off"
            spellcheck="false"
            value={query}
            oninput={(e) => onQueryInput(e.currentTarget.value)}
            onkeydown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault();
                clearTimeout(searchTimer);
                void runSearch(query);
              }
            }}
          />
        </label>
        {#if hits.length}
          <ul class="list">
            {#each hits as t}
              <li>
                <button type="button" class="row" onclick={() => run('play_track', { track: t })}>
                  <div class="thumb" style={coverUrl(t) ? `background-image:url('${coverUrl(t)}')` : ''}></div>
                  <div class="tx">
                    <div class="tt">{t.title}</div>
                    <div class="sub">{artistsLine(t.artists)}</div>
                  </div>
                </button>
                <div class="act">
                  <IconBtn icon={PlayListAddIcon} size={18} title="Play next" onclick={() => run('queue_next', { track: t })} />
                  <IconBtn icon={Add01Icon} size={18} title="Add to queue" onclick={() => run('queue_add', { track: t })} />
                </div>
              </li>
            {/each}
          </ul>
        {:else}
          <div class="empty compact">
            <p class="empty-s">{searchBusy ? 'Searching…' : searchMsg}</p>
          </div>
        {/if}
      </div>
    {:else}
      <div class="pane ly" role="tabpanel">
        {#if !st?.track}
          <p class="msg">Nothing playing</p>
        {:else if !lyr}
          <p class="msg">Loading lyrics…</p>
        {:else if !lyr.lines.length}
          <p class="msg">No lyrics for this song</p>
        {:else}
          {#each lyr.lines as line, i}
            <p id="ly-{i}" class:on={!lyr.synced || i === lyricIndex} class:past={lyr.synced && i < lyricIndex}>
              {line.text || '♪'}
            </p>
          {/each}
        {/if}
      </div>
    {/if}
  </section>
</div>

{#if error}
  <div class="err" role="alert">{error}</div>
{/if}

<style>
  .bg, .shade {
    position: fixed;
    inset: 0;
    pointer-events: none;
    z-index: 0;
  }
  .bg {
    background: center / cover no-repeat;
    filter: blur(56px) saturate(1.08);
    transform: scale(1.22);
    opacity: 0.42;
  }
  .shade {
    background:
      linear-gradient(180deg, rgba(12, 11, 10, 0.35), rgba(12, 11, 10, 0.78) 48%, var(--bg) 100%);
  }
  .shell {
    position: relative;
    z-index: 1;
    max-width: 480px;
    margin: 0 auto;
    min-height: 100%;
    padding: calc(12px + var(--safe-t)) 16px calc(24px + var(--safe-b));
    display: flex;
    flex-direction: column;
    gap: 18px;
  }
  .brand {
    font-size: 15px;
    font-weight: 650;
    letter-spacing: -0.02em;
    text-align: center;
    color: var(--text);
  }
  .now {
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: 14px;
  }
  .art {
    width: min(72vw, 280px);
    aspect-ratio: 1;
    margin: 4px auto 2px;
    border-radius: 12px;
    background: var(--bg-elev) center / cover no-repeat;
    box-shadow: var(--shadow);
  }
  .meta { text-align: center; min-width: 0; padding: 0 4px; }
  .title {
    font-size: 22px;
    line-height: 28px;
    font-weight: 650;
    letter-spacing: -0.02em;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .artist {
    margin-top: 6px;
    font-size: 15px;
    line-height: 20px;
    color: var(--text-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .artist.dead {
    color: var(--danger);
    white-space: normal;
    overflow: visible;
    text-overflow: unset;
  }
  .seek-row {
    display: grid;
    grid-template-columns: 36px 1fr 36px;
    align-items: center;
    gap: 8px;
    padding: 0 2px;
  }
  .seek-row .t {
    font-size: 11px;
    line-height: 14px;
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
    text-align: center;
  }
  .ctrls {
    display: flex;
    justify-content: center;
    align-items: center;
    gap: 6px;
  }
  .tools {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 2px 2px;
  }
  .likes {
    display: flex;
    align-items: center;
    gap: 2px;
    flex: none;
  }
  .vol {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-3);
    padding: 0 4px;
  }
  .panel {
    flex: 1;
    min-height: 220px;
    display: flex;
    flex-direction: column;
    border-top: 1px solid var(--line);
    padding-top: 12px;
  }
  .tabs {
    display: grid;
    grid-template-columns: 1fr 1fr 1fr;
    gap: 0;
    margin-bottom: 10px;
  }
  .tabs button {
    padding: 10px 8px;
    font-size: 13px;
    font-weight: 600;
    color: var(--text-3);
    border-bottom: 2px solid transparent;
  }
  .tabs button.on {
    color: var(--text);
    border-bottom-color: var(--text);
  }
  .tab-n {
    display: inline-grid;
    place-items: center;
    min-width: 18px;
    height: 18px;
    margin-left: 6px;
    padding: 0 5px;
    border-radius: 9px;
    background: var(--bg-active);
    color: var(--text-2);
    font-size: 11px;
    font-weight: 650;
    font-variant-numeric: tabular-nums;
    vertical-align: 1px;
  }
  .tabs button.on .tab-n {
    background: rgba(255, 255, 255, 0.14);
    color: var(--text);
  }
  .pane {
    flex: 1;
    max-height: min(42vh, 420px);
    overflow: auto;
    -webkit-overflow-scrolling: touch;
  }
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: 10px;
    padding: 36px 20px 28px;
  }
  .empty.compact { padding: 40px 16px; gap: 0; }
  .empty-ico {
    width: 56px;
    height: 56px;
    border-radius: 16px;
    display: grid;
    place-items: center;
    color: var(--text-2);
    background: var(--bg-elev);
    border: 1px solid var(--line);
    margin-bottom: 4px;
  }
  .empty-t {
    font-size: 16px;
    line-height: 22px;
    font-weight: 650;
    letter-spacing: -0.015em;
  }
  .empty-s {
    max-width: 28ch;
    color: var(--text-3);
    font-size: 13px;
    line-height: 18px;
  }
  .empty-cta {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    min-height: 40px;
    margin-top: 8px;
    padding: 0 16px;
    border-radius: 20px;
    font-size: 14px;
    font-weight: 600;
    letter-spacing: -0.01em;
    background: var(--bg-hover);
    color: var(--text);
    border: 1px solid var(--line-2);
  }
  .empty-cta:active { background: var(--bg-active); }
  .search {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 44px;
    padding: 0 12px;
    margin-bottom: 8px;
    border-radius: var(--radius);
    background: var(--bg-elev);
    border: 1px solid var(--line);
    color: var(--text-3);
  }
  .search input {
    flex: 1;
    min-width: 0;
    height: 100%;
    outline: none;
    font-size: 15px;
    color: var(--text);
  }
  .search input::placeholder { color: var(--text-3); }
  .list { list-style: none; display: flex; flex-direction: column; }
  .list li {
    display: flex;
    align-items: center;
    gap: 4px;
    min-height: 56px;
  }
  .row {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 6px;
    border-radius: var(--radius);
    text-align: left;
  }
  .row:active, .row.cur { background: var(--bg-hover); }
  .row.cur .tt { color: var(--accent); }
  .thumb {
    width: 40px;
    height: 40px;
    border-radius: 8px;
    background: var(--bg-elev) center / cover no-repeat;
    flex: none;
  }
  .n {
    width: 22px;
    text-align: right;
    font-size: 12px;
    color: var(--text-3);
    font-variant-numeric: tabular-nums;
    flex: none;
  }
  .now-tag {
    width: 32px;
    flex: none;
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--accent);
  }
  .tx { min-width: 0; flex: 1; }
  .tt {
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .sub {
    margin-top: 2px;
    font-size: 12px;
    line-height: 16px;
    color: var(--text-3);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .act { display: flex; flex: none; }
  .act :global(.ibtn) {
    width: 36px;
    height: 36px;
    min-width: 36px;
    min-height: 36px;
  }
  .ly p {
    padding: 10px 6px;
    color: var(--text-3);
    font-size: 1.15rem;
    font-weight: 600;
    letter-spacing: -0.01em;
    line-height: 1.35;
    transition: color 160ms var(--ease);
  }
  .ly p.past { color: var(--text-3); opacity: 0.55; }
  .ly p.on { color: var(--text); opacity: 1; }
  .msg {
    color: var(--text-3);
    padding: 36px 12px;
    text-align: center;
    font-size: 14px;
  }
  .err {
    position: fixed;
    left: 16px;
    right: 16px;
    bottom: calc(16px + var(--safe-b));
    z-index: 20;
    padding: 12px 14px;
    border-radius: 12px;
    background: #3a1d19;
    color: #ffd5cf;
    box-shadow: var(--shadow);
  }
</style>
