<script lang="ts">
  import { t as tr } from '../i18n.svelte';
  import { loadConfig, saveConfig, lastfmLogin, listenbrainzCheck } from '../scrobble';

  let { onChange }: { onChange?: () => void } = $props();

  let cfg = $state(loadConfig());
  let lfmUser = $state(cfg.lastfm.user);
  let lfmPass = $state('');
  let busy = $state('');
  let msg = $state('');
  let err = $state('');

  function persist() {
    saveConfig($state.snapshot(cfg));
    onChange?.();
  }

  async function connectLastfm() {
    err = ''; msg = '';
    if (!cfg.lastfm.apiKey.trim() || !cfg.lastfm.secret.trim() || !lfmUser.trim() || !lfmPass) {
      err = 'Fill in API key, shared secret, username and password.';
      return;
    }
    busy = 'lfm';
    try {
      const r = await lastfmLogin(cfg.lastfm.apiKey.trim(), cfg.lastfm.secret.trim(), lfmUser.trim(), lfmPass);
      cfg.lastfm.apiKey = cfg.lastfm.apiKey.trim();
      cfg.lastfm.secret = cfg.lastfm.secret.trim();
      cfg.lastfm.session = r.session;
      cfg.lastfm.user = r.user;
      lfmPass = '';
      persist();
      msg = `Connected to Last.fm as ${r.user}.`;
    } catch (e) {
      err = `Last.fm: ${e instanceof Error ? e.message : e}`;
    } finally { busy = ''; }
  }

  function disconnectLastfm() {
    cfg.lastfm.session = '';
    cfg.lastfm.user = '';
    persist();
    msg = 'Disconnected from Last.fm.';
  }

  async function connectLb() {
    err = ''; msg = '';
    const token = cfg.listenbrainz.token.trim();
    if (!token) return;
    busy = 'lb';
    try {
      const user = await listenbrainzCheck(token);
      cfg.listenbrainz.token = token;
      persist();
      msg = user ? `ListenBrainz token OK (${user}).` : 'ListenBrainz token OK.';
    } catch (e) {
      err = `ListenBrainz: ${e instanceof Error ? e.message : e}`;
    } finally { busy = ''; }
  }

  function disconnectLb() {
    cfg.listenbrainz.token = '';
    persist();
    msg = 'Disconnected from ListenBrainz.';
  }
</script>

<div class="set-row set-stack">
  <span class="lb">{tr('Scrobbling')}
    <span class="set-meta">{tr('Send what you play to Last.fm and/or ListenBrainz (after half the song, or 4 minutes).')}</span>
  </span>

  <div class="scr">
    <strong>{tr('Last.fm')}</strong>
    {#if cfg.lastfm.session}
      <div class="line">{tr('Connected as {user}', { user: cfg.lastfm.user ?? '' })}
        <button class="btn ghost" type="button" onclick={disconnectLastfm}>{tr('Disconnect')}</button>
      </div>
    {:else}
      <input type="text" placeholder={tr('API key (last.fm/api/account/create)')} bind:value={cfg.lastfm.apiKey} />
      <input type="text" placeholder={tr('Shared secret')} bind:value={cfg.lastfm.secret} />
      <input type="text" placeholder={tr('Username')} bind:value={lfmUser} autocomplete="off" />
      <input type="password" placeholder={tr('Password (used once, not stored)')} bind:value={lfmPass} autocomplete="off" />
      <div class="line">
        <button class="btn ghost" type="button" disabled={busy === 'lfm'} onclick={connectLastfm}>
          {busy === 'lfm' ? tr('Connecting…') : tr('Connect')}
        </button>
      </div>
    {/if}
  </div>

  <div class="scr">
    <strong>{tr('ListenBrainz')}</strong>
    {#if cfg.listenbrainz.token}
      <div class="line">{tr('Token saved')}
        <button class="btn ghost" type="button" onclick={disconnectLb}>{tr('Disconnect')}</button>
      </div>
    {:else}
      <input type="text" placeholder={tr('User token (listenbrainz.org/settings)')} bind:value={cfg.listenbrainz.token} autocomplete="off" />
      <div class="line">
        <button class="btn ghost" type="button" disabled={busy === 'lb' || !cfg.listenbrainz.token.trim()} onclick={connectLb}>
          {busy === 'lb' ? tr('Checking…') : tr('Connect')}
        </button>
      </div>
    {/if}
  </div>

  {#if msg}<div class="ok">{msg}</div>{/if}
  {#if err}<div class="bad">{err}</div>{/if}
</div>

<style>
  .scr { display: flex; flex-direction: column; gap: 6px; }
  .scr input {
    width: 100%; box-sizing: border-box; padding: 8px 12px;
    font: inherit; font-size: 13px; color: var(--text);
    background: var(--bg-hover); border: 1px solid transparent;
    border-radius: 8px; outline: none;
  }
  .scr input:focus { border-color: rgba(var(--accent-rgb), 0.6); }
  .line { display: flex; align-items: center; gap: 10px; font-size: 13px; color: var(--text-2); }
  .ok { font-size: 12px; color: var(--text-2); }
  .bad { font-size: 12px; color: #ff6b6b; }
</style>
