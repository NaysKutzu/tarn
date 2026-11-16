//! Local listening history for the Stats ("Wrapped") page. Nothing leaves
//! the device: one JSON line per listen in `listens.jsonl` in the data dir.
//!
//! A listen counts once the track played for 30 s or half its length
//! (whichever is shorter), and is written when the track changes, repeats
//! or the app quits, with the seconds actually heard.

use std::io::Write;
use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::models::{PlayerState, Track};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Listen {
    /// Unix seconds when the track started.
    pub at: u64,
    /// Seconds actually played.
    pub secs: u32,
    pub track: Track,
}

/// Seconds that make a play count (or half the track if shorter).
const MIN_SECS: f64 = 30.0;

#[derive(Default)]
struct Current {
    track: Option<Track>,
    at: u64,
    heard: f64,
    duration: f64,
    last_pos: f64,
}

pub struct Stats {
    path: PathBuf,
    cur: Mutex<Current>,
    enabled: std::sync::atomic::AtomicBool,
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Stats {
    pub fn open(dir: &Path, enabled: bool) -> Self {
        Self {
            path: dir.join("listens.jsonl"),
            cur: Mutex::new(Current::default()),
            enabled: std::sync::atomic::AtomicBool::new(enabled),
        }
    }

    pub fn set_enabled(&self, on: bool) {
        let was = self.enabled.swap(on, std::sync::atomic::Ordering::Relaxed);
        if was && !on {
            *self.cur.lock() = Current::default();
        }
    }

    fn enabled(&self) -> bool {
        self.enabled.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Feed a player snapshot (called every ~200 ms).
    pub fn tick(&self, st: &PlayerState) {
        if !self.enabled() {
            return;
        }
        let done = {
            let mut c = self.cur.lock();
            observe(&mut c, st, now())
        };
        if let Some(l) = done {
            self.append(&l);
        }
    }

    /// Write the track in progress (app exit).
    pub fn flush(&self) {
        if !self.enabled() {
            return;
        }
        let done = {
            let mut c = self.cur.lock();
            let l = finish(&c);
            *c = Current::default();
            l
        };
        if let Some(l) = done {
            self.append(&l);
        }
    }

    fn append(&self, l: &Listen) {
        let Ok(mut line) = serde_json::to_string(l) else {
            return;
        };
        line.push('\n');
        let r = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .and_then(|mut f| f.write_all(line.as_bytes()));
        if let Err(e) = r {
            eprintln!("stats: {e}");
        }
    }

    /// Listens since `since` (unix seconds), oldest first.
    pub fn listens(&self, since: u64) -> Vec<Listen> {
        let Ok(text) = std::fs::read_to_string(&self.path) else {
            return Vec::new();
        };
        text.lines()
            .filter_map(|l| serde_json::from_str::<Listen>(l).ok())
            .filter(|l| l.at >= since)
            .collect()
    }

    pub fn clear(&self) {
        *self.cur.lock() = Current::default();
        let _ = std::fs::remove_file(&self.path);
    }
}

fn finish(c: &Current) -> Option<Listen> {
    let t = c.track.as_ref()?;
    let need = if c.duration > 1.0 {
        MIN_SECS.min(c.duration / 2.0)
    } else {
        MIN_SECS
    };
    (c.heard >= need).then(|| Listen {
        at: c.at,
        secs: c.heard.round() as u32,
        track: t.clone(),
    })
}

/// Advance the tracker; returns a finished listen when one ends.
fn observe(c: &mut Current, st: &PlayerState, now: u64) -> Option<Listen> {
    let id = st.track.as_ref().map(|t| t.id.as_str()).unwrap_or("");
    let same = c.track.as_ref().is_some_and(|t| t.id == id);
    // A jump back to the start of the same song (repeat one, or replay)
    // after it already counted is a new listen.
    let restarted = same && st.position + 5.0 < c.last_pos && st.position < 3.0;
    if !same || restarted {
        let out = finish(c);
        *c = Current {
            track: st.track.clone().filter(|t| !t.id.is_empty()),
            at: now,
            heard: 0.0,
            duration: st.duration,
            last_pos: st.position,
        };
        return out;
    }
    if st.duration > 0.0 {
        c.duration = st.duration;
    }
    let d = st.position - c.last_pos;
    // Count real playback only (no seeks, no paused time).
    if st.playing && d > 0.0 && d < 2.0 {
        c.heard += d;
    }
    c.last_pos = st.position;
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(id: &str, pos: f64, playing: bool) -> PlayerState {
        PlayerState {
            track: (!id.is_empty()).then(|| Track {
                id: id.into(),
                title: format!("T {id}"),
                ..Default::default()
            }),
            position: pos,
            duration: 200.0,
            playing,
            ..PlayerState::default()
        }
    }

    fn run(steps: &[(&str, f64, bool)]) -> Vec<Listen> {
        let mut c = Current::default();
        let mut out = Vec::new();
        for (i, (id, pos, playing)) in steps.iter().enumerate() {
            if let Some(l) = observe(&mut c, &st(id, *pos, *playing), 1000 + i as u64) {
                out.push(l);
            }
        }
        out.extend(finish(&c));
        out
    }

    fn play(id: &'static str, from: f64, to: f64) -> Vec<(&'static str, f64, bool)> {
        let mut v = Vec::new();
        let mut p = from;
        while p <= to {
            v.push((id, p, true));
            p += 0.2;
        }
        v
    }

    #[test]
    fn counts_after_thirty_seconds() {
        let mut s = play("a", 0.0, 40.0);
        s.extend(play("b", 0.0, 10.0));
        let l = run(&s);
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].track.id, "a");
        assert!((39..=41).contains(&l[0].secs), "{}", l[0].secs);
    }

    #[test]
    fn seeks_and_pauses_do_not_count() {
        let mut s = play("a", 0.0, 10.0);
        // Seek far ahead, then sit paused.
        s.push(("a", 150.0, true));
        for _ in 0..500 {
            s.push(("a", 150.0, false));
        }
        s.extend(play("a", 150.0, 160.0));
        let l = run(&s);
        assert!(l.is_empty(), "only ~20 s heard: {l:?}");
    }

    #[test]
    fn repeat_one_counts_twice() {
        let mut s = play("a", 0.0, 60.0);
        s.extend(play("a", 0.0, 60.0));
        let l = run(&s);
        assert_eq!(l.len(), 2);
    }

    #[test]
    fn short_tracks_need_half() {
        let mut c = Current::default();
        let mut s = st("a", 0.0, true);
        s.duration = 40.0;
        observe(&mut c, &s, 0);
        let mut p = 0.0;
        while p < 21.0 {
            p += 0.2;
            s.position = p;
            observe(&mut c, &s, 0);
        }
        assert!(finish(&c).is_some());
    }

    #[test]
    fn writes_and_reads_jsonl() {
        let dir = std::env::temp_dir().join(format!("tarn-stats-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let s = Stats::open(&dir, true);
        s.clear();
        let l = Listen {
            at: 5,
            secs: 100,
            track: Track {
                id: "x".into(),
                title: "X".into(),
                ..Default::default()
            },
        };
        s.append(&l);
        s.append(&Listen {
            at: 50,
            ..l.clone()
        });
        assert_eq!(s.listens(0).len(), 2);
        assert_eq!(s.listens(10), vec![Listen { at: 50, ..l }]);
        s.clear();
        assert!(s.listens(0).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
