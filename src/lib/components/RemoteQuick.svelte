<script lang="ts">
  import { onMount } from 'svelte';
  import { api, type RemoteInfo } from '../api';
  import { t as tr } from '../i18n.svelte';
  import Hi from './Hi.svelte';
  import { SmartPhone01Icon, Cancel01Icon } from '../icons';
  import Toggle from './Toggle.svelte';

  let {
    /** Popover opens above the button (player bar) or below (title bar). */
    placement = 'up',
    /** When false, parent opens the panel via openRemote(). */
    showButton = true,
  }: {
    placement?: 'up' | 'down';
    showButton?: boolean;
  } = $props();

  let info = $state<RemoteInfo>({ running: false, url: null, qr_svg: null, error: null });
  let busy = $state(false);
  let open = $state(false);
  let copied = $state(false);
  let root = $state<HTMLElement | null>(null);
  let popEl = $state<HTMLElement | null>(null);
  /** Fixed coords so the panel clears the player chrome. */
  let popPos = $state<{ top?: string; bottom?: string; right: string } | null>(null);

  /** Mount the popover on <body> so player/panel stacking cannot bury it. */
  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return {
      destroy() {
        node.remove();
      },
    };
  }

  function placePop() {
    if (!root) return;
    const anchor = root.querySelector('button') ?? root;
    const r = anchor.getBoundingClientRect();
    const right = `${Math.max(8, window.innerWidth - r.right)}px`;
    popPos = placement === 'up'
      ? { bottom: `${Math.max(8, window.innerHeight - r.top + 10)}px`, right }
      : { top: `${r.bottom + 8}px`, right };
  }

  export function running(): boolean {
    return info.running;
  }

  /** Open the QR panel; starts the remote if it is off. */
  export async function openRemote() {
    if (busy) return;
    placePop();
    open = true;
    if (!info.running) {
      await set(true);
      return;
    }
    try { info = await api.remoteStatus(); } catch { /* ignore */ }
  }

  onMount(() => {
    api.remoteStatus().then((i) => (info = i)).catch(() => {});
  });

  $effect(() => {
    if (!open) return;
    // Defer so the same click that opened us does not immediately close.
    let ready = false;
    const arm = window.setTimeout(() => { ready = true; }, 0);
    const onDoc = (e: PointerEvent) => {
      if (!ready) return;
      const t = e.target as Node;
      if (root?.contains(t) || popEl?.contains(t)) return;
      open = false;
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') open = false;
    };
    placePop();
    document.addEventListener('pointerdown', onDoc, true);
    document.addEventListener('keydown', onKey);
    window.addEventListener('resize', placePop);
    return () => {
      window.clearTimeout(arm);
      document.removeEventListener('pointerdown', onDoc, true);
      document.removeEventListener('keydown', onKey);
      window.removeEventListener('resize', placePop);
      popPos = null;
    };
  });

  async function set(on: boolean) {
    busy = true;
    try {
      if (on) info = await api.remoteStart();
      else {
        await api.remoteStop();
        info = { running: false, url: null, qr_svg: null, error: null };
      }
    } catch (e) {
      info = { running: false, url: null, qr_svg: null, error: String(e) };
    } finally {
      busy = false;
    }
  }

  async function copyUrl() {
    if (!info.url) return;
    try {
      await navigator.clipboard.writeText(info.url);
      copied = true;
      setTimeout(() => (copied = false), 1600);
    } catch { /* ignore */ }
  }

  /** First click starts the remote and shows the QR. Later clicks toggle the panel. */
  async function onBtn(e: MouseEvent) {
    e.stopPropagation();
    if (busy) return;
    if (!info.running) {
      placePop();
      open = true;
      await set(true);
      return;
    }
    open = !open;
    if (open) {
      placePop();
      try { info = await api.remoteStatus(); } catch { /* ignore */ }
    }
  }
</script>

