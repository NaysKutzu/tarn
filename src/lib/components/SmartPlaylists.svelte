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

  function persist() { saveSmart(lists); }

  function startNew(base?: Omit<SmartPlaylist, 'id'>) {
    editing = {
      id: newId(),
      name: base?.name ?? tr('New smart playlist'),
      rules: structuredClone($state.snapshot(base?.rules ?? DEFAULT_RULES)) as SmartRules,
    };
  }
  function edit(p: SmartPlaylist) {
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
    <button class="btn" type="button" onclick={() => startNew()}><Hi icon={Add01Icon} size={18} /> {tr('New')}</button>
  </div>
  <p class="t-meta sp-sub">
    {tr('Rule-based lists built on this device from your liked songs, downloads and listening history. They update every time you open them.')}
    {#if loaded}{tr('Tempo known for {n} songs.', { n: bpmKnown })}{/if}
  </p>

  {#if editing}
    <div class="sp-edit">
      <label class="sp-f wide"><span>{tr('Name')}</span>
        <input type="text" bind:value={editing.name} maxlength="80" />
      </label>
      <div class="sp-f wide"><span>{tr('Songs from')}</span>
        <div class="sp-row">
          {#each SOURCES as s (s.id)}
            <label class="sp-chk"><Checkbox checked={editing.rules.sources.includes(s.id)} label={s.label} onchange={(v) => toggleSource(s.id, v)} /> {s.label}</label>
          {/each}
        </div>
      </div>
      <label class="sp-f"><span>{tr('BPM from')}</span>
        <input type="number" min="0" max="300" placeholder="-" value={editing.rules.bpmMin ?? ''} oninput={(e) => editing && (editing.rules.bpmMin = num(e.currentTarget.value))} />
      </label>
      <label class="sp-f"><span>{tr('BPM to')}</span>
        <input type="number" min="0" max="300" placeholder="-" value={editing.rules.bpmMax ?? ''} oninput={(e) => editing && (editing.rules.bpmMax = num(e.currentTarget.value))} />
      </label>
      <label class="sp-chk sp-f"><Checkbox bind:checked={editing.rules.bpmFold} label={tr('Count half / double time')} /> {tr('Count half / double time')}</label>
      <label class="sp-f"><span>{tr('Artist contains')}</span>
        <input type="text" bind:value={editing.rules.artist} placeholder={tr('Any artist')} />
      </label>
      <label class="sp-f"><span>{tr('Min plays')}</span>
        <input type="number" min="0" value={editing.rules.minPlays} oninput={(e) => editing && (editing.rules.minPlays = int0(e.currentTarget.value))} />
      </label>
      <div class="sp-f"><span>{tr('Counted over')}</span>
        <Dropdown align="left" value={String(editing.rules.playsDays)} options={DAYS} onchange={(v) => editing && (editing.rules.playsDays = int0(v))} />
      </div>
      <label class="sp-f"><span>{tr('Not played for (days)')}</span>
        <input type="number" min="0" value={editing.rules.notPlayedDays} oninput={(e) => editing && (editing.rules.notPlayedDays = int0(e.currentTarget.value))} />
      </label>
      <label class="sp-chk sp-f"><Checkbox bind:checked={editing.rules.neverPlayed} label={tr('Never played')} /> {tr('Never played')}</label>
      <label class="sp-f"><span>{tr('Min length (min)')}</span>
        <input type="number" min="0" placeholder="-" value={editing.rules.minMinutes ?? ''} oninput={(e) => editing && (editing.rules.minMinutes = num(e.currentTarget.value))} />
      </label>
      <label class="sp-f"><span>{tr('Max length (min)')}</span>
        <input type="number" min="0" placeholder="-" value={editing.rules.maxMinutes ?? ''} oninput={(e) => editing && (editing.rules.maxMinutes = num(e.currentTarget.value))} />
      </label>
      <div class="sp-f"><span>{tr('Order')}</span>
        <Dropdown align="left" value={editing.rules.sort} options={SORTS} onchange={(v) => editing && (editing.rules.sort = v as SmartSort)} />
      </div>
      <label class="sp-f"><span>{tr('Limit (0 = all)')}</span>
        <input type="number" min="0" value={editing.rules.limit} oninput={(e) => editing && (editing.rules.limit = int0(e.currentTarget.value))} />
      </label>
      <div class="sp-actions wide">
        <span class="t-meta">{loaded ? tr('{n} songs match', { n: preview.length }) : tr('Loading…')}</span>
        <span style="flex:1"></span>
        <button class="btn ghost" type="button" onclick={() => (editing = null)}>{tr('Cancel')}</button>
        <button class="btn" type="button" onclick={save}>{tr('Save')}</button>
      </div>
      {#if (editing.rules.bpmMin != null || editing.rules.bpmMax != null) && bpmKnown === 0}
        <p class="t-meta wide">{tr('No tempo data yet. Turn on BPM matching, loudness normalisation or Auto-DJ and play some songs - Tarn measures them as they play.')}</p>
      {/if}
      {#if !statsEnabled}
        <p class="t-meta wide">{tr('Listening stats are off, so play-count rules only see older history.')}</p>
      {/if}
    </div>
  {/if}

  {#if !lists.length && !editing}
    <div class="sp-presets">
      {#each presets() as p (p.name)}
        <button type="button" class="sp-card" onclick={() => startNew(p)}>
          <b>{tr(p.name)}</b>
          <span class="t-meta">{describe(p.rules, tr)}</span>
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
        <button class="btn ghost" type="button" title={tr('Edit rules')} onclick={() => edit(sel)}><Hi icon={PencilEdit01Icon} size={18} /></button>
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
        <p class="t-meta">{tr('No songs match these rules yet.')}</p>
      {/if}
    {/if}
  {/if}
</div>

<style>
  .smart { display: flex; flex-direction: column; gap: 14px; }
  .sp-head { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .sp-head .page-title { margin: 0; }
  .sp-sub { margin: -6px 0 0; max-width: 70ch; }
  .sp-edit {
    display: grid; grid-template-columns: repeat(auto-fill, minmax(180px, 1fr)); gap: 12px 16px;
    padding: 16px; border-radius: 14px; background: var(--bg-elev);
  }
  .sp-f { display: flex; flex-direction: column; gap: 6px; font-size: 13px; }
  .sp-f > span { opacity: 0.75; }
  .sp-f input { padding: 8px 10px; border-radius: 8px; border: 1px solid var(--line-2); background: transparent; color: inherit; font: inherit; }
  .wide { grid-column: 1 / -1; }
  .sp-row { display: flex; flex-wrap: wrap; gap: 14px; }
  .sp-chk { display: flex; flex-direction: row; align-items: center; gap: 8px; cursor: pointer; align-self: end; padding-bottom: 8px; }
  .sp-actions { display: flex; align-items: center; gap: 8px; }
  .sp-presets { display: grid; grid-template-columns: repeat(auto-fill, minmax(220px, 1fr)); gap: 12px; }
  .sp-card {
    display: flex; flex-direction: column; gap: 4px; text-align: left; padding: 16px; border-radius: 14px; border: 0;
    background: var(--bg-elev); color: inherit; font: inherit; cursor: pointer;
  }
  .sp-card:hover { background: var(--bg-active); }
  .sp-tabs { display: flex; flex-wrap: wrap; gap: 8px; }
  .sp-tabs button {
    padding: 6px 14px; border-radius: 999px; border: 1px solid var(--line-2);
    background: transparent; color: inherit; font: inherit; cursor: pointer;
  }
  .sp-tabs button.on { background: var(--accent); color: var(--accent-fg); border-color: transparent; }
  .sp-bar { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .btn.danger { color: var(--danger); }
</style>
