<script lang="ts">
  import { t as tr } from '../i18n.svelte';
  import type { Track } from '../types';
  import { artistsLine, fmtTime } from '../format';
  import Cover from './Cover.svelte';
  import Hi from './Hi.svelte';
  import { Cancel01Icon, DragDropVerticalIcon, Download04Icon } from '../icons';
  import { off } from '../offline.svelte';
  import { filterTracks } from '../filter';

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

  // The side panel starts just above the current track and can reveal earlier
  // ones. The fullscreen rail keeps the played rows above (scrolled out of
  // view) in chunks of 10, so the window only shifts every few tracks and the
  // list can glide to the next song instead of re-rendering.
  let first = $derived(
    variant === 'np'
      ? Math.max(0, Math.floor(index / 10) * 10 - 10)
      : Math.max(0, index - 3 - back),
  );

  // Fullscreen rail: keep the current song at the top, easing to the next one.
  // Own rAF tween: native smooth scrolling is off whenever the desktop has
  // animations disabled, and a user wheel / drag must be able to interrupt it.
  let lastIdx = -1;
  let lastFirst = -1;
  let glideRaf = 0;
  function stopGlide() {
    cancelAnimationFrame(glideRaf);
    glideRaf = 0;
  }
  function glideTo(sc: HTMLElement, to: number, ms: number) {
    stopGlide();
    const from = sc.scrollTop;
    if (ms <= 0 || Math.abs(to - from) < 1) {
      sc.scrollTop = to;
      return;
    }
    const t0 = performance.now();
    const step = (now: number) => {
      const p = Math.min(1, (now - t0) / ms);
      const e = 1 - Math.pow(1 - p, 3);
      sc.scrollTop = from + (to - from) * e;
      glideRaf = p < 1 ? requestAnimationFrame(step) : 0;
    };
    glideRaf = requestAnimationFrame(step);
  }
  // A highlight "pill" that glides from one row to the next (fullscreen rail).
  let pill = $state<{ y: number; h: number } | null>(null);
  let pillAnim = $state(false);
  $effect(() => {
    const idx = index;
    void first;
    void rows.length;
    if (variant !== 'np' || !listEl) return;
    const row = listEl.querySelector<HTMLElement>(`[data-qi="${idx}"]`);
    if (!row) { pill = null; return; }
    pill = { y: row.offsetTop, h: row.offsetHeight };
    if (!pillAnim) requestAnimationFrame(() => requestAnimationFrame(() => (pillAnim = true)));
  });

  $effect(() => {
    const idx = index;
    const f = first;
    if (variant !== 'np' || !listEl || searching) return;
    const sc = listEl.closest<HTMLElement>('.np-queue-list');
    const row = listEl.querySelector<HTMLElement>(`[data-qi="${idx}"]`);
    if (!sc || !row) return;
    // Only follow the song when it actually changed: any other re-run
    // (state ticks, list refresh) must not yank the list back while the
    // user is scrolling it.
    if (idx === lastIdx && f === lastFirst) return;
    const smooth = lastIdx >= 0 && f === lastFirst && Math.abs(idx - lastIdx) === 1;
    lastIdx = idx;
    lastFirst = f;
    const top = row.getBoundingClientRect().top - sc.getBoundingClientRect().top + sc.scrollTop - 4;
    glideTo(sc, Math.max(0, top), smooth ? 550 : 0);
    const stop = () => stopGlide();
    sc.addEventListener('wheel', stop, { passive: true });
    sc.addEventListener('pointerdown', stop, { passive: true });
    sc.addEventListener('touchstart', stop, { passive: true });
    return () => {
      sc.removeEventListener('wheel', stop);
      sc.removeEventListener('pointerdown', stop);
      sc.removeEventListener('touchstart', stop);
    };
  });

  // Optimistic order while a drop is in flight, so rows don't snap back.
  let local = $state<Track[] | null>(null);
  let shown = $derived(local ?? tracks);
  // Search (side panel only): keeps each row's real queue index so play,
  // remove and the keyboard shortcuts still act on the right track.
  let q = $state('');
  let searching = $derived(variant === 'panel' && q.trim().length > 0);
  const MAX_HITS = 100;
  let hits = $derived.by(() => {
    if (!searching) return [] as { t: Track; i: number }[];
    const want = new Set(filterTracks(shown, q));
    const out: { t: Track; i: number }[] = [];
    for (let i = 0; i < shown.length && out.length < MAX_HITS; i++) {
      if (want.has(shown[i])) out.push({ t: shown[i], i });
    }
    return out;
  });

  let end = $derived(Math.min(shown.length, first + limit));
  let rows = $derived(
    searching ? hits : shown.slice(first, end).map((t, k) => ({ t, i: first + k })),
  );

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
    if (e.button !== 0 || searching) return;
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

