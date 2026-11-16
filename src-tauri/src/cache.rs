//! On-disk LRU cache of fully downloaded audio streams.
//!
//! Tracks are stored under the app data dir (`tarn/audio-cache/`) and evicted
//! by least-recent access when the total size exceeds a configurable cap
//! (default 1 GiB). Partial files are kept so a dropped download can resume.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

const INDEX: &str = "index.json";
pub const DEFAULT_MAX_MB: u64 = 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Entry {
    file: String,
    size: u64,
    accessed: u64,
    complete: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Index {
    items: HashMap<String, Entry>,
    /// Ids kept for offline listening: never evicted, not counted against
    /// the cache cap.
    #[serde(default)]
    pinned: std::collections::HashSet<String>,
}

pub struct AudioCache {
    dir: PathBuf,
    max_bytes: AtomicU64,
    index: Mutex<Index>,
}

impl AudioCache {
    pub fn open() -> Self {
        let dir = dirs::data_dir()
            .or_else(|| dirs::home_dir().map(|h| h.join(".local/share")))
            .unwrap_or_else(|| PathBuf::from("."))
            .join("tarn")
            .join("audio-cache");
        let _ = fs::create_dir_all(&dir);
        let index = load_index(&dir);
        Self {
            dir,
            max_bytes: AtomicU64::new(DEFAULT_MAX_MB * 1024 * 1024),
            index: Mutex::new(index),
        }
    }

    pub fn set_max_mb(&self, mb: u64) {
        // Saturate so joke sizes (TB/PB/EB) don't wrap the byte cap.
        let bytes = mb.saturating_mul(1024).saturating_mul(1024);
        self.max_bytes.store(bytes, Ordering::Relaxed);
        if bytes == 0 {
            self.clear();
        } else {
            self.evict(0);
        }
    }

    pub fn max_bytes(&self) -> u64 {
        self.max_bytes.load(Ordering::Relaxed)
    }

    /// Bytes held by evictable (non-downloaded) entries.
    pub fn used_bytes(&self) -> u64 {
        let g = self.index.lock().unwrap();
        g.items
            .iter()
            .filter(|(k, _)| !g.pinned.contains(*k))
            .map(|(_, e)| e.size)
            .sum()
    }

    /// Bytes held by tracks pinned for offline listening.
    pub fn pinned_bytes(&self) -> u64 {
        let g = self.index.lock().unwrap();
        g.items
            .iter()
            .filter(|(k, _)| g.pinned.contains(*k))
            .map(|(_, e)| e.size)
            .sum()
    }

    pub fn is_pinned(&self, id: &str) -> bool {
        self.index.lock().unwrap().pinned.contains(id)
    }

    /// Mark ids as downloaded-for-offline. Files that arrive later are
    /// protected from eviction from the moment they are committed.
    pub fn pin<I: IntoIterator<Item = String>>(&self, ids: I) {
        {
            let mut g = self.index.lock().unwrap();
            g.pinned.extend(ids);
        }
        let _ = self.persist();
    }

    /// Release ids back into the normal LRU pool (files stay until evicted).
    pub fn unpin<'a, I: IntoIterator<Item = &'a String>>(&self, ids: I) {
        {
            let mut g = self.index.lock().unwrap();
            for id in ids {
                g.pinned.remove(id);
            }
        }
        let _ = self.persist();
    }

    /// Ids that are fully on disk but only cached (not downloaded).
    pub fn complete_unpinned(&self) -> Vec<String> {
        let g = self.index.lock().unwrap();
        g.items
            .iter()
            .filter(|(k, e)| e.complete && e.size > 64 && !g.pinned.contains(*k))
            .map(|(k, _)| k.clone())
            .collect()
    }

    pub fn pinned_ids(&self) -> Vec<String> {
        self.index.lock().unwrap().pinned.iter().cloned().collect()
    }

    pub fn has_complete(&self, id: &str) -> bool {
        let g = self.index.lock().unwrap();
        g.items.get(id).is_some_and(|e| e.complete && e.size > 64)
    }

    /// Read a complete cached body, touching LRU.
    pub fn read_complete(&self, id: &str) -> Option<Vec<u8>> {
        let path = {
            let mut g = self.index.lock().unwrap();
            let e = g.items.get_mut(id)?;
            if !e.complete || e.size < 64 {
                return None;
            }
            e.accessed = now();
            let p = self.dir.join(&e.file);
            drop(g);
            let _ = self.persist();
            p
        };
        let bytes = fs::read(&path).ok()?;
        if bytes.len() < 64 {
            return None;
        }
        Some(bytes)
    }

    /// Path and current length of an incomplete part file, for Range resumes.
    pub fn partial(&self, id: &str) -> Option<(PathBuf, u64)> {
        let part = self.part_path(id);
        let meta = fs::metadata(&part).ok()?;
        if !meta.is_file() || meta.len() == 0 {
            return None;
        }
        Some((part, meta.len()))
    }

    pub fn part_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{}.part", safe(id)))
    }

    pub fn final_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{}.m4a", safe(id)))
    }

    /// Append a chunk to the part file. Creates it if needed.
    pub fn append_part(&self, id: &str, chunk: &[u8]) -> Result<()> {
        if chunk.is_empty() || (self.max_bytes() == 0 && !self.is_pinned(id)) {
            return Ok(());
        }
        let path = self.part_path(id);
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("open cache part {}", path.display()))?;
        f.write_all(chunk)?;
        Ok(())
    }

    /// Seed a part file from already-in-memory bytes (overwrite).
    pub fn write_part(&self, id: &str, bytes: &[u8]) -> Result<()> {
        if self.max_bytes() == 0 && !self.is_pinned(id) {
            return Ok(());
        }
        fs::write(self.part_path(id), bytes)?;
        Ok(())
    }

    /// Mark the download complete: move `.part` (or the given bytes) into the
    /// final file and record it in the index. Evicts LRU entries if needed.
    pub fn commit(&self, id: &str, bytes: &[u8]) -> Result<()> {
        if self.max_bytes() == 0 && !self.is_pinned(id) {
            let _ = fs::remove_file(self.part_path(id));
            return Ok(());
        }
        if bytes.len() < 64 {
            return Err(anyhow!("cache commit too small"));
        }
        if !self.is_pinned(id) {
            self.evict(bytes.len() as u64);
        }
        let dest = self.final_path(id);
        let part = self.part_path(id);
        if part.exists() {
            // Prefer the on-disk part if it matches; otherwise write bytes.
            let on_disk = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
            if on_disk == bytes.len() as u64 {
                let _ = fs::rename(&part, &dest);
            } else {
                fs::write(&dest, bytes)?;
                let _ = fs::remove_file(&part);
            }
        } else {
            fs::write(&dest, bytes)?;
        }
        // If rename landed on a cross-device fail, dest may be missing.
        if !dest.exists() {
            fs::write(&dest, bytes)?;
        }
        let size = fs::metadata(&dest).map(|m| m.len()).unwrap_or(bytes.len() as u64);
        {
            let mut g = self.index.lock().unwrap();
            g.items.insert(
                id.to_string(),
                Entry {
                    file: dest
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into(),
                    size,
                    accessed: now(),
                    complete: true,
                },
            );
        }
        self.persist()?;
        Ok(())
    }

    pub fn evict(&self, extra: u64) {
        let max = self.max_bytes();
        if max == 0 {
            return;
        }
        loop {
            let victim = {
                let g = self.index.lock().unwrap();
                let used: u64 = g
                    .items
                    .iter()
                    .filter(|(k, _)| !g.pinned.contains(*k))
                    .map(|(_, e)| e.size)
                    .sum();
                if used.saturating_add(extra) <= max {
                    return;
                }
                g.items
                    .iter()
                    .filter(|(k, _)| !g.pinned.contains(*k))
                    .min_by_key(|(_, e)| e.accessed)
                    .map(|(k, e)| (k.clone(), e.file.clone()))
            };
            let Some((id, file)) = victim else {
                return;
            };
            let _ = fs::remove_file(self.dir.join(&file));
            let _ = fs::remove_file(self.part_path(&id));
            self.index.lock().unwrap().items.remove(&id);
            let _ = self.persist();
        }
    }

    /// Drop both the complete file and any partial for this track.
    pub fn forget(&self, id: &str) {
        let _ = fs::remove_file(self.final_path(id));
        let _ = fs::remove_file(self.part_path(id));
        self.index.lock().unwrap().items.remove(id);
        let _ = self.persist();
    }

    /// Empty the cache. Downloaded (pinned) tracks are kept.
    pub fn clear(&self) {
        let keep: std::collections::HashSet<String> = {
            let g = self.index.lock().unwrap();
            g.pinned
                .iter()
                .filter_map(|id| g.items.get(id).map(|e| e.file.clone()))
                .collect()
        };
        if let Ok(rd) = fs::read_dir(&self.dir) {
            for e in rd.flatten() {
                let p = e.path();
                let name = e.file_name().to_string_lossy().to_string();
                if p.is_file() && name != INDEX && !keep.contains(&name) {
                    let _ = fs::remove_file(p);
                }
            }
        }
        {
            let mut g = self.index.lock().unwrap();
            let pinned = g.pinned.clone();
            g.items.retain(|k, _| pinned.contains(k));
        }
        let _ = self.persist();
    }

    /// Remove one file for good, pinned or not (used when deleting a download).
    pub fn remove_file(&self, id: &str) {
        self.forget(id);
        self.index.lock().unwrap().pinned.remove(id);
        let _ = self.persist();
    }

    fn persist(&self) -> Result<()> {
        let g = self.index.lock().unwrap();
        let tmp = self.dir.join(".index.json.tmp");
        let data = serde_json::to_vec_pretty(&*g)?;
        fs::write(&tmp, data)?;
        fs::rename(&tmp, self.dir.join(INDEX))?;
        Ok(())
    }
}

