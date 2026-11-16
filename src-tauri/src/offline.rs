//! Offline library: playlists / albums / liked songs downloaded to disk.
//!
//! Audio lives in the shared [`AudioCache`] but is *pinned*, so the LRU never
//! evicts it. This module adds what the cache can't know: which collections
//! were downloaded, the track metadata needed to browse them with no
//! network, the cover art, and a small download queue that survives
//! restarts (anything pinned but not yet on disk is resumed at startup).

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::cache::AudioCache;
use crate::models::{Collection, Track};

const FILE: &str = "offline.json";
/// Parallel track downloads. Two keeps the pipe busy without starving playback.
const WORKERS: usize = 2;
const MAX_ATTEMPTS: u8 = 3;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Source {
    pub id: String,
    /// "playlist" | "album" | "liked"
    pub kind: String,
    pub name: String,
    pub cover: Option<String>,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub track_ids: Vec<String>,
    #[serde(default)]
    pub added: u64,
}

#[derive(Default, Serialize, Deserialize)]
struct Data {
    sources: Vec<Source>,
    tracks: HashMap<String, Track>,
}

#[derive(Serialize, Clone, Debug)]
pub struct SourceInfo {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub cover: Option<String>,
    pub subtitle: Option<String>,
    pub total: usize,
    pub done: usize,
    pub failed: usize,
}