{#if variant === 'panel' && tracks.length > 8}
  <input
    class="qsearch"
    type="search"
    placeholder={tr('Search in queue')}
    aria-label={tr('Search in queue')}
    bind:value={q}
  />
{/if}
<div class="ql" class:npq={variant === 'np'} class:dragging={!!drag} bind:this={listEl}>
  {#if variant === 'np' && pill}
    <div class="pill" class:anim={pillAnim} style="height:{pill.h}px;transform:translateY({pill.y}px)"></div>
  {/if}
  {#if searching && !hits.length}
    <div class="qnone">No songs match “{q}”</div>
  {/if}
  {#if first > 0 && variant === 'panel' && !searching}
    <button class="more" type="button" onclick={() => (back += PAGE)}>
      Show earlier ({first})
    </button>
  {/if}
  {#each rows as { t, i } (`${i}:${t.id}`)}
    {@const gone = off.offline && !off.playable.has(t.id)}
    <div
      class="qr"
      class:now={i === index}
      class:played={i < index}
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
        title={tr('Drag to reorder')}
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
        <span class="dl" title={tr('Downloaded')}><Hi icon={Download04Icon} size={13} /></span>
      {/if}
      {#if i === index}
        <span class="now-tag">{tr('Playing')}</span>
      {:else}
        {#if t.duration}<span class="dur">{fmtTime(t.duration)}</span>{/if}
        <button
          class="rm"
          type="button"
          title={tr('Remove from queue')}
          onclick={(e) => { e.stopPropagation(); onRemove(i); }}
        ><Hi icon={Cancel01Icon} size={16} /></button>
      {/if}
    </div>
  {/each}
  {#if !searching && end < shown.length}
    <div class="more-sentinel" bind:this={sentinel}></div>
  {/if}
  {#if !searching && drag && end === shown.length && drag.over >= shown.length && drag.from !== shown.length - 1}
    <div class="drop-end"></div>
  {/if}
</div>

<style>
  .ql { position: relative; }
  .qsearch {
    width: 100%;
    box-sizing: border-box;
    margin: 0 0 10px;
    padding: 9px 12px;
    font: inherit;
    font-size: 13px;
    color: var(--text);
    background: var(--bg-elev);
    border: 1px solid var(--line);
    border-radius: 10px;
    outline: none;
  }
  .qsearch::placeholder { color: var(--text-3); }
  .qsearch:focus {
    border-color: color-mix(in srgb, var(--text) 28%, transparent);
  }
  .qnone { padding: 14px 4px; font-size: 13px; color: var(--text-3); }
  .more {
    display: block;
    width: 100%;
    padding: 8px 4px;
    margin-bottom: 2px;
    font-size: 12px;
    color: var(--text-3);
    border-radius: 0;
    text-align: left;
  }
  .more:hover { background: transparent; color: var(--text); }
  .more-sentinel { height: 1px; }
  .ql.dragging { user-select: none; cursor: grabbing; }
  .qr {
    position: relative;
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 52px;
    padding: 5px 6px 5px 22px;
    box-sizing: border-box;
    width: 100%;
    margin-bottom: 0;
    border-radius: 8px;
    color: inherit;
    cursor: default;
    outline: none;
  }
  .qr :global(img), .qr :global(.ph) { border-radius: 6px; flex: 0 0 auto; }
  .npq .qr :global(img), .npq .qr :global(.ph) { border-radius: 12px; box-shadow: 0 3px 10px rgba(0, 0, 0, 0.4); }
  .qr:hover, .qr:focus-visible { background: var(--bg-hover); }
  .qr.now {
    background: transparent;
  }
  .qr.now .tn { color: var(--text); font-weight: 650; }
  .qr.now .ta { color: var(--text-2); }
  .qr.played { opacity: 0.48; }
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
    font-size: 13px; line-height: 18px; font-weight: 550;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }
  .ta {
    font-size: 12px; line-height: 16px; color: var(--text-3);
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }
  .dur { font-size: 12px; color: var(--text-3); font-variant-numeric: tabular-nums; }
  .dl { color: var(--text-3); opacity: 0.8; display: grid; place-items: center; flex: 0 0 auto; }
  .now-tag {
    flex: 0 0 auto;
    font-size: 11px;
    font-weight: 550;
    letter-spacing: 0.02em;
    color: var(--text-3);
    text-transform: none;
  }
  .qr.now .now-tag { color: var(--text-2); }
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
  .npq .qr { padding: 7px 12px 7px 22px; border-radius: 18px; background-clip: padding-box; transition: background 0.5s var(--ease, ease), opacity 0.5s var(--ease, ease); }
  .npq .dl { color: rgba(245, 243, 240, 0.55); }
  .npq .qr:hover, .npq .qr:focus-visible { background: rgba(245, 243, 240, 0.08); }
  .npq .qr.now { background: transparent; }
  .pill {
    position: absolute;
    left: 0;
    right: 0;
    top: 0;
    border-radius: 18px;
    pointer-events: none;
    background: linear-gradient(180deg, rgba(245, 243, 240, 0.16), rgba(245, 243, 240, 0.08));
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.16), inset 0 -10px 18px rgba(0, 0, 0, 0.14);
  }
  .pill.anim { transition: transform 0.55s cubic-bezier(0.22, 1, 0.36, 1), height 0.3s ease; }
  .npq .tn { font-size: 13px; }
  .npq .ta, .npq .dur, .npq .rm { color: rgba(245, 243, 240, 0.55); }
  .npq .now-tag { color: rgba(245, 243, 240, 0.8); }
  .npq .rm:hover { background: rgba(255, 255, 255, 0.12); color: #fff; }
  .npq .grip { color: rgba(245, 243, 240, 0.45); }
</style>
