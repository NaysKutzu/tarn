<script lang="ts">
  import { t as tr } from '../i18n.svelte';
  import Hi from './Hi.svelte';
  import { Search01Icon, Cancel01Icon } from '../icons';

  let {
    value = $bindable(''),
    placeholder = 'Filter',
    /** "12 of 340" style hint shown while filtering. */
    count = '',
    id = 'filter-input',
    compact = false,
  }: {
    value?: string;
    placeholder?: string;
    count?: string;
    id?: string;
    compact?: boolean;
  } = $props();
</script>

<div class="fb" class:compact>
  <span class="fb-ic"><Hi icon={Search01Icon} size={compact ? 15 : 17} /></span>
  <input
    {id}
    type="text"
    bind:value
    {placeholder}
    autocomplete="off"
    spellcheck="false"
    onkeydown={(e) => { if (e.key === 'Escape') { value = ''; (e.currentTarget as HTMLInputElement).blur(); } }}
  />
  {#if value}
    {#if count}<span class="fb-n">{count}</span>{/if}
    <button class="fb-x" type="button" title={tr('Clear')} onclick={() => { value = ''; document.getElementById(id)?.focus(); }}>
      <Hi icon={Cancel01Icon} size={14} />
    </button>
  {/if}
</div>

<style>
  .fb {
    display: flex; align-items: center; gap: 8px;
    height: 38px; padding: 0 10px 0 12px;
    border-radius: 10px;
    background: var(--bg-elev);
    border: 1px solid var(--line);
    color: var(--text-2);
    max-width: 380px;
    transition: border-color 0.15s, background 0.15s;
  }
  .fb:focus-within { border-color: rgba(var(--accent-rgb), 0.55); background: var(--bg-hover); color: var(--text); }
  .fb.compact {
    height: 28px;
    max-width: none;
    margin: 0 4px 6px;
    padding: 0 8px 0 9px;
    border-radius: 7px;
    background: var(--bg-hover);
    border-color: transparent;
    gap: 6px;
  }
  .fb.compact:hover { background: var(--bg-active); }
  .fb.compact:focus-within {
    background: var(--bg-elev);
    border-color: transparent;
    box-shadow: inset 0 0 0 1px rgba(var(--accent-rgb), 0.35);
  }
  .fb.compact input { font-size: 12.5px; }
  .fb-ic { display: grid; place-items: center; flex: 0 0 auto; }
  input {
    flex: 1; min-width: 0; background: none; border: 0; outline: 0;
    color: var(--text); font: inherit; font-size: 14px;
  }
  input::placeholder { color: var(--text-3, #6b7280); }
  .fb-n { font-size: 12px; color: var(--text-2); white-space: nowrap; font-variant-numeric: tabular-nums; }
  .fb-x { display: grid; place-items: center; width: 22px; height: 22px; border-radius: 50%; color: var(--text-2); }
  .fb-x:hover { background: var(--bg-active); color: var(--text); }
</style>
