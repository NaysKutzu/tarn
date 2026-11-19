<script lang="ts">
  import type { Track } from '../types';
  import { api } from '../api';
  import TrackTable from './TrackTable.svelte';
  import Dropdown from './Dropdown.svelte';
  import Checkbox from './Checkbox.svelte';
  import Hi from './Hi.svelte';
  import { PlayIcon, ShuffleIcon, Delete02Icon, PencilEdit01Icon, Add01Icon } from '../icons';
  import {
    DEFAULT_RULES, describe, evaluate, loadSmart, newId, presets, saveSmart,
    type SmartPlaylist, type SmartRules, type SmartSort, type SmartSource,
  } from '../smart';
  import type { Listen } from '../stats';

  let {
    liked,
    currentId = null,
    playing = false,
    statsEnabled,
    onPlay,
    onContext,
    onArtist,
    onAlbum,
  }: {
    liked: Track[];
    currentId?: string | null;
    playing?: boolean;
    statsEnabled: boolean;
    onPlay: (tracks: Track[], index: number, shuffle?: boolean) => void;
    onContext: (e: MouseEvent, t: Track) => void;
    onArtist: (id: string, name: string) => void;
    onAlbum: (id: string) => void;
  } = $props();

  // Plain-text labels; the i18n PR can wrap these in tr() once both land.
  const tr = (s: string, v?: Record<string, string | number>) =>
    v ? Object.entries(v).reduce((o, [k, x]) => o.split(`{${k}}`).join(String(x)), s) : s;

  const initial = loadSmart();
  let lists = $state<SmartPlaylist[]>(initial);
  let selId = $state<string | null>(initial[0]?.id ?? null);
  let editing = $state<SmartPlaylist | null>(null);
  let moreOpen = $state(false);

  let history = $state<Track[]>([]);
  let downloads = $state<Track[]>([]);
  let listens = $state<Listen[]>([]);
  let bpms = $state<Record<string, number>>({});
  let loaded = $state(false);

  async function loadPool() {
    const [h, d, l] = await Promise.all([
      api.history().catch(() => [] as Track[]),
      api.offlineTracks().catch(() => [] as Track[]),
      api.getListens(0).catch(() => [] as Listen[]),
    ]);
    history = h;
    downloads = d;
    listens = l;
    const ids = new Set<string>();
    for (const t of [...liked, ...h, ...d]) ids.add(t.id);
    for (const x of l) ids.add(x.track.id);
    bpms = await api.trackBpms([...ids]).catch(() => ({}));
    loaded = true;
  }
  $effect(() => { void liked; void loadPool(); });

  const sel = $derived(lists.find((p) => p.id === selId) ?? null);
  // Seed so "random" stays put until the page is reopened.
  const seed = Math.random() * 1e9;
  function rng() {
    let s = seed;
    return () => ((s = (s * 16807) % 2147483647) / 2147483647);
  }
  function run(r: SmartRules): Track[] {
    return evaluate(r, { liked, history, downloads }, { listens, bpms, random: rng() });
  }
  const tracks = $derived(sel && loaded ? run(sel.rules) : []);
  const preview = $derived(editing && loaded ? run(editing.rules) : []);
  const bpmKnown = $derived(Object.keys(bpms).length);
  const needBpmHint = $derived(
    !!editing && (editing.rules.bpmMin != null || editing.rules.bpmMax != null) && bpmKnown === 0,
  );

  function persist() { saveSmart(lists); }

  function startNew(base?: Omit<SmartPlaylist, 'id'>) {
    moreOpen = !!(base?.rules.artist || base?.rules.minMinutes != null || base?.rules.maxMinutes != null);
    editing = {
      id: newId(),
      name: base?.name ?? tr('New playlist'),
      rules: structuredClone($state.snapshot(base?.rules ?? DEFAULT_RULES)) as SmartRules,
    };
  }
  function edit(p: SmartPlaylist) {
    moreOpen = !!(p.rules.artist || p.rules.minMinutes != null || p.rules.maxMinutes != null);
    editing = structuredClone($state.snapshot(p)) as SmartPlaylist;
  }
  function save() {
    if (!editing) return;
    const p = { ...editing, name: editing.name.trim() || tr('Smart playlist') };
    const i = lists.findIndex((x) => x.id === p.id);
    if (i >= 0) lists[i] = p; else lists.push(p);
    selId = p.id;
    editing = null;
    persist();
  }
  let confirmDel = $state<string | null>(null);
  function remove(p: SmartPlaylist) {
    if (confirmDel !== p.id) {
      confirmDel = p.id;
      setTimeout(() => { if (confirmDel === p.id) confirmDel = null; }, 3000);
      return;
    }
    lists = lists.filter((x) => x.id !== p.id);
    if (selId === p.id) selId = lists[0]?.id ?? null;
    confirmDel = null;
    persist();
  }

  const SOURCES: { id: SmartSource; label: string }[] = [
    { id: 'liked', label: 'Liked songs' },
    { id: 'history', label: 'Played before' },
    { id: 'downloads', label: 'Downloads' },
  ];
  const SORTS: { value: SmartSort; label: string }[] = [
    { value: 'plays', label: 'Most played' },
    { value: 'recent', label: 'Recently played' },
    { value: 'bpm_asc', label: 'BPM, slow to fast' },
    { value: 'bpm_desc', label: 'BPM, fast to slow' },
    { value: 'title', label: 'Title' },
    { value: 'random', label: 'Random' },
  ];
  const DAYS = [
    { value: '7', label: 'Last 7 days' },
    { value: '30', label: 'Last 30 days' },
    { value: '90', label: 'Last 90 days' },
    { value: '365', label: 'Last year' },
    { value: '0', label: 'All time' },
  ];

  function toggleSource(id: SmartSource, on: boolean) {
    if (!editing) return;
    const s = new Set(editing.rules.sources);
    if (on) s.add(id); else s.delete(id);
    editing.rules.sources = SOURCES.map((x) => x.id).filter((x) => s.has(x));
  }
  const num = (v: string): number | null => {
    const n = Number(v);
    return v.trim() === '' || !Number.isFinite(n) ? null : n;
  };
  const int0 = (v: string): number => Math.max(0, Math.round(Number(v) || 0));