#[derive(Serialize, Clone, Debug)]
pub struct ActiveItem {
    pub id: String,
    pub title: String,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Status {
    pub sources: Vec<SourceInfo>,
    /// Tracks waiting to download (not counting the ones in flight).
    pub queued: usize,
    pub active: Vec<ActiveItem>,
    pub failed: usize,
    /// Bytes used by downloaded audio.
    pub bytes: u64,
    /// Pinned + fully on disk.
    pub downloaded: Vec<String>,
    /// Fully on disk but only cached (may be evicted later).
    pub cached: Vec<String>,
    pub paused: bool,
}

pub struct Offline {
    dir: PathBuf,
    data: Mutex<Data>,
    queue: Mutex<VecDeque<String>>,
    attempts: Mutex<HashMap<String, u8>>,
    failed: Mutex<HashSet<String>>,
    active: Mutex<Vec<String>>,
    wake: tokio::sync::Notify,
    paused: AtomicBool,
    running: AtomicUsize,
}

pub fn root_dir() -> PathBuf {
    dirs::data_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("tarn")
        .join("offline")
}

/// Stable file name for a remote cover URL (FNV-1a, 64 bit). The webview
/// computes the same value to ask for the local copy.
pub fn cover_key(url: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in url.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Offline {
    pub fn open(cache: &AudioCache) -> Arc<Self> {
        let dir = root_dir();
        let _ = fs::create_dir_all(dir.join("covers"));
        let data: Data = fs::read_to_string(dir.join(FILE))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        // Pins live in the cache index; make sure every source track is pinned.
        cache.pin(data.sources.iter().flat_map(|s| s.track_ids.iter().cloned()));
        let me = Arc::new(Self {
            dir,
            data: Mutex::new(data),
            queue: Mutex::new(VecDeque::new()),
            attempts: Mutex::new(HashMap::new()),
            failed: Mutex::new(HashSet::new()),
            active: Mutex::new(Vec::new()),
            wake: tokio::sync::Notify::new(),
            paused: AtomicBool::new(false),
            running: AtomicUsize::new(0),
        });
        me.requeue_missing(cache);
        me
    }

    pub fn covers_dir(&self) -> PathBuf {
        self.dir.join("covers")
    }

    fn persist(&self) {
        let g = self.data.lock().unwrap();
        let tmp = self.dir.join(".offline.json.tmp");
        if let Ok(bytes) = serde_json::to_vec(&*g) {
            if fs::write(&tmp, bytes).is_ok() {
                let _ = fs::rename(&tmp, self.dir.join(FILE));
            }
        }
    }

    /// Queue every pinned track whose audio isn't on disk yet.
    pub fn requeue_missing(&self, cache: &AudioCache) {
        let ids: Vec<String> = {
            let g = self.data.lock().unwrap();
            let mut seen = HashSet::new();
            g.sources
                .iter()
                .flat_map(|s| s.track_ids.iter())
                .filter(|id| seen.insert((*id).clone()))
                .cloned()
                .collect()
        };
        let mut q = self.queue.lock().unwrap();
        let active = self.active.lock().unwrap();
        for id in ids {
            if !cache.has_complete(&id) && !q.contains(&id) && !active.contains(&id) {
                q.push_back(id);
            }
        }
        drop(q);
        self.wake.notify_waiters();
        self.wake.notify_one();
    }

    /// Register a collection for offline use and start downloading it.
    pub fn add_source(&self, cache: &AudioCache, mut src: Source, tracks: Vec<Track>) {
        let mut ids = Vec::new();
        let mut seen = HashSet::new();
        {
            let mut g = self.data.lock().unwrap();
            for mut t in tracks {
                if t.id.is_empty() || !seen.insert(t.id.clone()) {
                    continue;
                }
                t.ensure_cover();
                ids.push(t.id.clone());
                g.tracks.insert(t.id.clone(), t);
            }
            src.track_ids = ids.clone();
            src.added = now();
            g.sources.retain(|s| s.id != src.id);
            g.sources.push(src);
        }
        cache.pin(ids.iter().cloned());
        {
            let mut f = self.failed.lock().unwrap();
            let mut a = self.attempts.lock().unwrap();
            for id in &ids {
                f.remove(id);
                a.remove(id);
            }
        }
        self.persist();
        self.requeue_missing(cache);
    }

    /// Forget a source. Tracks no other source needs are deleted from disk.
    pub fn remove_source(&self, cache: &AudioCache, id: &str) {
        let orphans: Vec<String> = {
            let mut g = self.data.lock().unwrap();
            let Some(pos) = g.sources.iter().position(|s| s.id == id) else {
                return;
            };
            let gone = g.sources.remove(pos);
            let still: HashSet<&String> =
                g.sources.iter().flat_map(|s| s.track_ids.iter()).collect();
            let orphans: Vec<String> = gone
                .track_ids
                .into_iter()
                .filter(|t| !still.contains(t))
                .collect();
            for t in &orphans {
                g.tracks.remove(t);
            }
            orphans
        };
        {
            let mut q = self.queue.lock().unwrap();
            q.retain(|x| !orphans.contains(x));
        }
        for t in &orphans {
            cache.remove_file(t);
            self.failed.lock().unwrap().remove(t);
            self.attempts.lock().unwrap().remove(t);
        }
        self.persist();
        self.gc_covers();
    }

    pub fn remove_all(&self, cache: &AudioCache) {
        let ids: Vec<String> = self.data.lock().unwrap().sources.iter().map(|s| s.id.clone()).collect();
        for id in ids {
            self.remove_source(cache, &id);
        }
        self.queue.lock().unwrap().clear();
    }

    /// Stop downloading a source but keep what already arrived. With nothing
    /// finished yet the source is dropped entirely.
    pub fn cancel_source(&self, cache: &AudioCache, id: &str) {
        let pending: Vec<String> = {
            let mut g = self.data.lock().unwrap();
            let Some(pos) = g.sources.iter().position(|s| s.id == id) else {
                return;
            };
            let (done, pending): (Vec<String>, Vec<String>) = g.sources[pos]
                .track_ids
                .iter()
                .cloned()
                .partition(|t| cache.has_complete(t));
            if done.is_empty() {
                drop(g);
                self.remove_source(cache, id);
                return;
            }
            g.sources[pos].track_ids = done;
            let still: HashSet<String> =
                g.sources.iter().flat_map(|s| s.track_ids.iter().cloned()).collect();
            let orphans: Vec<String> = pending.into_iter().filter(|t| !still.contains(t)).collect();
            for t in &orphans {
                g.tracks.remove(t);
            }
            orphans
        };
        self.queue.lock().unwrap().retain(|x| !pending.contains(x));
        for t in &pending {
            self.failed.lock().unwrap().remove(t);
            self.attempts.lock().unwrap().remove(t);
            cache.remove_file(t);
        }
        self.persist();
        self.gc_covers();
    }

    pub fn cancel_all(&self, cache: &AudioCache) {
        let ids: Vec<String> = self.data.lock().unwrap().sources.iter().map(|s| s.id.clone()).collect();
        for id in ids {
            let unfinished = {
                let g = self.data.lock().unwrap();
                g.sources.iter().find(|s| s.id == id).is_some_and(|s| s.track_ids.iter().any(|t| !cache.has_complete(t)))
            };
            if unfinished {
                self.cancel_source(cache, &id);
            }
        }
        self.queue.lock().unwrap().clear();
    }

    pub fn cancel_pending(&self) {
        self.queue.lock().unwrap().clear();
    }

    pub fn set_paused(&self, on: bool) {
        self.paused.store(on, Ordering::Relaxed);
        if !on {
            self.wake.notify_waiters();
            self.wake.notify_one();
        }
    }

    /// Retry everything that failed.
    pub fn retry_failed(&self, cache: &AudioCache) {
        self.failed.lock().unwrap().clear();
        self.attempts.lock().unwrap().clear();
        self.requeue_missing(cache);
    }

    pub fn has_source(&self, id: &str) -> bool {
        self.data.lock().unwrap().sources.iter().any(|s| s.id == id)
    }

    pub fn track(&self, id: &str) -> Option<Track> {
        self.data.lock().unwrap().tracks.get(id).cloned()
    }

    /// Browse a downloaded source with no network: tracks that are on disk,
    /// in their original order.
    pub fn collection(&self, cache: &AudioCache, id: &str) -> Option<Collection> {
        let g = self.data.lock().unwrap();
        // The Liked Music playlist is stored under one id whatever the view uses.
        let id = if matches!(id, "LM" | "VLLM") { "liked" } else { id };
        let s = g.sources.iter().find(|s| s.id == id)?;
        let tracks: Vec<Track> = s
            .track_ids
            .iter()
            .filter(|t| cache.has_complete(t))
            .filter_map(|t| g.tracks.get(t).cloned())
            .collect();
        Some(Collection {
            kind: if s.kind == "album" { "album".into() } else { "playlist".into() },
            id: s.id.clone(),
            playlist_id: Some(s.id.clone()),
            name: s.name.clone(),
            cover: s.cover.clone(),
            author: s.subtitle.clone(),
            track_count: Some(tracks.len() as u64),
            tracks,
            ..Default::default()
        })
    }

    /// Every downloaded track, newest source first (for "Downloaded songs").
    pub fn all_tracks(&self, cache: &AudioCache) -> Vec<Track> {
        let g = self.data.lock().unwrap();
        let mut order: Vec<&Source> = g.sources.iter().collect();
        order.sort_by(|a, b| b.added.cmp(&a.added));
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for s in order {
            for id in &s.track_ids {
                if cache.has_complete(id) && seen.insert(id.clone()) {
                    if let Some(t) = g.tracks.get(id) {
                        out.push(t.clone());
                    }
                }
            }
        }
        out
    }

    pub fn status(&self, cache: &AudioCache) -> Status {
        let g = self.data.lock().unwrap();
        let failed = self.failed.lock().unwrap();
        let sources = g
            .sources
            .iter()
            .map(|s| SourceInfo {
                id: s.id.clone(),
                kind: s.kind.clone(),
                name: s.name.clone(),
                cover: s.cover.clone(),
                subtitle: s.subtitle.clone(),
                total: s.track_ids.len(),
                done: s.track_ids.iter().filter(|t| cache.has_complete(t)).count(),
                failed: s
                    .track_ids
                    .iter()
                    .filter(|t| failed.contains(*t) && !cache.has_complete(t))
                    .count(),
            })
            .collect();
        let pinned = cache.pinned_ids();
        let (downloaded, cached_unpinned): (Vec<String>, Vec<String>) = {
            let d: Vec<String> = pinned.into_iter().filter(|i| cache.has_complete(i)).collect();
            (d, cache.complete_unpinned())
        };
        let active: Vec<ActiveItem> = self
            .active
            .lock()
            .unwrap()
            .iter()
            .map(|id| ActiveItem {
                id: id.clone(),
                title: g.tracks.get(id).map(|t| t.title.clone()).unwrap_or_default(),
            })
            .collect();
        Status {
            sources,
            queued: self.queue.lock().unwrap().len(),
            active,
            failed: failed.len(),
            bytes: cache.pinned_bytes(),
            downloaded,
            cached: cached_unpinned,
            paused: self.paused.load(Ordering::Relaxed),
        }
    }

    // -- covers -----------------------------------------------------------

    pub fn cover_path(&self, key: &str) -> PathBuf {
        self.dir.join("covers").join(key)
    }

    pub fn has_cover(&self, url: &str) -> bool {
        self.cover_path(&cover_key(url)).is_file()
    }

    pub async fn fetch_cover(&self, http: &reqwest::Client, url: &str) {
        if !url.starts_with("http") || self.has_cover(url) {
            return;
        }
        let Ok(resp) = http.get(url).send().await else {
            return;
        };
        if !resp.status().is_success() {
            return;
        }
        let Ok(bytes) = resp.bytes().await else {
            return;
        };
        if bytes.len() < 64 {
            return;
        }
        let path = self.cover_path(&cover_key(url));
        let tmp = path.with_extension("tmp");
        if fs::write(&tmp, &bytes).is_ok() {
            let _ = fs::rename(&tmp, &path);
        }
    }

    /// Fetch art for a whole source (cover + every track) a few at a time.
    pub async fn fetch_source_covers(self: &Arc<Self>, http: &reqwest::Client, id: &str) {
        let urls: Vec<String> = {
            let g = self.data.lock().unwrap();
            let Some(s) = g.sources.iter().find(|s| s.id == id) else {
                return;
            };
            let mut v: Vec<String> = s.cover.iter().cloned().collect();
            for t in &s.track_ids {
                if let Some(c) = g.tracks.get(t).and_then(|t| t.cover.clone()) {
                    v.push(c);
                }
            }
            let mut seen = HashSet::new();
            v.retain(|u| seen.insert(u.clone()));
            v
        };
        for chunk in urls.chunks(6) {
            let mut set = tokio::task::JoinSet::new();
            for u in chunk {
                let me = self.clone();
                let http = http.clone();
                let u = u.clone();
                set.spawn(async move { me.fetch_cover(&http, &u).await });
            }
            while set.join_next().await.is_some() {}
        }
    }

    fn gc_covers(&self) {
        let keep: HashSet<String> = {
            let g = self.data.lock().unwrap();
            let mut keep = HashSet::new();
            for s in &g.sources {
                if let Some(c) = &s.cover {
                    keep.insert(cover_key(c));
                }
                for t in &s.track_ids {
                    if let Some(c) = g.tracks.get(t).and_then(|t| t.cover.as_ref()) {
                        keep.insert(cover_key(c));
                    }
                }
            }
            keep
        };
        if let Ok(rd) = fs::read_dir(self.covers_dir()) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if !keep.contains(&name) {
                    let _ = fs::remove_file(e.path());
                }
            }
        }
    }