<div class="rq" class:up={placement === 'up'} class:nobtn={!showButton} bind:this={root}>
  {#if showButton}
    <button
      class="ibtn lg rq-btn"
      type="button"
      class:on={open}
      class:live={info.running && !open}
      title={tr('Phone remote')}
      aria-label={tr('Phone remote')}
      aria-expanded={open}
      aria-haspopup="dialog"
      disabled={busy}
      onclick={onBtn}
    >
      <Hi icon={SmartPhone01Icon} size={22} />
      {#if info.running && !open}<i class="rq-dot" aria-hidden="true"></i>{/if}
    </button>
  {/if}
  {#if open}
    <div
      class="rq-pop"
      role="dialog"
      aria-label={tr('Phone remote')}
      use:portal
      bind:this={popEl}
      style:position="fixed"
      style:top={popPos?.top}
      style:bottom={popPos?.bottom}
      style:right={popPos?.right}
    >
      <div class="rq-h">
        <div>
          <div class="rq-title">{tr('Phone remote')}</div>
          <div class="rq-sub">{tr("Control Tarn from your phone's browser on the same Wi-Fi. Stays on until you turn it off")}</div>
        </div>
        <button class="rq-x" type="button" aria-label={tr('Close')} onclick={() => (open = false)}>
          <Hi icon={Cancel01Icon} size={18} />
        </button>
      </div>
      <div class="rq-row">
        <span>{tr('Phone remote')}</span>
        <Toggle checked={info.running} disabled={busy} label={tr('Phone remote')} onchange={(v) => set(v)} />
      </div>
      {#if info.error}
        <p class="rq-err">{info.error}</p>
      {/if}
      {#if info.running && info.qr_svg}
        <div class="rq-body">
          <div class="rq-qr" aria-label="QR code for the phone remote">{@html info.qr_svg}</div>
          <div class="rq-txt">
            <b>{tr('Scan with your phone camera')}</b>
            <span class="rq-meta">{tr('Search songs, control playback, queue and lyrics. Same Wi-Fi only.')}</span>
            <button class="rq-url" type="button" onclick={copyUrl} title={tr('Copy link')}>{info.url}</button>
            <span class="rq-meta">{copied ? tr('Copied') : tr('Anyone with this link on your network can control playback. Turning the remote off and on makes a new link.')}</span>
          </div>
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .rq {
    position: relative;
    flex: 0 0 auto;
    z-index: 40;
  }
  .rq.nobtn {
    position: absolute;
    inset: 0;
    pointer-events: none;
  }
  .rq.nobtn .rq-pop {
    pointer-events: auto;
  }
  .rq-btn {
    position: relative;
  }
  /* Running remote = tiny dot, not a permanent accent “on” state. */
  .rq-btn.live {
    color: inherit;
  }
  .rq-dot {
    position: absolute;
    top: 8px;
    right: 8px;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: rgb(var(--accent-rgb));
    box-shadow: 0 0 0 2px var(--bg-player, #141312);
  }
  .rq-pop {
    /* Fixed coords escape the player; z-index must clear panel/np chrome. */
    width: min(380px, calc(100vw - 24px));
    padding: 14px;
    border-radius: 16px;
    border: 1px solid var(--line-2);
    background: color-mix(in srgb, var(--bg-elev) 96%, #0c0b0a 4%);
    backdrop-filter: blur(22px) saturate(1.15);
    -webkit-backdrop-filter: blur(22px) saturate(1.15);
    box-shadow: var(--shadow);
    z-index: 10060;
  }
  .rq-h {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    margin-bottom: 12px;
  }
  .rq-title { font-size: 15px; font-weight: 650; letter-spacing: -0.01em; }
  .rq-sub { margin-top: 4px; font-size: 12px; line-height: 16px; color: var(--text-2); }
  .rq-x {
    width: 28px;
    height: 28px;
    border-radius: 8px;
    display: grid;
    place-items: center;
    color: var(--text-3);
    flex: none;
  }
  .rq-x:hover { background: var(--bg-hover); color: var(--text); }
  .rq-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    min-height: 40px;
    font-size: 14px;
    font-weight: 550;
  }
  .rq-err { color: var(--danger); font-size: 12px; margin-top: 8px; }
  .rq-body {
    display: flex;
    gap: 14px;
    margin-top: 12px;
    align-items: center;
    flex-wrap: wrap;
  }
  .rq-qr {
    width: 128px;
    height: 128px;
    border-radius: 14px;
    overflow: hidden;
    background: #fff;
    flex: none;
  }
  .rq-qr :global(svg) { width: 100%; height: 100%; display: block; }
  .rq-txt { display: flex; flex-direction: column; gap: 6px; min-width: 0; flex: 1; }
  .rq-meta { font-size: 11px; line-height: 15px; color: var(--text-3); }
  .rq-url {
    font-size: 11px;
    word-break: break-all;
    text-align: left;
    padding: 8px 10px;
    border-radius: 10px;
    background: var(--bg);
    border: 1px solid var(--line);
    color: var(--text-2);
  }
  .rq-url:hover { color: var(--text); border-color: rgba(var(--accent-rgb), 0.4); }
</style>
