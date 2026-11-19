<script lang="ts">
  import type { Snippet } from 'svelte';
  import { onMount } from 'svelte';

  let {
    children,
    class: className = '',
    canLeft = $bindable(false),
    canRight = $bindable(false),
  }: {
    children: Snippet;
    class?: string;
    canLeft?: boolean;
    canRight?: boolean;
  } = $props();

  let rowEl = $state<HTMLDivElement | null>(null);

  function update() {
    const el = rowEl;
    if (!el) {
      canLeft = false;
      canRight = false;
      return;
    }
    const max = el.scrollWidth - el.clientWidth;
    const overflow = max > 4;
    canLeft = overflow && el.scrollLeft > 2;
    canRight = overflow && el.scrollLeft < max - 2;
  }

  /** Page the row left (-1) or right (1). Bound via `bind:this`. */
  export function page(dir: -1 | 1) {
    const el = rowEl;
    if (!el) return;
    const step = Math.max(Math.floor(el.clientWidth * 0.85), 240);
    el.scrollBy({ left: dir * step, behavior: 'smooth' });
  }

  onMount(() => {
    const el = rowEl;
    if (!el) return;
    update();
    const ro = new ResizeObserver(update);
    ro.observe(el);
    const mo = new MutationObserver(update);
    mo.observe(el, { childList: true, subtree: true });
    return () => {
      ro.disconnect();
      mo.disconnect();
    };
  });
</script>

<div class="scroll-row {className}">
  <div class="row" bind:this={rowEl} onscroll={update}>
    {@render children()}
  </div>
</div>
