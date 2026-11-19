<script lang="ts">
  import { t as tr } from '../i18n.svelte';
  import IconBtn from './IconBtn.svelte';
  import ContextMenu from './ContextMenu.svelte';
  import { MoonSleepIcon } from '../icons';
  import type { MenuEntry } from '../types';

  let {
    trackId,
    position,
    duration,
    onStop,
  }: {
    /** Id of the track that is playing now (null when nothing plays). */
    trackId: string | null;
    position: number;
    duration: number;
    /** Called when the timer fires: pause playback. */
    onStop: () => void;
  } = $props();

  // Either "after N minutes" (deadline in ms) or "when this track ends".
  type Mode = { kind: 'time'; at: number; mins: number } | { kind: 'track'; id: string };
  let mode = $state<Mode | null>(null);
  let now = $state(Date.now());
  let menu = $state<{ x: number; y: number } | null>(null);

  const PRESETS = [15, 30, 45, 60, 90];

  function fire() {
    mode = null;
    onStop();
  }

  // Tick only while a timer is running.
  $effect(() => {
    if (mode?.kind !== 'time') return;
    now = Date.now();
    const iv = setInterval(() => {
      now = Date.now();
      if (mode?.kind === 'time' && now >= mode.at) fire();
    }, 1000);
    return () => clearInterval(iv);
  });

  // End-of-track: stop in the last moment of the track, or as soon as the
  // player has moved on to another one (crossfade / skipped past the end).
  $effect(() => {
    if (mode?.kind !== 'track') return;
    if (trackId !== mode.id) fire();
    else if (duration > 0 && position >= duration - 0.6) fire();
  });

  let left = $derived.by(() => {
    if (!mode) return '';
    if (mode.kind === 'track') return 'end';
    const s = Math.max(0, Math.ceil((mode.at - now) / 1000));
    const m = Math.ceil(s / 60);
    return m >= 60 ? `${Math.floor(m / 60)}h${String(m % 60).padStart(2, '0')}` : `${m}m`;
  });

  function entries(): MenuEntry[] {
    const list: MenuEntry[] = PRESETS.map((m) => ({
      label: m >= 60 && m % 60 === 0 ? `${m / 60} hour${m > 60 ? 's' : ''}` : `${m} minutes`,
      onclick: () => (mode = { kind: 'time', at: Date.now() + m * 60_000, mins: m }),
    }));
    list.push({
      label: tr('End of this track'),
      disabled: !trackId,
      onclick: () => { if (trackId) mode = { kind: 'track', id: trackId }; },
    });
    if (mode) {
      list.push({ sep: true });
      list.push({ label: tr('Turn off sleep timer'), danger: true, onclick: () => (mode = null) });
    }
    return list;
  }

  function open(e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    // Open above the player bar, aligned to the button.
    menu = { x: r.left, y: Math.max(8, r.top - 260) };
  }
</script>

<div class="st">
  <IconBtn
    icon={MoonSleepIcon}
    size={22}
    btnSize="lg"
    on={!!mode}
    onclick={open}
    title={mode ? `Sleep timer: ${left}` : 'Sleep timer'}
  />
  {#if mode}<span class="left">{left}</span>{/if}
</div>

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} entries={entries()} onClose={() => (menu = null)} />
{/if}

<style>
  .st { position: relative; display: inline-flex; align-items: center; }
  .left {
    position: absolute;
    right: -2px;
    bottom: -2px;
    font-size: 9px;
    font-weight: 700;
    line-height: 1;
    padding: 2px 3px;
    border-radius: 6px;
    background: rgb(var(--accent-rgb));
    color: var(--accent-fg, #fff);
    pointer-events: none;
  }
</style>
