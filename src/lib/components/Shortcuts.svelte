<script lang="ts">
  import { t as tr } from '../i18n.svelte';
  import {
    ACTION_LABELS, APP_ACTIONS, APP_DEFAULTS, GLOBAL_ACTIONS,
    appCombo, globalCombo, prettyCombo, appShortcuts, type ShortcutAction,
  } from '../shortcuts';

  let {
    shortcuts,
    globalHotkeys,
    desktop,
    mac,
    onChange,
  }: {
    shortcuts: Record<string, string>;
    globalHotkeys: Record<string, string>;
    desktop: boolean;
    mac: boolean;
    onChange: (shortcuts: Record<string, string>, globalHotkeys: Record<string, string>) => void;
  } = $props();

  /** Which binding is waiting for a key press: "app:next", "global:like"… */
  let recording = $state<string | null>(null);
  const appMap = $derived(appShortcuts(shortcuts));

  function start(id: string) {
    recording = recording === id ? null : id;
  }

  function onKey(e: KeyboardEvent) {
    if (!recording) return;
    e.preventDefault();
    e.stopPropagation();
    const [kind, action] = recording.split(':');
    if (e.key === 'Escape') { recording = null; return; }
    const clear = (e.key === 'Backspace' || e.key === 'Delete') && !e.ctrlKey && !e.altKey && !e.metaKey;
    const combo = clear ? '' : kind === 'app' ? appCombo(e) : globalCombo(e);
    if (combo === null) return; // just a modifier so far
    if (kind === 'app') {
      const next: Record<string, string> = { ...shortcuts };
      // One combo, one action: take it away from whatever had it.
      if (combo) for (const [a, c] of Object.entries(appMap)) if (c === combo && a !== action) next[a] = '';
      next[action] = combo;
      // Only store differences from the defaults.
      for (const a of Object.keys(next)) if (next[a] === (APP_DEFAULTS[a] ?? '')) delete next[a];
      onChange(next, globalHotkeys);
    } else {
      const next: Record<string, string> = { ...globalHotkeys };
      if (combo) for (const [a, c] of Object.entries(next)) if (c === combo && a !== action) delete next[a];
      if (combo) next[action] = combo; else delete next[action];
      onChange(shortcuts, next);
    }
    recording = null;
  }

  function resetApp() {
    onChange({}, globalHotkeys);
  }
  function clearGlobal() {
    onChange(shortcuts, {});
  }
  const label = (a: ShortcutAction) => tr(ACTION_LABELS[a]);
</script>

<svelte:window onkeydowncapture={onKey} />

<div class="set-row set-stack">
  <span class="lb">{tr('Keyboard shortcuts')}
    <span class="set-meta">{tr('Click a shortcut, then press the keys. Backspace clears it, Esc cancels.')}</span>
  </span>
  <div class="sc-grid">
    {#each APP_ACTIONS as a}
      <span class="sc-name">{label(a)}</span>
      <button class="sc-key" class:rec={recording === `app:${a}`} class:unset={!appMap[a]} type="button" onclick={() => start(`app:${a}`)}>
        {recording === `app:${a}` ? tr('Press keys…') : prettyCombo(appMap[a], mac)}
      </button>
    {/each}
  </div>
  <div><button class="btn ghost sm" type="button" onclick={resetApp} disabled={Object.keys(shortcuts).length === 0}>{tr('Reset to defaults')}</button></div>

  {#if desktop}
    <span class="lb" style="margin-top:8px">{tr('Global hotkeys')}
      <span class="set-meta">{tr('Work even when Tarn is in the background. Pick combos other apps don\'t use.')}</span>
    </span>
    <div class="sc-grid">
      {#each GLOBAL_ACTIONS as a}
        <span class="sc-name">{label(a)}</span>
        <button class="sc-key" class:rec={recording === `global:${a}`} class:unset={!globalHotkeys[a]} type="button" onclick={() => start(`global:${a}`)}>
          {recording === `global:${a}` ? tr('Press keys…') : prettyCombo(globalHotkeys[a] ?? '', mac)}
        </button>
      {/each}
    </div>
    <div><button class="btn ghost sm" type="button" onclick={clearGlobal} disabled={Object.keys(globalHotkeys).length === 0}>{tr('Clear all')}</button></div>
  {/if}
</div>

<style>
  .sc-grid {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 6px 16px;
    align-items: center;
  }
  .sc-name { font-size: 13px; }
  .sc-key {
    min-width: 140px;
    padding: 5px 10px;
    border-radius: 8px;
    border: 1px solid var(--line, rgba(127, 127, 127, 0.3));
    background: transparent;
    color: inherit;
    font: inherit;
    font-size: 12.5px;
    cursor: pointer;
    text-align: center;
  }
  .sc-key.unset { opacity: 0.55; }
  .sc-key.rec {
    border-color: var(--accent, #7c5cff);
    color: var(--accent, #7c5cff);
  }
</style>
