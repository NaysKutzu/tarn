// Shared listen-together state so menus can offer "Add to jam queue".
import { api, type JamInfo } from './api';

export const jam = $state<{ info: JamInfo }>({
  info: { mode: 'off', code: null, guests: 0, following: null, rtt_ms: null, error: null },
});

export async function refreshJam() {
  try { jam.info = await api.jamStatus(); } catch { /* mobile / not available */ }
}