fn load_index(dir: &Path) -> Index {
    let p = dir.join(INDEX);
    let Ok(s) = fs::read_to_string(p) else {
        return Index::default();
    };
    let mut idx: Index = serde_json::from_str(&s).unwrap_or_default();
    // Drop entries whose files vanished.
    idx.items.retain(|_, e| dir.join(&e.file).is_file());
    idx
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn safe(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tarn-cache-test-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn cache_in(dir: PathBuf, max_bytes: u64) -> AudioCache {
        AudioCache {
            dir,
            max_bytes: AtomicU64::new(max_bytes),
            index: Mutex::new(Index::default()),
        }
    }

    #[test]
    fn pinned_tracks_survive_eviction_and_clear() {
        let dir = tmp("pin");
        let c = cache_in(dir.clone(), 250);
        c.pin(["keep".to_string()]);
        c.commit("keep", &[1u8; 200]).unwrap();
        c.commit("a", &[2u8; 200]).unwrap();
        // Over the cap: the unpinned one is evicted, the download stays.
        c.commit("b", &[3u8; 200]).unwrap();
        assert!(c.has_complete("keep"));
        assert!(!c.has_complete("a"));
        assert!(c.has_complete("b"));
        assert_eq!(c.pinned_bytes(), 200);
        c.clear();
        assert!(c.has_complete("keep"));
        assert!(!c.has_complete("b"));
        c.remove_file("keep");
        assert!(!c.has_complete("keep"));
        assert!(!c.is_pinned("keep"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn pinned_download_works_with_cache_disabled() {
        let dir = tmp("zero");
        let c = cache_in(dir.clone(), 0);
        c.commit("x", &[1u8; 200]).unwrap();
        assert!(!c.has_complete("x"));
        c.pin(["x".to_string()]);
        c.commit("x", &[1u8; 200]).unwrap();
        assert!(c.has_complete("x"));
        let _ = fs::remove_dir_all(dir);
    }
}
