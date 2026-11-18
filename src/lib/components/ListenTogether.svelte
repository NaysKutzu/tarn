<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../api';
  import { jam, refreshJam } from '../jam.svelte';
  import { toast } from '../toast';

  let code = $state('');
  let busy = $state(false);

  onMount(() => {
    void refreshJam();
    const t = setInterval(() => { if (jam.info.mode !== 'off') void refreshJam(); }, 2000);
    return () => clearInterval(t);
  });

  async function host() {
    busy = true;
    try { jam.info = await api.jamHost(); } catch (e) { toast(String(e), 'error', 3000); } finally { busy = false; }
  }
  async function join() {
    busy = true;
    try { jam.info = await api.jamJoin(code); } catch (e) { toast(String(e), 'error', 3000); } finally { busy = false; }
  }
  async function leave() {
    await api.jamLeave().catch(() => {});
    await refreshJam();
  }
  async function copy() {
    if (!jam.info.code) return;
    try { await navigator.clipboard.writeText(jam.info.code); toast('Join code copied', 'info', 1800); } catch { /* ignore */ }
  }
</script>

<div class="set-row">
  <span class="lb">Listen together <span class="beta">Beta</span>
    <span class="t-meta" style="display:block;font-weight:450;margin-top:2px">
      {#if jam.info.mode === 'hosting'}
        Hosting · {jam.info.guests} {jam.info.guests === 1 ? 'guest' : 'guests'} listening. Guests follow your playback and can add songs to your queue
      {:else if jam.info.mode === 'joined'}
        {#if jam.info.error}{jam.info.error}{:else}Following the host{jam.info.following ? ` · ${jam.info.following}` : ''}{jam.info.rtt_ms != null ? ` · ${jam.info.rtt_ms} ms` : ''}{/if}
      {:else}
        Play the same music in sync with other Tarn apps on this network
      {/if}
    </span>
  </span>
  {#if jam.info.mode === 'off'}
    <button class="btn ghost sm" type="button" disabled={busy} onclick={host}>Host</button>
  {:else}
    <button class="btn ghost sm" type="button" onclick={leave}>{jam.info.mode === 'hosting' ? 'Stop' : 'Leave'}</button>
  {/if}
</div>
{#if jam.info.mode === 'hosting' && jam.info.code}
  <div class="jt">
    <span class="t-meta">Share this join code with people on the same Wi-Fi:</span>
    <div class="jt-row"><code class="jt-code">{jam.info.code}</code><button class="btn ghost sm" type="button" onclick={copy}>Copy</button></div>
  </div>
{:else if jam.info.mode === 'off'}
  <div class="jt">
    <div class="jt-row">
      <input class="jt-in" type="text" placeholder="Join code, e.g. 192.168.1.5:47811#…" bind:value={code} onkeydown={(e) => { if (e.key === 'Enter' && code.trim()) join(); }} />
      <button class="btn ghost sm" type="button" disabled={busy || !code.trim()} onclick={join}>Join</button>
    </div>
    {#if jam.info.error}<span class="t-meta jt-err">{jam.info.error}</span>{/if}
  </div>
{/if}

<style>
  .beta { font-size: 10.5px; font-weight: 700; letter-spacing: 0.04em; text-transform: uppercase; padding: 2px 6px; border-radius: 999px; background: var(--accent-dim); color: var(--accent); margin-left: 6px; vertical-align: middle; }
  .jt { display: flex; flex-direction: column; gap: 8px; padding: 0 0 14px; }
  .jt-row { display: flex; gap: 8px; align-items: center; }
  .jt-code { font-size: 12px; word-break: break-all; user-select: all; flex: 1; }
  .jt-in { flex: 1; padding: 8px 10px; border-radius: 8px; border: 1px solid var(--line-2); background: transparent; color: inherit; font: inherit; font-size: 13px; }
  .jt-err { color: var(--danger); }
</style>
