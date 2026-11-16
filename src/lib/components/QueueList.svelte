<script lang="ts">
  import type { Track } from '../types';
  import { artistsLine, fmtTime } from '../format';
  import Cover from './Cover.svelte';
  import Hi from './Hi.svelte';
  import { Cancel01Icon, DragDropVerticalIcon, Download04Icon } from '../icons';
  import { off } from '../offline.svelte';

  let {
    tracks,
    index,
    variant = 'panel',
    onPlay,
    onRemove,
    onMove,
    onContext,
    onHover,
  }: {
    tracks: Track[];
    index: number;
    variant?: 'panel' | 'np';
    onPlay: (i: number) => void;
    onRemove: (i: number) => void | Promise<void>;
    onMove: (from: number, to: number) => void | Promise<void>;
    onContext?: (e: MouseEvent, t: Track) => void;
    onHover?: (t: Track) => void;
  } = $props();

  // Only a window of the queue is in the DOM: a few hundred rows (each with a
  // cover) froze the panel. More rows are added as the list is scrolled.
  const PAGE = 40;
  let limit = $state(PAGE);
  let back = $state(0); // extra already-played rows revealed above the current one

  // The fullscreen rail only shows what is left; the side panel starts just
  // above the current track and can reveal earlier ones.
  let first = $derived(
    variant === 'np' ? Math.max(0, index) : Math.max(0, index - 3 - back),
  );

  // Optimistic order while a drop is in flight, so rows don't snap back.
  let local = $state<Track[] | null>(null);
  let shown = $derived(local ?? tracks);
  let end = $derived(Math.min(shown.length, first + limit));
  let rows = $derived(shown.slice(first, end).map((t, k) => ({ t, i: first + k })));

  let sentinel = $state<HTMLDivElement | undefined>();
  $effect(() => {
    if (!sentinel || typeof IntersectionObserver === 'undefined') return;
    const root = sentinel.closest<HTMLElement>('.panel-b, .np-queue-list');
    const io = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) limit += PAGE;
      },
      { root, rootMargin: '400px 0px' },
    );
    io.observe(sentinel);
    return () => io.disconnect();
  });

  let listEl = $state<HTMLDivElement | undefined>();
  let drag = $state<{ from: number; over: number } | null>(null);
  let scroller: HTMLElement | null = null;
  let lastY = 0;
  let raf = 0;

  function slotAt(y: number): number {
    if (!listEl) return first;
    const els = listEl.querySelectorAll<HTMLElement>('[data-qi]');
    for (const el of els) {
      const r = el.getBoundingClientRect();
      if (y < r.top + r.height / 2) return Number(el.dataset.qi);
    }
    return end;
  }

  function startDrag(e: PointerEvent, i: number) {
    if (e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    scroller = listEl?.closest<HTMLElement>('.panel-b, .np-queue-list') ?? null;
    drag = { from: i, over: i };
    lastY = e.clientY;
    tick();
  }

  function moveDrag(e: PointerEvent) {
    if (!drag) return;
    lastY = e.clientY;
    drag = { from: drag.from, over: Math.max(first, slotAt(e.clientY)) };
  }

  // Scroll the list while the pointer hovers near its edge.
  function tick() {
    cancelAnimationFrame(raf);
    if (!drag) return;
    if (scroller) {
      const r = scroller.getBoundingClientRect();
      const edge = 44;
      if (lastY < r.top + edge) scroller.scrollTop -= Math.ceil((r.top + edge - lastY) / 4);
      else if (lastY > r.bottom - edge) scroller.scrollTop += Math.ceil((lastY - (r.bottom - edge)) / 4);
      drag = { from: drag.from, over: Math.max(first, slotAt(lastY)) };
    }
    raf = requestAnimationFrame(tick);
  }

  async function endDrag(e: PointerEvent) {
    if (!drag) return;
    cancelAnimationFrame(raf);
    const { from, over } = drag;
    drag = null;
    try { (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId); } catch { /* already released */ }
    const to = over > from ? over - 1 : over;
    if (to === from) return;
    const next = shown.slice();
    const [m] = next.splice(from, 1);
    next.splice(to, 0, m);
    local = next;
    try {
      await onMove(from, to);
    } finally {
      // The next state tick carries the real order; drop the override.
      setTimeout(() => (local = null), 250);
    }
  }

  function cancelDrag() {
    cancelAnimationFrame(raf);
    drag = null;
  }

  $effect(() => {
    void tracks;
    // Backend caught up - fall back to the authoritative list.
    if (!drag) local = null;
  });

  function key(e: KeyboardEvent, i: number) {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      onPlay(i);
    } else if ((e.key === 'Delete' || e.key === 'Backspace') && i !== index) {
      e.preventDefault();
      onRemove(i);
    } else if (e.altKey && e.key === 'ArrowUp' && i > first) {
      e.preventDefault();
      onMove(i, i - 1);
    } else if (e.altKey && e.key === 'ArrowDown' && i < shown.length - 1) {
      e.preventDefault();
      onMove(i, i + 1);
    }
  }
</script>

<svelte:window onblur={cancelDrag} />

