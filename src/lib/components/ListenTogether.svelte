<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '../api';
  import { t as tr } from '../i18n.svelte';
  import { jam, refreshJam, PUBLIC_JAM_SERVER, jamPrefs, jamTarget, savePrefs, hostJam, jamName, expandJoinCode, fmtShort } from '../jam.svelte';
  import { toast } from '../toast';

  let code = $state('');
  let busy = $state(false);
  // Preferences are shared with the player-bar button (jamPrefs).
  const target = $derived(jamTarget());
  function togglePublic() { jamPrefs.usePublic = !jamPrefs.usePublic; savePrefs(); }

  onMount(() => {
    void refreshJam();
    const t = setInterval(() => { if (jam.info.mode !== 'off') void refreshJam(); }, 2000);
    return () => clearInterval(t);
  });

  async function host() {
    busy = true;
    savePrefs();
    try { await hostJam(); } catch (e) { toast(String(e), 'error', 3000); } finally { busy = false; }
  }
  async function join() {
    busy = true;
    try { jam.info = await api.jamJoin(expandJoinCode(code), jamName()); } catch (e) { toast(String(e), 'error', 3000); } finally { busy = false; }
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
  <span class="lb">{tr('Listen together')} <span class="beta">Beta</span>
    <span class="set-meta">
      {#if jam.info.mode === 'hosting'}
        {jam.info.guests === 1
          ? tr('Hosting · 1 guest listening. Guests follow your playback and can add songs to your queue')
          : tr('Hosting · {n} guests listening. Guests follow your playback and can add songs to your queue', { n: jam.info.guests })}
      {:else if jam.info.mode === 'joined'}
        {#if jam.info.error}{jam.info.error}{:else}{tr('Following the host')}{jam.info.following ? ` · ${jam.info.following}` : ''}{jam.info.rtt_ms != null ? ` · ${jam.info.rtt_ms} ms` : ''}{/if}
      {:else}
        {tr('Play the same music in sync with other Tarn apps on this network')}
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
    <span class="t-meta">{target ? 'Share this join code with anyone:' : 'Share this join code with people on the same Wi-Fi:'}</span>
    {#if jam.info.short}<div class="jt-row"><code class="jt-short">{fmtShort(jam.info.short)}</code></div>{/if}
    <div class="jt-row"><code class="jt-code">{jam.info.code}</code><button class="btn ghost sm" type="button" onclick={copy}>Copy</button></div>
    {#if jam.info.error}<span class="t-meta jt-err">{jam.info.error}</span>{/if}
  </div>
{:else if jam.info.mode === 'off'}
  <div class="jt">
    <div class="jt-row">
      <input class="jt-in" type="text" placeholder="Code (e.g. 482 913) or invite link" bind:value={code} onkeydown={(e) => { if (e.key === 'Enter' && code.trim()) join(); }} />
      <button class="btn ghost sm" type="button" disabled={busy || !code.trim()} onclick={join}>Join</button>
    </div>
    {#if PUBLIC_JAM_SERVER}
      <label class="jt-row">
        <input type="checkbox" checked={jamPrefs.usePublic} onchange={togglePublic} />
        <span class="t-meta">Use the public server so friends anywhere can join. Off keeps it on this network only; the server sees your IP and what's playing.</span>
      </label>
    {/if}
    <span class="t-meta">Or use your own jam server (optional, overrides the above):</span>
    <div class="jt-row">
      <input class="jt-in" type="text" placeholder="https://jam.example.com" bind:value={jamPrefs.server} />
      <input class="jt-in" type="password" placeholder="Server password" autocomplete="off" bind:value={jamPrefs.password} />
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
  .jt-short { font-size: 22px; font-weight: 700; letter-spacing: 0.12em; user-select: all; }
</style>
