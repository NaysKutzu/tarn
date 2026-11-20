<script lang="ts">
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { api } from '../api';
  import { t as tr } from '../i18n.svelte';
  import { jam, refreshJam, hostJam, jamPrefs, savePrefs, PUBLIC_JAM_SERVER } from '../jam.svelte';
  import { toast } from '../toast';
  import type { Settings } from '../types';
  import Hi from './Hi.svelte';
  import Toggle from './Toggle.svelte';
  import { UserGroupIcon, Cancel01Icon } from '../icons';

  let {
    placement = 'up',
    settings,
    onsave,
  }: {
    placement?: 'up' | 'down';
    /** App settings object (for the Discord join-button toggle). */
    settings: Settings;
    onsave: () => void;
  } = $props();

  let open = $state(false);
  let busy = $state(false);
  let copied = $state(false);
  let code = $state('');
  let invite = $state(false);
  let root = $state<HTMLElement | null>(null);
  let popEl = $state<HTMLElement | null>(null);
  let popPos = $state<{ top?: string; bottom?: string; right: string } | null>(null);

  function portal(node: HTMLElement) {
    document.body.appendChild(node);
    return { destroy() { node.remove(); } };
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

  /** A tarn://join link was opened: show it and let the user decide. */
  async function takeInvite() {
    try {
      const c = await api.jamTakeInvite();
      if (!c) return;
      code = c;
      invite = true;
      placePop();
      open = true;
    } catch { /* not available */ }
  }

  onMount(() => {
    void refreshJam();
    void takeInvite();
    let un: (() => void) | undefined;
    listen('jam-invite', () => void takeInvite()).then((u) => (un = u)).catch(() => {});
    const t = setInterval(() => { if (jam.info.mode !== 'off') void refreshJam(); }, 2000);
    return () => { clearInterval(t); un?.(); };
  });

  $effect(() => {
    if (!open) return;
    let ready = false;
    const arm = window.setTimeout(() => { ready = true; }, 0);
    const onDoc = (e: PointerEvent) => {
      if (!ready) return;
      const t = e.target as Node;
      if (root?.contains(t) || popEl?.contains(t)) return;
      open = false;
    };
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') open = false; };
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

  async function start() {
    busy = true;
    savePrefs();
    try { await hostJam(); } catch (e) { toast(String(e), 'error', 3000); } finally { busy = false; }
  }
  async function join() {
    busy = true;
    try { jam.info = await api.jamJoin(code.trim()); if (jam.info.mode === 'joined') { invite = false; code = ''; } }
    catch (e) { toast(String(e), 'error', 3000); } finally { busy = false; }
  }
  async function leave() {
    await api.jamLeave().catch(() => {});
    await refreshJam();
  }
  async function copy() {
    if (!jam.info.code) return;
    try {
      await navigator.clipboard.writeText(jam.info.code);
      copied = true;
      setTimeout(() => (copied = false), 1600);
    } catch { /* ignore */ }
  }
  function togglePublic(v: boolean) { jamPrefs.usePublic = v; savePrefs(); }
  const onWeb = $derived(!!jam.info.code?.startsWith('https://'));
</script>

<div class="jq" bind:this={root}>
  <button
    class="ibtn lg jq-btn"
    type="button"
    class:live={jam.info.mode !== 'off' && !open}
    title={tr('Listen with other people')}
    aria-label={tr('Listen with other people')}
    aria-expanded={open}
    aria-haspopup="dialog"
    onclick={(e) => { e.stopPropagation(); placePop(); open = !open; }}
  >
    <Hi icon={UserGroupIcon} size={22} />
    {#if jam.info.mode !== 'off' && !open}<i class="jq-dot" aria-hidden="true"></i>{/if}
  </button>
  {#if open}
    <div
      class="jq-pop"
      role="dialog"
      aria-label={tr('Listen with other people')}
      use:portal
      bind:this={popEl}
      style:position="fixed"
      style:top={popPos?.top}
      style:bottom={popPos?.bottom}
      style:right={popPos?.right}
    >
      <div class="jq-h">
        <div>
          <div class="jq-title">{tr('Listen with other people')}</div>
          <div class="jq-sub">
            {#if jam.info.mode === 'hosting'}
              {jam.info.guests === 1 ? tr('1 person is listening with you') : tr('{n} people are listening with you', { n: jam.info.guests })}
            {:else if jam.info.mode === 'joined'}
              {jam.info.error ?? `${tr('Following the host')}${jam.info.following ? ` · ${jam.info.following}` : ''}${jam.info.rtt_ms != null ? ` · ${jam.info.rtt_ms} ms` : ''}`}
            {:else}
              {tr('Same music, in sync. Friends follow your playback and can add songs to your queue')}
            {/if}
          </div>
        </div>
        <button class="jq-x" type="button" aria-label={tr('Close')} onclick={() => (open = false)}>
          <Hi icon={Cancel01Icon} size={18} />
        </button>
      </div>

      {#if jam.info.mode === 'hosting' && jam.info.code}
        <button class="jq-link" type="button" onclick={copy} title={tr('Copy link')}>{jam.info.code}</button>
        <div class="jq-actions">
          <button class="jq-big" type="button" onclick={copy}>{copied ? tr('Copied') : tr('Copy invite link')}</button>
          <button class="jq-sec" type="button" onclick={leave}>{tr('Stop')}</button>
        </div>
        {#if onWeb}
          <div class="jq-row">
            <span>
              {tr('Show a Join button on Discord')}
              <small>{tr('Everyone who sees your Discord status can join. Turn it off to invite people yourself')}</small>
            </span>
            <Toggle
              checked={!!settings.jam_discord_button}
              label={tr('Show a Join button on Discord')}
              onchange={(v) => { settings.jam_discord_button = v; onsave(); }}
            />
          </div>
        {:else}
          <p class="jq-meta">{tr('This link only works on your Wi-Fi. Turn on the public server to invite people anywhere')}</p>
        {/if}
        {#if jam.info.error}<p class="jq-err">{jam.info.error}</p>{/if}
      {:else if jam.info.mode === 'joined'}
        <div class="jq-actions">
          <button class="jq-sec" type="button" onclick={leave}>{tr('Leave')}</button>
        </div>
      {:else}
        {#if invite}
          <p class="jq-meta">{tr('You were invited to a listening session. Join it?')}</p>
        {:else}
          <button class="jq-big" type="button" disabled={busy} onclick={start}>{tr('Start listening together')}</button>
        {/if}
        <div class="jq-join">
          <input
            class="jq-in"
            type="text"
            placeholder={tr('Paste an invite link to join')}
            bind:value={code}
            onkeydown={(e) => { if (e.key === 'Enter' && code.trim()) join(); }}
          />
          <button class="jq-sec" type="button" disabled={busy || !code.trim()} onclick={join}>{tr('Join')}</button>
        </div>
        {#if PUBLIC_JAM_SERVER}
          <div class="jq-row">
            <span>
              {tr('Use the public server')}
              <small>{tr('Lets friends join from anywhere. Off keeps it on your network; the server sees your IP and what is playing')}</small>
            </span>
            <Toggle checked={jamPrefs.usePublic} label={tr('Use the public server')} onchange={togglePublic} />
          </div>
        {/if}
        {#if jam.info.error}<p class="jq-err">{jam.info.error}</p>{/if}
      {/if}
    </div>
  {/if}
</div>

<style>
  .jq { position: relative; flex: 0 0 auto; z-index: 40; }
  .jq-btn { position: relative; }
  .jq-btn.live { color: inherit; }
  .jq-dot {
    position: absolute; top: 8px; right: 8px; width: 6px; height: 6px; border-radius: 50%;
    background: rgb(var(--accent-rgb)); box-shadow: 0 0 0 2px var(--bg-player, #141312);
  }
  .jq-pop {
    width: min(380px, calc(100vw - 24px));
    padding: 14px; border-radius: 16px; border: 1px solid var(--line-2);
    background: color-mix(in srgb, var(--bg-elev) 96%, #0c0b0a 4%);
    backdrop-filter: blur(22px) saturate(1.15); -webkit-backdrop-filter: blur(22px) saturate(1.15);
    box-shadow: var(--shadow); z-index: 10060;
    display: flex; flex-direction: column; gap: 12px;
  }
  .jq-h { display: flex; align-items: flex-start; gap: 10px; }
  .jq-h > div { flex: 1; min-width: 0; }
  .jq-title { font-size: 15px; font-weight: 650; letter-spacing: -0.01em; }
  .jq-sub { margin-top: 4px; font-size: 12px; line-height: 16px; color: var(--text-2); }
  .jq-x { width: 28px; height: 28px; border-radius: 8px; display: grid; place-items: center; color: var(--text-3); flex: none; }
  .jq-x:hover { background: var(--bg-hover); color: var(--text); }
  .jq-big {
    padding: 12px 14px; border-radius: 12px; font-weight: 650; font-size: 14px;
    background: rgb(var(--accent-rgb)); color: #1a1209; flex: 1;
  }
  .jq-big:hover { filter: brightness(1.06); }
  .jq-big:disabled, .jq-sec:disabled { opacity: 0.5; }
  .jq-sec { padding: 12px 14px; border-radius: 12px; font-weight: 600; font-size: 13px; border: 1px solid var(--line-2); }
  .jq-sec:hover { background: var(--bg-hover); }
  .jq-actions { display: flex; gap: 8px; }
  .jq-link {
    font-size: 11px; word-break: break-all; text-align: left; padding: 8px 10px; border-radius: 10px;
    background: var(--bg); border: 1px solid var(--line); color: var(--text-2);
  }
  .jq-link:hover { color: var(--text); border-color: rgba(var(--accent-rgb), 0.4); }
  .jq-join { display: flex; gap: 8px; }
  .jq-in {
    flex: 1; min-width: 0; padding: 10px 12px; border-radius: 10px; border: 1px solid var(--line-2);
    background: transparent; color: inherit; font: inherit; font-size: 13px;
  }
  .jq-row { display: flex; align-items: center; justify-content: space-between; gap: 12px; font-size: 13.5px; font-weight: 550; }
  .jq-row small { display: block; margin-top: 3px; font-size: 11px; line-height: 15px; font-weight: 400; color: var(--text-3); }
  .jq-meta { font-size: 12px; line-height: 16px; color: var(--text-2); margin: 0; }
  .jq-err { color: var(--danger); font-size: 12px; margin: 0; }
</style>