<div class="ql" class:np={variant === 'np'} class:dragging={!!drag} bind:this={listEl}>
  {#if first > 0 && variant === 'panel'}
    <button class="more" type="button" onclick={() => (back += PAGE)}>
      Show earlier ({first})
    </button>
  {/if}
  {#each rows as { t, i } (`${i}:${t.id}`)}
    {@const gone = off.offline && !off.playable.has(t.id)}
    <div
      class="qr"
      class:now={i === index}
      class:played={variant === 'panel' && i < index}
      class:lifted={drag?.from === i}
      class:drop-before={drag && drag.over === i && drag.from !== i && drag.from + 1 !== i}
      class:gone
      data-qi={i}
      role="button"
      tabindex="0"
      onclick={() => onPlay(i)}
      onkeydown={(e) => key(e, i)}
      oncontextmenu={(e) => onContext?.(e, t)}
      onmouseenter={() => onHover?.(t)}
    >
      <span
        class="grip"
        role="presentation"
        title="Drag to reorder"
        onpointerdown={(e) => startDrag(e, i)}
        onpointermove={moveDrag}
        onpointerup={endDrag}
        onpointercancel={cancelDrag}
        onclick={(e) => e.stopPropagation()}
      ><Hi icon={DragDropVerticalIcon} size={16} /></span>
      <Cover src={t.cover} videoId={t.id} size={variant === 'np' ? 40 : 40} alt="" />
      <div class="tx">
        <div class="tn">{t.title}</div>
        <div class="ta">{artistsLine(t.artists)}</div>
      </div>
      {#if off.downloaded.has(t.id)}
        <span class="dl" title="Downloaded"><Hi icon={Download04Icon} size={13} /></span>
      {/if}
      {#if i === index}
        <span class="now-tag">Now</span>
      {:else}
        {#if t.duration}<span class="dur">{fmtTime(t.duration)}</span>{/if}
        <button
          class="rm"
          type="button"
          title="Remove from queue"
          onclick={(e) => { e.stopPropagation(); onRemove(i); }}
        ><Hi icon={Cancel01Icon} size={16} /></button>
      {/if}
    </div>
  {/each}
  {#if end < shown.length}
    <div class="more-sentinel" bind:this={sentinel}></div>
  {/if}
  {#if drag && end === shown.length && drag.over >= shown.length && drag.from !== shown.length - 1}
    <div class="drop-end"></div>
  {/if}
</div>

<style>
  .ql { position: relative; }
  .more {
    display: block;
    width: 100%;
    padding: 8px;
    margin-bottom: 4px;
    font-size: 12px;
    color: var(--text-2);
    border-radius: 8px;
  }
  .more:hover { background: var(--bg-hover); color: var(--text); }
  .more-sentinel { height: 1px; }
  .ql.dragging { user-select: none; cursor: grabbing; }
  .qr {
    position: relative;
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 52px;
    padding: 4px 8px 4px 22px;
    box-sizing: border-box;
    width: 100%;
    margin-bottom: 2px;
    border-radius: 8px;
    color: inherit;
    cursor: default;
    outline: none;
  }
  .qr :global(img), .qr :global(.ph) { border-radius: 8px; flex: 0 0 auto; }
  .np .qr :global(img), .np .qr :global(.ph) { border-radius: 10px; }
  .qr:hover, .qr:focus-visible { background: var(--bg-hover); }
  .qr.now { background: var(--accent-dim); }
  .qr.played { opacity: 0.55; }
  .qr.gone { opacity: 0.38; }
  .qr.lifted { opacity: 0.45; }
  .qr.drop-before::before,
  .drop-end {
    content: '';
    position: absolute;
    left: 8px;
    right: 8px;
    top: -1px;
    height: 2px;
    border-radius: 2px;
    background: rgb(var(--accent-rgb));
    pointer-events: none;
    z-index: 2;
  }
  .drop-end { position: relative; left: 0; margin: 0 8px; right: 0; top: 0; }
  .grip {
    position: absolute;
    left: 5px;
    top: 50%;
    transform: translateY(-50%);
    width: 14px;
    display: grid;
    place-items: center;
    height: 36px;
    color: var(--text-3, #6b7280);
    opacity: 0;
    cursor: grab;
    touch-action: none;
    border-radius: 4px;
  }
  .qr:hover .grip, .qr:focus-visible .grip, .ql.dragging .grip { opacity: 1; }
  .grip:hover { color: var(--text); }
  .tx { min-width: 0; flex: 1; }
  .tn {
    font-size: 14px; line-height: 18px; font-weight: 600;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }
  .ta {
    font-size: 12px; line-height: 16px; color: var(--text-2);
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }
  .dur { font-size: 12px; color: var(--text-2); font-variant-numeric: tabular-nums; }
  .dl { color: var(--text-2); opacity: 0.8; display: grid; place-items: center; flex: 0 0 auto; }
  .now-tag {
    flex: 0 0 auto;
    font-size: 10px; font-weight: 700; letter-spacing: 0.06em; text-transform: uppercase;
    color: rgb(var(--accent-rgb));
  }
  .rm {
    display: grid; place-items: center;
    width: 28px; height: 28px; border-radius: 50%;
    color: var(--text-2);
    opacity: 0;
  }
  .qr:hover .rm, .qr:focus-visible .rm { opacity: 1; }
  .rm:hover { background: var(--bg-active); color: var(--text); }
  .qr:hover .dur { display: none; }

  /* Fullscreen rail: light-on-dark like the rest of the now-playing screen. */
  .np .qr { padding: 6px 10px 6px 22px; border-radius: 10px; }
  .np .dl { color: rgba(245, 243, 240, 0.55); }
  .np .qr:hover, .np .qr:focus-visible { background: rgba(245, 243, 240, 0.08); }
  .np .qr.now { background: rgba(245, 243, 240, 0.1); }
  .np .tn { font-size: 13px; }
  .np .ta, .np .dur, .np .rm { color: rgba(245, 243, 240, 0.55); }
  .np .now-tag { color: rgba(245, 243, 240, 0.8); }
  .np .rm:hover { background: rgba(255, 255, 255, 0.12); color: #fff; }
  .np .grip { color: rgba(245, 243, 240, 0.45); }
</style>
