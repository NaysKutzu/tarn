<script lang="ts">
  import { t as tr } from '../i18n.svelte';
  import { api } from '../api';
  import { parseImport } from '../playlistIo';

  let {
    onClose,
    onDone,
  }: {
    onClose: () => void;
    /** Playlist was created (at least partly): refresh the library. */
    onDone: (name: string, added: number, total: number) => void;
  } = $props();

  let name = $state('');
  let text = $state('');
  let phase = $state<'form' | 'running' | 'done'>('form');
  let done = $state(0);
  let added = $state(0);
  let missing = $state<string[]>([]);
  let error = $state('');
  let cancelled = false;

  let rows = $derived(parseImport(text));

  async function readFile(e: Event) {
    const f = (e.currentTarget as HTMLInputElement).files?.[0];
    if (!f) return;
    text = await f.text();
    if (!name.trim()) name = f.name.replace(/\.[^.]+$/, '');
  }

  async function start() {
    const list = rows;
    const title = name.trim();
    if (!title || !list.length) return;
    error = '';
    phase = 'running';
    done = 0; added = 0; missing = []; cancelled = false;
    let pid = '';
    try {
      pid = await api.playlistCreate(title);
    } catch (e) {
      error = `Couldn't create the playlist: ${e}`;
      phase = 'form';
      return;
    }
    for (const r of list) {
      if (cancelled) break;
      try {
        let vid = r.id;
        if (!vid) {
          const hits = await api.searchTracks(`${r.artist} ${r.title}`.trim());
          vid = hits.find((t) => t.id)?.id;
        }
        if (vid) {
          await api.playlistAdd(pid, vid);
          added++;
        } else {
          missing = [...missing, `${r.artist ? r.artist + ' - ' : ''}${r.title}`];
        }
      } catch {
        missing = [...missing, `${r.artist ? r.artist + ' - ' : ''}${r.title}`];
      }
      done++;
    }
    phase = 'done';
    onDone(title, added, list.length);
  }

  function stop() { cancelled = true; }
  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape' && phase !== 'running') onClose();
  }
</script>

<svelte:window onkeydown={onKey} />

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="imp-scrim" onmousedown={(e) => { if (e.target === e.currentTarget && phase !== 'running') onClose(); }}>
  <div class="imp" role="dialog" aria-modal="true" aria-label={tr('Import playlist')}>
    <h2>{tr('Import playlist')}</h2>

    {#if phase === 'form'}
      <p class="hint">
        {tr('Paste a list (one song per line,')} <code>{tr('Artist - Title')}</code>) or a CSV, or pick a file.
        CSV exports from Tarn or Exportify (Spotify) work as they are. Songs are matched by searching YouTube Music.
      </p>
      <input class="in" type="text" placeholder={tr('Playlist name')} bind:value={name} />
      <textarea class="in ta" rows="9" placeholder={'Imran Khan - Imaginary\nTussin - Uber'} bind:value={text}></textarea>
      <div class="row">
        <label class="file">
          <input type="file" accept=".csv,.txt,text/csv,text/plain" onchange={readFile} />
        </label>
        <span class="count">{rows.length ? `${rows.length} songs found` : ''}</span>
      </div>
      {#if error}<div class="err">{error}</div>{/if}
      <div class="acts">
        <button class="btn ghost" type="button" onclick={onClose}>{tr('Cancel')}</button>
        <button class="btn" type="button" disabled={!name.trim() || !rows.length} onclick={start}>
          Import {rows.length || ''} songs
        </button>
      </div>
    {:else if phase === 'running'}
      <p class="hint">{tr('Adding songs… {a} / {b}', { a: done, b: rows.length })}</p>
      <div class="bar"><div class="fill" style="width:{rows.length ? (done / rows.length) * 100 : 0}%"></div></div>
      <div class="acts"><button class="btn ghost" type="button" onclick={stop}>{tr('Stop')}</button></div>
    {:else}
      <p class="hint">{tr('Added {a} of {b} songs to “{name}”.', { a: added, b: rows.length, name: name.trim() })}</p>
      {#if missing.length}
        <details>
          <summary>{tr('{n} not found', { n: missing.length })}</summary>
          <ul class="miss">{#each missing.slice(0, 200) as m}<li>{m}</li>{/each}</ul>
        </details>
      {/if}
      <div class="acts"><button class="btn" type="button" onclick={onClose}>{tr('Done')}</button></div>
    {/if}
  </div>
</div>

<style>
  .imp-scrim {
    position: fixed; inset: 0; z-index: 200;
    display: grid; place-items: center;
    background: rgba(0, 0, 0, 0.55);
  }
  .imp {
    width: min(520px, calc(100vw - 32px));
    max-height: calc(100vh - 48px);
    overflow: auto;
    padding: 22px;
    border-radius: 14px;
    background: var(--bg-elev, #1a1a1a);
    color: var(--text);
    box-shadow: 0 20px 60px rgba(0, 0, 0, 0.5);
  }
  h2 { margin: 0 0 10px; font-size: 18px; }
  .hint { margin: 0 0 12px; font-size: 13px; color: var(--text-2); line-height: 1.5; }
  code { font-size: 12px; }
  .in {
    width: 100%; box-sizing: border-box; margin-bottom: 10px;
    padding: 9px 12px; font: inherit; font-size: 13px;
    color: var(--text); background: var(--bg-hover);
    border: 1px solid transparent; border-radius: 8px; outline: none;
  }
  .in:focus { border-color: rgba(var(--accent-rgb), 0.6); }
  .ta { resize: vertical; font-family: inherit; }
  .row { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin-bottom: 14px; }
  .file { font-size: 12px; color: var(--text-2); }
  .count { font-size: 12px; color: var(--text-2); }
  .err { margin-bottom: 10px; font-size: 12px; color: #ff6b6b; }
  .acts { display: flex; justify-content: flex-end; gap: 8px; }
  .bar { height: 6px; border-radius: 3px; background: var(--bg-hover); overflow: hidden; margin-bottom: 14px; }
  .fill { height: 100%; background: rgb(var(--accent-rgb)); transition: width 0.15s; }
  .miss { margin: 8px 0 0; padding-left: 18px; max-height: 180px; overflow: auto; font-size: 12px; color: var(--text-2); }
  details { margin-bottom: 14px; font-size: 13px; }
</style>