    // -- workers ----------------------------------------------------------

    fn next_job(&self) -> Option<String> {
        let id = self.queue.lock().unwrap().pop_front()?;
        self.active.lock().unwrap().push(id.clone());
        Some(id)
    }

    fn finish_job(&self, id: &str) {
        self.active.lock().unwrap().retain(|x| x != id);
    }

    /// Start the download workers. `emit` is called whenever progress changes.
    pub fn spawn_workers(
        self: &Arc<Self>,
        ytm: Arc<crate::ytm::Ytm>,
        player: crate::player::Player,
        http: reqwest::Client,
        emit: Arc<dyn Fn() + Send + Sync>,
    ) {
        for _ in 0..WORKERS {
            let me = self.clone();
            let ytm = ytm.clone();
            let player = player.clone();
            let http = http.clone();
            let emit = emit.clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    // Back off while paused, offline, or the current song is
                    // still filling its buffer.
                    if me.paused.load(Ordering::Relaxed)
                        || ytm.is_offline()
                        || player.is_buffering()
                    {
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        continue;
                    }
                    let Some(id) = me.next_job() else {
                        let _ = tokio::time::timeout(Duration::from_secs(5), me.wake.notified()).await;
                        continue;
                    };
                    me.running.fetch_add(1, Ordering::Relaxed);
                    emit();
                    if !ytm.cache.has_complete(&id) {
                        ytm.prefetch(&id).await;
                    }
                    let ok = ytm.cache.has_complete(&id);
                    if ok {
                        if let Some(url) = me.track(&id).and_then(|t| t.cover) {
                            me.fetch_cover(&http, &url).await;
                        }
                    }
                    me.finish_job(&id);
                    me.running.fetch_sub(1, Ordering::Relaxed);
                    if !ok {
                        let n = {
                            let mut a = me.attempts.lock().unwrap();
                            let n = a.entry(id.clone()).or_insert(0);
                            *n += 1;
                            *n
                        };
                        // Only count real failures; going offline mid-download
                        // just puts the track back.
                        if ytm.is_offline() {
                            me.queue.lock().unwrap().push_back(id);
                        } else if n < MAX_ATTEMPTS {
                            tokio::time::sleep(Duration::from_secs(2)).await;
                            me.queue.lock().unwrap().push_back(id);
                        } else {
                            me.failed.lock().unwrap().insert(id);
                        }
                    }
                    emit();
                }
            });
        }
    }
}

/// Mime sniffing for the cover protocol.
pub fn sniff_image(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else {
        "image/jpeg"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_key_is_stable_fnv1a64() {
        // Reference values - the webview computes the same hash in JS.
        assert_eq!(cover_key(""), "cbf29ce484222325");
        assert_eq!(cover_key("a"), "af63dc4c8601ec8c");
        assert_eq!(cover_key("https://x/y.jpg").len(), 16);
    }

    #[test]
    fn sniffs_image_types() {
        assert_eq!(sniff_image(&[0x89, b'P', b'N', b'G', 0, 0]), "image/png");
        assert_eq!(sniff_image(b"RIFF\0\0\0\0WEBPVP8 "), "image/webp");
        assert_eq!(sniff_image(&[0xff, 0xd8, 0xff]), "image/jpeg");
    }
}
