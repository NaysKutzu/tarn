import { listen } from '@tauri-apps/api/event';
import { api } from './api';
import type { OfflineStatus, OfflineSourceInfo } from './types';

/**
 * Shared offline state: is the app offline right now, what is downloaded,
 * and how far the download queue has got. Everything in the UI that needs
 * to dim, badge or hide things reads from here.
 */
export const off = $state({
  /** User toggled offline mode. */
  mode: false,
  /** Connection looks dead. */
  netDown: false,
  /** Effective: mode || netDown. */
  offline: false,
  status: null as OfflineStatus | null,
  /** Pinned + on disk. */
  downloaded: new Set<string>(),
  /** Anything that can play with no network (downloaded or cached). */
  playable: new Set<string>(),
});

let started = false;

export async function refreshOffline() {
  try {
    const s = await api.offlineStatus();
    off.status = s;
    off.downloaded = new Set(s.downloaded);
    off.playable = new Set([...s.downloaded, ...s.cached]);
  } catch { /* backend not ready yet */ }
}

export async function initOffline(onNet?: (offline: boolean, was: boolean) => void) {
  if (started) return;
  started = true;
  try {
    const n = await api.netState();
    off.mode = n.mode;
    off.netDown = n.net_down;
    off.offline = n.offline;
  } catch { /* ignore */ }
  await refreshOffline();
  let pending = false;
  await listen('offline', () => {
    // Coalesce bursts (one event per finished track).
    if (pending) return;
    pending = true;
    setTimeout(() => { pending = false; refreshOffline(); }, 250);
  });
  await listen<{ mode: boolean; net_down: boolean; offline: boolean }>('net', (e) => {
    const was = off.offline;
    off.mode = e.payload.mode;
    off.netDown = e.payload.net_down;
    off.offline = e.payload.offline;
    refreshOffline();
    onNet?.(off.offline, was);
  });
}

export function sourceInfo(id: string | null | undefined): OfflineSourceInfo | null {
  if (!id) return null;
  return off.status?.sources.find((s) => s.id === id) ?? null;
}

export function fmtBytes(n: number): string {
  if (n < 1024 * 1024) return `${Math.max(1, Math.round(n / 1024))} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(n < 100 * 1024 * 1024 ? 1 : 0)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}
