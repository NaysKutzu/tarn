export interface Track {
  id: string;
  title: string;
  artists: string[];
  artist_ids: string[];
  album: string | null;
  album_id: string | null;
  duration: number | null;
  cover: string | null;
  is_video: boolean;
}

export interface ArtistCard {
  id: string;
  name: string;
  cover: string | null;
  subscribers: number | null;
}

export interface PlaylistCard {
  id: string;
  name: string;
  cover: string | null;
  /** Up to 4 track covers for a 2×2 mosaic when there's no custom art. */
  covers?: string[];
  track_count: number | null;
  subtitle: string | null;
  year: number | null;
  album_type: string | null;
  author: string | null;
  from_ytm: boolean | null;
  color: string | null;
  params: string | null;
  browse_id: string | null;
}

export interface MoodChip {
  title: string;
  params: string | null;
  browse_id: string | null;
  color: string | null;
  selected: boolean;
}

export interface ShelfItem {
  kind: string;
  id: string;
  title: string;
  subtitle: string | null;
  cover: string | null;
  artists: string[];
  artist_ids: string[];
  album: string | null;
  album_id: string | null;
  duration: number | null;
  playlist_id: string | null;
  color: string | null;
  browse_id: string | null;
  params: string | null;
  year: number | null;
  album_type: string | null;
  track_count: number | null;
  is_video: boolean;
}

export interface Shelf {
  id: string | null;
  title: string;
  subtitle: string | null;
  kind: string;
  items: ShelfItem[];
}

export interface Home {
  chips: MoodChip[];
  shelves: Shelf[];
  continuation: string | null;
  signed_in: boolean;
}

export interface Explore {
  links: MoodChip[];
  shelves: Shelf[];
  moods: ShelfItem[];
}

export interface SearchResults {
  query: string;
  top: ShelfItem | null;
  songs: Track[];
  videos: Track[];
  albums: PlaylistCard[];
  artists: ArtistCard[];
  playlists: PlaylistCard[];
  community: PlaylistCard[];
}

export interface ArtistPage {
  id: string;
  name: string;
  header: string | null;
  cover: string | null;
  description: string | null;
  subscribers: number | null;
  tracks: Track[];
  albums: PlaylistCard[];
  singles: PlaylistCard[];
  videos: Track[];
  playlists: PlaylistCard[];
  similar: ArtistCard[];
  radio_id: string | null;
  tracks_playlist_id: string | null;
  videos_playlist_id: string | null;
  subscribed: boolean;
}

export interface Collection {
  kind: string;
  id: string;
  playlist_id: string | null;
  name: string;
  cover: string | null;
  artists: string[];
  artist_ids: string[];
  author: string | null;
  year: number | null;
  description: string | null;
  track_count: number | null;
  album_type: string | null;
  tracks: Track[];
  related: PlaylistCard[];
  owned: boolean;
  saved: boolean;
}

export interface LyricLine {
  text: string;
  start_ms: number | null;
}

export interface Lyrics {
  source: string | null;
  lines: LyricLine[];
  synced: boolean;
}

export interface Related {
  tracks: Track[];
  versions: Track[];
  albums: PlaylistCard[];
  artists: ArtistCard[];
  playlists: PlaylistCard[];
}

export interface AccountInfo {
  name: string;
  photo: string | null;
  handle: string | null;
  page_id?: string | null;
}

export interface Library {
  signed_in: boolean;
  mock: boolean;
  account: AccountInfo | null;
  playlists: PlaylistCard[];
  liked: Track[];
  albums: PlaylistCard[];
  artists: ArtistCard[];
  recents: Track[];
}

export interface AuthStatus {
  signed_in: boolean;
  mock: boolean;
  account: AccountInfo | null;
  store: string | null;
}

