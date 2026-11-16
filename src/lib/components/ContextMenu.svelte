<script lang="ts">
  import Hi from './Hi.svelte';
  import { ArrowRight01Icon } from '../icons';
  import type { MenuEntry } from '../types';

  let {
    x,
    y,
    entries,
    onClose,
  }: {
    x: number;
    y: number;
    entries: MenuEntry[];
    onClose: () => void;
  } = $props();

  let open = $state<number | null>(null);
  let el: HTMLDivElement | undefined = $state();
  let pos = $state({ left: 0, top: 0 });

  // Keep the menu on screen, whatever its height turns out to be.
  $effect(() => {
    void entries;
    void open;
    const w = el?.offsetWidth ?? 240;
    const h = el?.offsetHeight ?? 320;
    pos = {
      left: Math.max(8, Math.min(x, window.innerWidth - w - 8)),
      top: Math.max(8, Math.min(y, window.innerHeight - h - 8)),
    };
  });

  $effect(() => {
    const on = (e: MouseEvent) => {
      if (el && !el.contains(e.target as Node)) onClose();
    };
    const esc = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('mousedown', on);
    window.addEventListener('keydown', esc);
    window.addEventListener('blur', onClose);
    return () => {
      window.removeEventListener('mousedown', on);
      window.removeEventListener('keydown', esc);
      window.removeEventListener('blur', onClose);
    };
  });

  function run(e: MenuEntry) {
    if ('sep' in e || e.disabled) return;
    e.onclick?.();
    onClose();
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="ctx"
  bind:this={el}
  style="left:{Math.round(pos.left)}px;top:{Math.round(pos.top)}px"
  role="menu"
  tabindex="-1"
  oncontextmenu={(e) => e.preventDefault()}
>
  {#each entries as e, i}
    {#if 'sep' in e}
      <div class="sep"></div>
    {:else if e.children}
      <button type="button" role="menuitem" onclick={() => (open = open === i ? null : i)}>
        <span class="ic">{#if e.icon}<Hi icon={e.icon} size={18} />{/if}</span>
        <span class="lbl">{e.label}</span>
        <span class="chev" class:open={open === i}><Hi icon={ArrowRight01Icon} size={14} /></span>
      </button>
      {#if open === i}
        <div class="sub">
          {#each e.children as c}
            {#if 'sep' in c}
              <div class="sep"></div>
            {:else}
              <button type="button" role="menuitem" disabled={c.disabled} onclick={() => run(c)}>{c.label}</button>
            {/if}
          {/each}
        </div>
      {/if}
    {:else}
      <button type="button" role="menuitem" class:danger={e.danger} disabled={e.disabled} onclick={() => run(e)}>
        <span class="ic">{#if e.icon}<Hi icon={e.icon} size={18} />{/if}</span>
        <span class="lbl">{e.label}</span>
      </button>
    {/if}
  {/each}
</div>

<style>
  .lbl { flex: 1; min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .ic { width: 18px; height: 18px; flex: 0 0 18px; }
  .chev { color: var(--text-2); display: flex; transition: transform 0.15s; }
  .chev.open { transform: rotate(90deg); }
  .sub {
    max-height: 220px; overflow: auto; margin: 2px 0 4px 10px;
    padding-left: 8px; border-left: 1px solid var(--line);
  }
  .sub button { padding-left: 10px; min-height: 34px; }
  .danger { color: #ff7a6b; }
  button:disabled { opacity: 0.4; pointer-events: none; }
</style>
