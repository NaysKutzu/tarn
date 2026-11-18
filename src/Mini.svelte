<script lang="ts">
  import { t as tr } from './lib/i18n.svelte';
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { api } from './lib/api';
  import type { PlayerState } from './lib/types';
  import Cover from './lib/components/Cover.svelte';
  import Hi from './lib/components/Hi.svelte';
  import { PlayIcon, PauseIcon, PreviousIcon, NextIcon, Cancel01Icon } from './lib/icons';
  import { artistsLine } from './lib/format';

  let player = $state<PlayerState | null>(null);

  // Opened from the tray: behave like a popup and go away when it loses focus.
  const popup = new URLSearchParams(location.search).has('pop');

  onMount(() => {
    for (const el of [document.documentElement, document.body, document.getElementById('app')]) {
      if (el) el.style.background = 'transparent';
    }
    let un: (() => void) | undefined;
    let unFocus: (() => void) | undefined;
    if (popup) {
      const w = getCurrentWindow();
      // Ignore the blur that can arrive before the window is first focused.
      let seenFocus = false;
      w.onFocusChanged(({ payload }) => {
        if (payload) seenFocus = true;
        else if (seenFocus) w.close().catch(() => {});
      }).then((u) => { unFocus = u; }).catch(() => {});
    }
    api.state().then((s) => { player = s; }).catch(() => {});
    listen<PlayerState>('player', (e) => {
      const next = e.payload;
      // Unchanged updates come without the queue; this window does not need it.
      player = next;
    }).then((u) => { un = u; });
    return () => { un?.(); unFocus?.(); };
  });

  let pct = $derived(
    player && player.duration > 0 ? Math.min(100, (player.position / player.duration) * 100) : 0,
  );

  function close() {
    getCurrentWindow().close().catch(() => {});
  }
</script>

<div class="mini" data-tauri-drag-region>
  <div class="cv" data-tauri-drag-region>
    <Cover src={player?.track?.cover ?? null} videoId={player?.track?.id ?? null} size={72} alt="" />
  </div>
  <div class="mid" data-tauri-drag-region>
    <div class="tn" data-tauri-drag-region>{player?.track?.title ?? tr('Nothing playing')}</div>
    <div class="ta" data-tauri-drag-region>{player?.track ? artistsLine(player.track.artists) : ''}</div>
    <div class="ctl">
      <button type="button" title={tr('Previous')} onclick={() => api.prev().catch(() => {})}><Hi icon={PreviousIcon} size={20} /></button>
      <button type="button" class="pp" title={player?.playing ? tr('Pause') : tr('Play')} onclick={() => api.toggle().catch(() => {})}>
        <Hi icon={player?.playing ? PauseIcon : PlayIcon} size={20} />
      </button>
      <button type="button" title={tr('Next')} onclick={() => api.next().catch(() => {})}><Hi icon={NextIcon} size={20} /></button>
    </div>
  </div>
  <button type="button" class="x" title={tr('Close')} onclick={close}><Hi icon={Cancel01Icon} size={16} /></button>
  <div class="bar" data-tauri-drag-region><div class="fill" style="width:{pct}%"></div></div>
</div>

<style>
  .mini {
    position: fixed; inset: 0;
    display: flex; align-items: center; gap: 12px;
    padding: 12px 14px 14px 12px;
    box-sizing: border-box;
    background: var(--bg-elev, #181715);
    color: var(--text, #f5f3f0);
    border: 1px solid var(--line-2, rgba(255, 255, 255, 0.12));
    border-radius: 12px;
    overflow: hidden;
    user-select: none;
  }
  .cv { flex: 0 0 auto; display: grid; }
  .cv :global(img), .cv :global(.ph) { border-radius: 8px; }
  .mid { min-width: 0; flex: 1; }
  .tn { font-size: 14px; font-weight: 600; line-height: 18px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .ta { font-size: 12px; line-height: 16px; color: var(--text-2, #c9c6c0); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .ctl { display: flex; align-items: center; gap: 6px; margin-top: 6px; }
  .ctl button, .x {
    display: grid; place-items: center;
    width: 32px; height: 32px; border-radius: 50%;
    color: inherit; background: none; border: 0; cursor: pointer;
  }
  .ctl button:hover, .x:hover { background: var(--bg-active, rgba(255, 255, 255, 0.08)); }
  .pp { background: var(--text, #f5f3f0) !important; color: #0c0b0a !important; }
  .x { position: absolute; top: 6px; right: 6px; width: 24px; height: 24px; opacity: 0.6; }
  .x:hover { opacity: 1; }
  .bar { position: absolute; left: 0; right: 0; bottom: 0; height: 3px; background: rgba(255, 255, 255, 0.08); }
  .fill { height: 100%; background: rgb(var(--accent-rgb, 245, 243, 240)); }
</style>