</script>

<div class="smart">
  <div class="sp-head">
    <h1 class="page-title">{tr('Smart playlists')}</h1>
    <button class="btn" type="button" onclick={() => startNew()}>
      <Hi icon={Add01Icon} size={18} />
      {lists.length ? tr('New playlist') : tr('Build your own')}
    </button>
  </div>
  <p class="t-meta sp-sub">
    {tr('Playlists that fill themselves from your likes, downloads, and listening history. They refresh every time you open them.')}
  </p>

  {#if editing}
    <div class="sp-edit">
      <label class="sp-f wide"><span>{tr('Name')}</span>
        <input type="text" bind:value={editing.name} maxlength="80" />
      </label>

      <div class="sp-sec wide">
        <h3 class="sp-sec-t">{tr('Include songs from')}</h3>
        <div class="sp-row">
          {#each SOURCES as s (s.id)}
            <label class="sp-chk"><Checkbox checked={editing.rules.sources.includes(s.id)} label={s.label} onchange={(v) => toggleSource(s.id, v)} /> {s.label}</label>
          {/each}
        </div>
      </div>

      <div class="sp-sec wide">
        <h3 class="sp-sec-t">{tr('Listening')}</h3>
        <div class="sp-grid">
          <label class="sp-f"><span>{tr('Minimum plays')}</span>
            <input type="number" min="0" value={editing.rules.minPlays} oninput={(e) => editing && (editing.rules.minPlays = int0(e.currentTarget.value))} />
          </label>
          <div class="sp-f"><span>{tr('Counted over')}</span>
            <Dropdown align="left" value={String(editing.rules.playsDays)} options={DAYS} onchange={(v) => editing && (editing.rules.playsDays = int0(v))} />
          </div>
          <label class="sp-f"><span>{tr('Quiet for at least (days)')}</span>
            <input type="number" min="0" value={editing.rules.notPlayedDays} oninput={(e) => editing && (editing.rules.notPlayedDays = int0(e.currentTarget.value))} />
          </label>
          <label class="sp-chk sp-f"><Checkbox bind:checked={editing.rules.neverPlayed} label={tr('Only never played')} /> {tr('Only never played')}</label>
        </div>
      </div>

      <div class="sp-sec wide">
        <h3 class="sp-sec-t">{tr('Tempo')}</h3>
        <div class="sp-grid">
          <label class="sp-f"><span>{tr('BPM from')}</span>
            <input type="number" min="0" max="300" placeholder={tr('Any')} value={editing.rules.bpmMin ?? ''} oninput={(e) => editing && (editing.rules.bpmMin = num(e.currentTarget.value))} />
          </label>
          <label class="sp-f"><span>{tr('BPM to')}</span>
            <input type="number" min="0" max="300" placeholder={tr('Any')} value={editing.rules.bpmMax ?? ''} oninput={(e) => editing && (editing.rules.bpmMax = num(e.currentTarget.value))} />
          </label>
          <label class="sp-chk sp-f"><Checkbox bind:checked={editing.rules.bpmFold} label={tr('Also match half / double time')} /> {tr('Also match half / double time')}</label>
        </div>
        {#if needBpmHint}
          <p class="t-meta sp-hint">{tr('No tempo data yet. Play some songs with BPM matching, loudness normalisation, or Auto-DJ on, and Tarn measures them as they play.')}</p>
        {:else if loaded && bpmKnown > 0}
          <p class="t-meta sp-hint">{tr('Tempo known for {n} songs.', { n: bpmKnown })}</p>
        {/if}
      </div>

      <details class="sp-more wide" bind:open={moreOpen}>
        <summary>{tr('More filters')}</summary>
        <div class="sp-grid">
          <label class="sp-f"><span>{tr('Artist contains')}</span>
            <input type="text" bind:value={editing.rules.artist} placeholder={tr('Any artist')} />
          </label>
          <label class="sp-f"><span>{tr('Min length (min)')}</span>
            <input type="number" min="0" placeholder={tr('Any')} value={editing.rules.minMinutes ?? ''} oninput={(e) => editing && (editing.rules.minMinutes = num(e.currentTarget.value))} />
          </label>
          <label class="sp-f"><span>{tr('Max length (min)')}</span>
            <input type="number" min="0" placeholder={tr('Any')} value={editing.rules.maxMinutes ?? ''} oninput={(e) => editing && (editing.rules.maxMinutes = num(e.currentTarget.value))} />
          </label>
        </div>
      </details>

      <div class="sp-sec wide">
        <h3 class="sp-sec-t">{tr('Result')}</h3>
        <div class="sp-grid">
          <div class="sp-f"><span>{tr('Order')}</span>
            <Dropdown align="left" value={editing.rules.sort} options={SORTS} onchange={(v) => editing && (editing.rules.sort = v as SmartSort)} />
          </div>
          <label class="sp-f"><span>{tr('Limit (0 = all)')}</span>
            <input type="number" min="0" value={editing.rules.limit} oninput={(e) => editing && (editing.rules.limit = int0(e.currentTarget.value))} />
          </label>
        </div>
      </div>

      <div class="sp-actions wide">
        <span class="t-meta">{loaded ? tr('{n} songs match', { n: preview.length }) : tr('Loading…')}</span>
        <span style="flex:1"></span>
        <button class="btn ghost" type="button" onclick={() => (editing = null)}>{tr('Cancel')}</button>
        <button class="btn" type="button" onclick={save}>{tr('Save playlist')}</button>
      </div>
      {#if !statsEnabled}
        <p class="t-meta wide sp-hint">{tr('Listening stats are off, so play-count filters only see older history.')}</p>
      {/if}
    </div>
  {/if}

  {#if !lists.length && !editing}
    <p class="sp-pick">{tr('Pick a starter')}</p>
    <div class="sp-presets">
      {#each presets() as p (p.name)}
        <button type="button" class="sp-card" onclick={() => startNew(p)}>
          <b>{tr(p.name)}</b>
          <span class="t-meta">{tr(p.blurb)}</span>
        </button>
      {/each}
    </div>
  {:else if lists.length}
    <div class="sp-tabs" role="tablist">
      {#each lists as p (p.id)}
        <button type="button" role="tab" aria-selected={selId === p.id} class:on={selId === p.id} onclick={() => (selId = p.id)}>{p.name}</button>
      {/each}
    </div>
    {#if sel}
      <div class="sp-bar">
        <div>
          <h2 class="shelf-title" style="margin:0">{sel.name}</h2>
          <div class="t-meta">{describe(sel.rules, tr)} · {tr('{n} songs', { n: tracks.length })}</div>
        </div>
        <span style="flex:1"></span>
        <button class="btn" type="button" disabled={!tracks.length} onclick={() => onPlay(tracks, 0)}><Hi icon={PlayIcon} size={18} /> {tr('Play')}</button>
        <button class="btn ghost" type="button" disabled={!tracks.length} onclick={() => onPlay(tracks, Math.floor(Math.random() * tracks.length), true)}><Hi icon={ShuffleIcon} size={18} /> {tr('Shuffle')}</button>
        <button class="btn ghost" type="button" title={tr('Edit playlist')} onclick={() => edit(sel)}><Hi icon={PencilEdit01Icon} size={18} /></button>
        <button class="btn ghost" type="button" title={confirmDel === sel.id ? tr('Click again to delete') : tr('Delete')} class:danger={confirmDel === sel.id} onclick={() => remove(sel)}><Hi icon={Delete02Icon} size={18} /></button>
      </div>
      {#if !loaded}
        <p class="t-meta">{tr('Loading…')}</p>
      {:else if tracks.length}
        <TrackTable {tracks} {currentId} {playing}
          onPlay={(_t, i) => onPlay(tracks, i)}
          {onArtist} {onAlbum}
          onContext={(e, t) => onContext(e, t)} />
      {:else}
        <p class="t-meta">{tr('Nothing matches yet. Loosen the filters, or play more so Tarn has history to work with.')}</p>
      {/if}
    {/if}
  {/if}
</div>

<style>
  .smart { display: flex; flex-direction: column; gap: 14px; }
  .sp-head { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .sp-head .page-title { margin: 0; }
  .sp-sub { margin: -6px 0 0; max-width: 62ch; }
  .sp-pick { margin: 4px 0 0; font-size: 13px; font-weight: 600; color: var(--text-2); }
  .sp-edit {
    display: flex; flex-direction: column; gap: 18px;
    padding: 16px; border-radius: 14px; background: var(--bg-elev);
  }
  .sp-sec { display: flex; flex-direction: column; gap: 10px; }
  .sp-sec-t { margin: 0; font-size: 13px; font-weight: 600; color: var(--text-2); }
  .sp-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(180px, 1fr)); gap: 12px 16px; }
  .sp-f { display: flex; flex-direction: column; gap: 6px; font-size: 13px; }
  .sp-f > span { color: var(--text-3); }
  .sp-f input { padding: 8px 10px; border-radius: 8px; border: 1px solid var(--line-2); background: transparent; color: inherit; font: inherit; }
  .wide { width: 100%; }
  .sp-row { display: flex; flex-wrap: wrap; gap: 14px; }
  .sp-chk { display: flex; flex-direction: row; align-items: center; gap: 8px; cursor: pointer; align-self: end; padding-bottom: 8px; }
  .sp-actions { display: flex; align-items: center; gap: 8px; }
  .sp-hint { margin: 0; }
  .sp-more { border-top: 1px solid var(--line); padding-top: 12px; }
  .sp-more summary {
    cursor: pointer; font-size: 13px; font-weight: 550; color: var(--text-2);
    list-style: none; user-select: none;
  }
  .sp-more summary::-webkit-details-marker { display: none; }
  .sp-more[open] summary { margin-bottom: 12px; }
  .sp-presets { display: grid; grid-template-columns: repeat(auto-fill, minmax(220px, 1fr)); gap: 12px; }
  .sp-card {
    display: flex; flex-direction: column; gap: 6px; text-align: left; padding: 16px; border-radius: 14px; border: 0;
    background: var(--bg-elev); color: inherit; font: inherit; cursor: pointer;
  }
  .sp-card:hover { background: var(--bg-active); }
  .sp-card:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .sp-tabs { display: flex; flex-wrap: wrap; gap: 8px; }
  .sp-tabs button {
    padding: 6px 14px; border-radius: 999px; border: 1px solid var(--line-2);
    background: transparent; color: inherit; font: inherit; cursor: pointer;
  }
  .sp-tabs button.on { background: var(--accent); color: var(--accent-fg); border-color: transparent; }
  .sp-tabs button:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .sp-bar { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .btn.danger { color: var(--danger); }
</style>
