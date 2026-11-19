// Shared listen-together state so menus can offer "Add to jam queue".
import { api, type JamInfo } from './api';

/** The community relay Tarn uses by default for Listen along over the internet.
 *  Set this to your deployed server's https URL (see server/README.md). Empty
 *  hides the toggle and keeps Listen along on the local network only. */
export const PUBLIC_JAM_SERVER: string = 'https://jam.694206767.xyz';

export const jam = $state<{ info: JamInfo }>({
  info: { mode: 'off', code: null, guests: 0, following: null, rtt_ms: null, error: null },
});

export async function refreshJam() {
  try { jam.info = await api.jamStatus(); } catch { /* mobile / not available */ }
}
