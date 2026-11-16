//! Per-track audio analysis, cached on disk: integrated loudness (for
//! volume normalization), peak, leading/trailing silence, and beat grids
//! for the start and end of the track (for BPM matching).
//!
//! Analysis runs on a background thread over complete audio bodies
//! (prefetched or fully downloaded), so a track is usually measured before
//! it starts playing. Results are keyed by video id.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::decode::GrowingBody;
use crate::transition::{analyze, Biquad, EnvBuilder, FrameRes, MIN_CONFIDENCE, SILENCE};

/// Bumped when the measurement changes so stale entries are recomputed.
const VERSION: u32 = 1;
/// Entries kept on disk before the oldest are dropped.
const MAX_ENTRIES: usize = 4000;
/// Seconds analysed at each end of the track for the beat grids.
const GRID_SECS: f32 = 20.0;

/// A beat grid in track time.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Grid {
    pub bpm: f64,
    /// Time of one beat, in seconds from the start of the track.
    pub phase_s: f64,
    pub confidence: f64,
}

impl Grid {
    pub fn period_s(&self) -> f64 {
        60.0 / self.bpm.max(1.0)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrackAnalysis {
    #[serde(default)]
    pub v: u32,
    /// Integrated loudness, LUFS (ITU-R BS.1770 style, gated).
    pub lufs: f32,
    /// Sample peak, 0..=1.
    pub peak: f32,
    pub duration_s: f32,
    /// Seconds of silence before the music starts.
    pub lead_silence_s: f32,
    /// Time the music ends (trailing silence starts), in seconds.
    pub music_end_s: f32,
    pub head: Option<Grid>,
    pub tail: Option<Grid>,
    /// Unix seconds when measured (oldest entries are evicted first).
    #[serde(default)]
    pub at: u64,
}

impl TrackAnalysis {
    /// Linear gain that brings this track to `target_lufs`, limited so the
    /// peak stays under -1 dBFS and the change within -12..+6 dB.
    pub fn gain_for(&self, target_lufs: f32) -> f32 {
        let mut db = (target_lufs - self.lufs).clamp(-12.0, 6.0);
        if self.peak > 0.0 {
            let headroom = -1.0 - 20.0 * self.peak.log10();
            db = db.min(headroom.max(0.0));
        }
        10f32.powf(db / 20.0)
    }
}

type DoneFn = Box<dyn Fn(&str, &TrackAnalysis) + Send + Sync>;

pub struct Analyzer {
    path: PathBuf,
    db: Mutex<HashMap<String, TrackAnalysis>>,
    pending: Mutex<HashSet<String>>,
    tx: std::sync::mpsc::Sender<(String, Arc<GrowingBody>)>,
    on_done: Mutex<Option<DoneFn>>,
    dirty: Mutex<Option<Instant>>,
}

impl Analyzer {
    pub fn open(dir: &std::path::Path) -> Arc<Self> {
        let path = dir.join("analysis.json");
        let db: HashMap<String, TrackAnalysis> = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let (tx, rx) = std::sync::mpsc::channel::<(String, Arc<GrowingBody>)>();
        let me = Arc::new(Self {
            path,
            db: Mutex::new(db.into_iter().filter(|(_, a)| a.v == VERSION).collect()),
            pending: Mutex::new(HashSet::new()),
            tx,
            on_done: Mutex::new(None),
            dirty: Mutex::new(None),
        });
        let weak = Arc::downgrade(&me);
        let _ = std::thread::Builder::new()
            .name("tarn-analysis".into())
            .spawn(move || {
                while let Ok((id, body)) = rx.recv() {
                    let Some(me) = weak.upgrade() else { break };
                    me.run_one(&id, body);
                    me.flush_if_due(false);
                }
            });
        me
    }

    pub fn on_done(&self, f: DoneFn) {
        *self.on_done.lock() = Some(f);
    }

    pub fn get(&self, id: &str) -> Option<TrackAnalysis> {
        if id.is_empty() {
            return None;
        }
        self.db.lock().get(id).cloned()
    }

    pub fn has(&self, id: &str) -> bool {
        self.db.lock().contains_key(id)
    }

    pub fn len(&self) -> usize {
        self.db.lock().len()
    }

    pub fn clear(&self) {
        self.db.lock().clear();
        let _ = std::fs::remove_file(&self.path);
    }

    /// Queue a body for analysis (no-op if cached or already queued).
    pub fn enqueue(&self, id: &str, body: Arc<GrowingBody>) {
        if id.is_empty() || self.has(id) {
            return;
        }
        if !self.pending.lock().insert(id.to_string()) {
            return;
        }
        let _ = self.tx.send((id.to_string(), body));
    }

    fn run_one(&self, id: &str, body: Arc<GrowingBody>) {
        // Wait for the full download (prefetches are usually done already).
        let t0 = Instant::now();
        while !body.is_complete() {
            // Give up on failed or abandoned downloads (only we hold it).
            if body.error().is_some()
                || Arc::strong_count(&body) == 1
                || t0.elapsed() > Duration::from_secs(600)
            {
                self.pending.lock().remove(id);
                return;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        let started = Instant::now();
        let res = measure_body(body);
        self.pending.lock().remove(id);
        match res {
            Some(mut a) => {
                a.at = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                crate::diag::log(format!(
                    "analysis: {id} {:.1} LUFS peak {:.2} head {:?} tail {:?} ({} ms)",
                    a.lufs,
                    a.peak,
                    a.head.map(|g| (g.bpm * 10.0).round() / 10.0),
                    a.tail.map(|g| (g.bpm * 10.0).round() / 10.0),
                    started.elapsed().as_millis()
                ));
                self.insert(id, a.clone());
                if let Some(f) = self.on_done.lock().as_ref() {
                    f(id, &a);
                }
            }
            None => eprintln!("analysis: {id} could not be measured"),
        }
    }

    fn insert(&self, id: &str, a: TrackAnalysis) {
        let mut db = self.db.lock();
        db.insert(id.to_string(), a);
        if db.len() > MAX_ENTRIES {
            let mut ages: Vec<(u64, String)> = db.iter().map(|(k, v)| (v.at, k.clone())).collect();
            ages.sort();
            for (_, k) in ages.into_iter().take(db.len() - MAX_ENTRIES) {
                db.remove(&k);
            }
        }
        drop(db);
        let mut d = self.dirty.lock();
        if d.is_none() {
            *d = Some(Instant::now());
        }
    }

    /// Write the cache if it changed and a few seconds passed (or `force`).
    pub fn flush_if_due(&self, force: bool) {
        let due = match *self.dirty.lock() {
            Some(t) => force || t.elapsed() > Duration::from_secs(5),
            None => false,
        };
        if !due && !force {
            return;
        }
        if self.dirty.lock().is_none() {
            return;
        }
        let bytes = serde_json::to_vec(&*self.db.lock()).unwrap_or_default();
        let tmp = self.path.with_extension("json.tmp");
        if std::fs::write(&tmp, bytes).is_ok() && std::fs::rename(&tmp, &self.path).is_ok() {
            *self.dirty.lock() = None;
        }
    }
}

/// Gated loudness meter with an approximate K-weighting filter.
pub struct Loudness {
    pre: Vec<Biquad>,
    hp: Vec<Biquad>,
    block: usize,
    n: usize,
    acc: f64,
    blocks: Vec<f64>,
    pub peak: f32,
}

impl Loudness {
    pub fn new(sr: u32, ch: usize) -> Self {
        let sr_f = sr.max(1) as f32;
        let mut pre = vec![Biquad::default(); ch];
        let mut hp = vec![Biquad::default(); ch];
        for f in pre.iter_mut() {
            f.set_high_shelf(1681.0, 4.0, sr_f);
        }
        for f in hp.iter_mut() {
            f.set_highpass(38.0, sr_f);
        }
        Self {
            pre,
            hp,
            // 400 ms blocks (non-overlapping is close enough for gating).
            block: (sr as usize * 2 / 5).max(1),
            n: 0,
            acc: 0.0,
            blocks: Vec::new(),
            peak: 0.0,
        }
    }

    pub fn push_frame(&mut self, frame: &[i16]) {
        let mut ms = 0f64;
        for (c, &v) in frame.iter().enumerate().take(self.pre.len()) {
            let x = v as f32 / 32768.0;
            self.peak = self.peak.max(x.abs());
            let y = self.hp[c].process(self.pre[c].process(x));
            ms += (y as f64) * (y as f64);
        }
        self.acc += ms;
        self.n += 1;
        if self.n >= self.block {
            self.blocks.push(self.acc / self.n as f64);
            self.acc = 0.0;
            self.n = 0;
        }
    }

    /// Integrated loudness in LUFS, or None for silence.
    pub fn lufs(&self) -> Option<f32> {
        let to_lufs = |ms: f64| -0.691 + 10.0 * ms.max(1e-12).log10();
        let abs: Vec<f64> = self
            .blocks
            .iter()
            .copied()
            .filter(|&m| to_lufs(m) > -70.0)
            .collect();
        if abs.is_empty() {
            return None;
        }
        let mean = abs.iter().sum::<f64>() / abs.len() as f64;
        let rel = to_lufs(mean) - 10.0;
        let gated: Vec<f64> = abs.into_iter().filter(|&m| to_lufs(m) > rel).collect();
        if gated.is_empty() {
            return None;
        }
        Some(to_lufs(gated.iter().sum::<f64>() / gated.len() as f64) as f32)
    }
}

/// Decode a complete body and measure it.
pub fn measure_body(body: Arc<GrowingBody>) -> Option<TrackAnalysis> {
    use rodio::Source;
    let mut src = crate::decode::open(body, 0.0).ok()?;
    let sr = src.sample_rate().max(1);
    let ch = src.channels().max(1) as usize;
    let mut f = vec![0i16; ch];
    let mut meter = Loudness::new(sr, ch);
    let mut head = EnvBuilder::new(sr, GRID_SECS);
    let mut tail = EnvBuilder::new(sr, GRID_SECS);
    let head_frames = (GRID_SECS * sr as f32) as u64;
    let mut frames = 0u64;
    let mut first_sound: Option<u64> = None;
    let mut last_sound = 0u64;
    while let FrameRes::Ok = crate::mix::pull_raw(&mut src, &mut f) {
        meter.push_frame(&f);
        let peak = f.iter().map(|&v| (v as i32).abs()).max().unwrap_or(0);
        if peak >= SILENCE {
            first_sound.get_or_insert(frames);
            last_sound = frames;
        }
        if frames < head_frames {
            head.push_frame(&f);
        }
        tail.push_frame(&f);
        frames += 1;
    }
    if frames < sr as u64 {
        return None;
    }
    let sr_f = sr as f64;
    let grid = |e: &EnvBuilder| -> Option<Grid> {
        let full: Vec<f32> = e.full.iter().copied().collect();
        let low: Vec<f32> = e.low.iter().copied().collect();
        let b = analyze(&full, &low, sr)?;
        (b.confidence >= MIN_CONFIDENCE).then(|| Grid {
            bpm: b.bpm,
            phase_s: (e.window_start() as f64 + b.phase) / sr_f,
            confidence: b.confidence,
        })
    };
    Some(TrackAnalysis {
        v: VERSION,
        lufs: meter.lufs().unwrap_or(-70.0),
        peak: meter.peak,
        duration_s: (frames as f64 / sr_f) as f32,
        lead_silence_s: (first_sound.unwrap_or(0) as f64 / sr_f) as f32,
        music_end_s: ((last_sound + 1) as f64 / sr_f) as f32,
        head: grid(&head),
        tail: grid(&tail),
        at: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(sr: u32, secs: f32, amp: f32) -> Vec<i16> {
        (0..(sr as f32 * secs) as usize)
            .map(|i| {
                ((2.0 * std::f32::consts::PI * 1000.0 * i as f32 / sr as f32).sin() * amp * 32767.0)
                    as i16
            })
            .collect()
    }

    fn meter_of(s: &[i16], sr: u32) -> Loudness {
        let mut m = Loudness::new(sr, 2);
        for &v in s {
            m.push_frame(&[v, v]);
        }
        m
    }

    #[test]
    fn loudness_tracks_level() {
        let sr = 48000;
        let loud = meter_of(&sine(sr, 4.0, 0.5), sr).lufs().unwrap();
        let quiet = meter_of(&sine(sr, 4.0, 0.05), sr).lufs().unwrap();
        // 20 dB amplitude difference → ~20 LU.
        assert!((loud - quiet - 20.0).abs() < 0.5, "{loud} vs {quiet}");
        // A 1 kHz stereo sine at -6 dBFS is about -6 LUFS with K-weighting
        // (+0.7 dB shelf at 1 kHz, +3 dB for two channels, -3 dB sine RMS).
        assert!(loud > -9.0 && loud < -3.0, "{loud}");
    }

    #[test]
    fn silence_has_no_loudness() {
        assert!(meter_of(&vec![0i16; 48000 * 2], 48000).lufs().is_none());
    }

    #[test]
    fn gain_is_limited_by_peak_and_range() {
        let a = TrackAnalysis {
            v: VERSION,
            lufs: -20.0,
            peak: 0.5,
            duration_s: 100.0,
            lead_silence_s: 0.0,
            music_end_s: 100.0,
            head: None,
            tail: None,
            at: 0,
        };
        // Wants +6 dB, but the peak (−6 dBFS) only allows +5 dB.
        let g = a.gain_for(-14.0);
        assert!(
            (20.0 * g.log10() - 5.02).abs() < 0.05,
            "{}",
            20.0 * g.log10()
        );
        let loud = TrackAnalysis {
            lufs: -5.0,
            peak: 1.0,
            ..a.clone()
        };
        assert!((20.0 * loud.gain_for(-14.0).log10() + 9.0).abs() < 0.01);
        let very = TrackAnalysis { lufs: 10.0, ..loud };
        assert!((20.0 * very.gain_for(-14.0).log10() + 12.0).abs() < 0.01);
    }

    #[test]
    fn cache_round_trips_and_drops_old_versions() {
        let dir = std::env::temp_dir().join(format!("tarn-analysis-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = TrackAnalysis {
            v: VERSION,
            lufs: -9.5,
            peak: 0.9,
            duration_s: 200.0,
            lead_silence_s: 0.2,
            music_end_s: 198.0,
            head: Some(Grid {
                bpm: 124.0,
                phase_s: 0.25,
                confidence: 3.0,
            }),
            tail: None,
            at: 1,
        };
        {
            let an = Analyzer::open(&dir);
            an.insert("abc", a.clone());
            an.insert("old", TrackAnalysis { v: 0, ..a.clone() });
            an.flush_if_due(true);
        }
        let an = Analyzer::open(&dir);
        assert_eq!(an.get("abc"), Some(a));
        assert!(an.get("old").is_none());
        an.clear();
        assert_eq!(an.len(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
