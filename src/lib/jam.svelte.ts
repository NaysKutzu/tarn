// Shared listen-together state so menus can offer "Add to jam queue".
import { api, type JamInfo } from './api';

/** The community relay Tarn uses by default for Listen along over the internet.
 *  Set this to your deployed server's https URL (see server/README.md). Empty
 *  hides the toggle and keeps Listen along on the local network only. */
export const PUBLIC_JAM_SERVER: string = 'https://jam.694206767.xyz';

export const jam = $state<{ info: JamInfo }>({
  info: { mode: 'off', code: null, guests: 0, people: [], host_name: null, following: null, rtt_ms: null, error: null },
});

export async function refreshJam() {
  try { jam.info = await api.jamStatus(); } catch { /* mobile / not available */ }
}

const load = (k: string) => { try { return localStorage.getItem(k) ?? ''; } catch { return ''; } };
const save = (k: string, v: string) => { try { localStorage.setItem(k, v); } catch { /* ignore */ } };

/** Listen along preferences, shared by Settings and the player-bar button. */
export const jamPrefs = $state({
  usePublic: load('jam.public') !== '0',
  server: load('jam.server'),
  password: load('jam.password'),
  /** Nickname shown to others; empty = use the account name. */
  name: load('jam.name'),
  /** Set by the app from the signed-in account. */
  accountName: '',
});

/** What other people see you as. */
export function jamName(): string {
  return jamPrefs.name.trim() || jamPrefs.accountName.trim() || 'Guest';
}

export function savePrefs() {
  save('jam.public', jamPrefs.usePublic ? '1' : '0');
  save('jam.server', jamPrefs.server.trim());
  save('jam.password', jamPrefs.password);
  save('jam.name', jamPrefs.name.trim());
}

/** Which server a new session is hosted on ('' = this network only). */
export function jamTarget(): string {
  return jamPrefs.server.trim() || (jamPrefs.usePublic ? PUBLIC_JAM_SERVER : '');
}

export async function hostJam() {
  const server = jamTarget();
  jam.info = await api.jamHost(server, jamPrefs.server.trim() ? jamPrefs.password : '', jamName());
  return jam.info;
}
