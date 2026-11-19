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

  /** Escape player/panel stacking. Menus must sit above the whole chrome. */
  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return {
      destroy() {
        node.remove();
      },
    };
  }

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
    // Defer so the opening click cannot immediately dismiss the menu.
    // No window blur listener. Tauri focus blips were killing nested menus (sleep).
    let alive = true;
    let cleanup: (() => void) | undefined;
    const arm = window.setTimeout(() => {
      if (!alive) return;
      const on = (e: MouseEvent) => {
        if (el && !el.contains(e.target as Node)) onClose();
      };
      const esc = (e: KeyboardEvent) => {
        if (e.key === 'Escape') onClose();
      };
      window.addEventListener('mousedown', on);
      window.addEventListener('keydown', esc);
      cleanup = () => {
        window.removeEventListener('mousedown', on);
        window.removeEventListener('keydown', esc);
      };
    }, 0);
    return () => {
      alive = false;
      window.clearTimeout(arm);
      cleanup?.();
    };
  });

  function run(entry: MenuEntry, ev?: MouseEvent) {
    if ('sep' in entry || entry.disabled) return;
    ev?.preventDefault();
    ev?.stopPropagation();
    const fn = entry.onclick;
    // Close first so nested menus (sleep) can open cleanly, then fire.
    onClose();
    queueMicrotask(() => fn?.());
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="ctx"
  use:portal
  bind:this={el}
  style="left:{Math.round(pos.left)}px;top:{Math.round(pos.top)}px"
  role="menu"
  tabindex="-1"
  oncontextmenu={(e) => e.preventDefault()}
  onmousedown={(e) => e.stopPropagation()}
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
              <button type="button" role="menuitem" disabled={c.disabled} onclick={(ev) => run(c, ev)}>{c.label}</button>
            {/if}
          {/each}
        </div>
      {/if}
    {:else}
      <button type="button" role="menuitem" class:danger={e.danger} disabled={e.disabled} onclick={(ev) => run(e, ev)}>
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