export interface Settings {
  quality: string;
  language: string;
  region: string;
  notifications: boolean;
  /** Show the current track on Discord Rich Presence. */
  discord_presence: boolean;
  autoplay: boolean;
  /** Resume and play the last session when the app starts. */
  resume_on_start: boolean;
  cache_max_mb: number;
  /** Only downloaded / cached songs play; nothing touches the network. */
  offline_mode: boolean;
  crossfade: boolean;
  crossfade_seconds: number;
  album_continuity: boolean;
  /** Also fade when you skip or pick another song. */
  crossfade_manual: boolean;
  eq_enabled: boolean;
  /** Five-band gains in dB, −12…+12. */
  eq_bands: [number, number, number, number, number];
  /** "equal_power" | "linear" | "logarithmic" | "exponential" | "s_curve" */
  crossfade_curve_out: string;
  crossfade_curve_in: string;
  /** Percent (10-100) of the crossfade each side spends moving. */
  crossfade_out_len: number;
  crossfade_in_len: number;
  /** Trim trailing/leading silence around transitions. */
  crossfade_smart: boolean;
  /** "off" | "lowpass" | "highpass" | "sweep" */
  crossfade_filter: string;
  bpm_match: boolean;
  /** Largest tempo change in percent. */
  bpm_max_change: number;
  bpm_beat_align: boolean;
  bpm_return_seconds: number;
  /** Keep a local listening history for the Stats page. */
  listening_stats: boolean;
}

export interface PlayerState {
  track: Track | null;
  playing: boolean;
  loading: boolean;
  buffering: boolean;
  buffered: number;
  position: number;
  duration: number;
  volume: number;
  queue: Track[];
  index: number;
  error: string | null;
  accent: [number, number, number] | null;
  shuffle: boolean;
  repeat: string;
  liked: boolean;
  disliked: boolean;
  autoplay: boolean;
  /** Periodic update without the queue: keep the one already shown. */
  queue_same?: boolean;
}

export interface GenrePage {
  id: string;
  name: string;
  sections: Shelf[];
}

export interface LikeState {
  liked: boolean;
  disliked: boolean;
}

export type View =
  | { id: 'home' }
  | { id: 'explore' }
  | { id: 'search' }
  | { id: 'library' }
  | { id: 'settings' }
  | { id: 'artist'; artistId: string }
  | { id: 'album'; albumId: string }
  | { id: 'playlist'; playlistId: string }
  | { id: 'genre'; genreId: string; title?: string }
  | { id: 'charts' }
  | { id: 'new-releases' }
  | { id: 'history' }
  | { id: 'downloads' }
  | { id: 'stats' };

import type { IconSvgElement } from '@hugeicons/svelte';

export type MenuEntry =
  | { sep: true }
  | {
      label: string;
      icon?: IconSvgElement;
      onclick?: () => void;
      children?: MenuEntry[];
      danger?: boolean;
      disabled?: boolean;
    };

/** Right-click target that is a whole playlist / album / artist. */
export interface CollTarget {
  kind: 'playlist' | 'album' | 'artist';
  id: string;
  title: string;
  cover: string | null;
  subtitle?: string | null;
}

export interface CtxTarget {
  coll?: CollTarget;
  x: number;
  y: number;
  track?: Track;
  item?: ShelfItem;
  albumId?: string;
  artistId?: string;
  playlistId?: string;
}

export interface UiSession {
  view: View | null;
  search_q: string | null;
  scroll: number | null;
}

export interface SavedSession {
  queue: Track[];
  index: number;
  position: number;
  volume: number;
  shuffle: boolean;
  repeat: string;
  view: View | null;
  search_q: string | null;
  scroll: number | null;
}

export interface NetState {
  /** User switched offline mode on. */
  mode: boolean;
  /** Connection looks dead. */
  net_down: boolean;
  /** Effective: mode || net_down. */
  offline: boolean;
}

export interface OfflineSourceInfo {
  id: string;
  kind: 'playlist' | 'album' | 'liked';
  name: string;
  cover: string | null;
  subtitle: string | null;
  total: number;
  done: number;
  failed: number;
}

export interface OfflineStatus {
  sources: OfflineSourceInfo[];
  queued: number;
  active: { id: string; title: string }[];
  failed: number;
  bytes: number;
  downloaded: string[];
  cached: string[];
  paused: boolean;
}

export interface OfflineSource {
  id: string;
  kind: 'playlist' | 'album' | 'liked';
  name: string;
  cover: string | null;
  subtitle: string | null;
}
