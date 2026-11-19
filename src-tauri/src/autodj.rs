//! Auto-DJ: reorders the upcoming part of the queue so each track's
//! starting tempo is close to the previous track's ending tempo, which
//! makes BPM-matched crossfades flow. Optional (off by default) and only
//! when shuffle is off.
//!
//! The next track is never moved (it may already be prefetched or armed
//! for a blend); the window after it is analysed (see `analysis.rs`) and
//! then ordered greedily by nearest tempo, folding half/double time. A
//! window is ordered once per set of tracks, so moving songs by hand
//! within it is respected.
//!
//! With harmonic mixing on, each pick also weighs the Camelot-wheel
//! distance between keys, so blends land in compatible keys (same key,
//! a fifth apart, or relative major/minor) when the tempos allow it.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// Tracks after the next one that Auto-DJ may reorder.
pub const WINDOW: usize = 8;

/// Tempo distance in log space, treating half / double time as equal.
pub fn tempo_dist(a: f64, b: f64) -> f64 {
    if a <= 0.0 || b <= 0.0 {
        return f64::INFINITY;
    }
    [0.5, 1.0, 2.0]
        .iter()
        .map(|f| (b * f / a).ln().abs())
        .fold(f64::INFINITY, f64::min)
}

/// One track's tempo at its start and end (None = unknown) and its key.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tempo {
    pub head: Option<f64>,
    pub tail: Option<f64>,
    pub key: Option<crate::key::Key>,
}

/// Extra cost (in log-tempo units, 0.05 ≈ 5 % tempo) for a key clash.
pub fn key_cost(a: Option<crate::key::Key>, b: Option<crate::key::Key>) -> f64 {
    match (a, b) {
        (Some(a), Some(b)) => match a.distance(&b) {
            0 => 0.0,
            1 => 0.005,
            2 => 0.04,
            _ => 0.08,
        },
        // Unknown key: neutral, slightly behind a known good match.
        _ => 0.02,
    }
}

impl Tempo {
    fn start(&self) -> Option<f64> {
        self.head.or(self.tail)
    }
    fn end(&self) -> Option<f64> {
        self.tail.or(self.head)
    }
}

/// Greedy nearest-tempo order of `items` after a track ending at
/// `prev` BPM. Tracks with no tempo keep their order at the end.
#[cfg(test)]
pub fn order(prev: Option<f64>, items: &[Tempo]) -> Vec<usize> {
    order_mixed(&Tempo { head: prev, tail: prev, key: None }, items, false)
}

/// Like `order`, starting after the track `prev`; with `harmonic` the
/// key distance is added to the tempo distance.
pub fn order_mixed(prev: &Tempo, items: &[Tempo], harmonic: bool) -> Vec<usize> {
    let prev_key = prev.key;
    let prev = prev.end();
    let mut left: Vec<usize> = (0..items.len())
        .filter(|&i| items[i].start().is_some())
        .collect();
    let unknown: Vec<usize> = (0..items.len())
        .filter(|&i| items[i].start().is_none())
        .collect();
    let mut out = Vec::with_capacity(items.len());
    let mut cur = prev;
    let mut cur_key = prev_key;
    while !left.is_empty() {
        let pick = match cur {
            // Ties keep the original order (lowest index first).
            Some(c) => {
                let mut best = 0;
                let mut best_d = f64::INFINITY;
                for (k, &i) in left.iter().enumerate() {
                    let mut d = tempo_dist(c, items[i].start().unwrap_or(c));
                    if harmonic {
                        d += key_cost(cur_key, items[i].key);
                    }
                    if d < best_d - 1e-9 {
                        best_d = d;
                        best = k;
                    }
                }
                best
            }
            None => 0,
        };
        let i = left.remove(pick);
        cur = items[i].end();
        cur_key = items[i].key;
        out.push(i);
    }
    out.extend(unknown);
    out
}

/// Background loop: keeps the upcoming window analysed and ordered.
pub fn start(app: Arc<crate::App>) {
    tauri::async_runtime::spawn(async move {
        // Analysis attempts per track, so a track that can't be measured
        // doesn't hold the window back forever.
        let mut tries: HashMap<String, u8> = HashMap::new();
        let mut last_fp: Option<Vec<String>> = None;
        loop {
            tokio::time::sleep(Duration::from_secs(4)).await;
            if !app.settings.lock().auto_dj {
                last_fp = None;
                continue;
            }
            if app.player.snapshot().shuffle {
                continue;
            }
            let (queue, index) = app.player.queue_and_index();
            let start = index + 2;
            if queue.len() < start + 2 {
                continue;
            }
            let end = queue.len().min(start + WINDOW);
            let ids: Vec<String> = queue[start..end].iter().map(|t| t.id.clone()).collect();
            let mut fp = ids.clone();
            fp.sort();
            fp.push(queue[index + 1].id.clone());
            if last_fp.as_ref() == Some(&fp) {
                continue;
            }
            // Measure what's missing (a couple per pass to stay light).
            let mut waiting = false;
            let mut fetched = 0;
            for id in std::iter::once(&queue[index + 1].id).chain(ids.iter()) {
                if id.is_empty() || app.analysis.has(id) {
                    continue;
                }
                let n = tries.entry(id.clone()).or_insert(0);
                if *n >= 3 {
                    continue;
                }
                waiting = true;
                if fetched < 2 {
                    *n += 1;
                    fetched += 1;
                    if !app.is_offline() {
                        app.ytm.prefetch(id).await;
                    }
                    crate::analyze_prefetched(&app.ytm, &app.player, id);
                }
            }
            if waiting {
                continue;
            }
            let tempo = |id: &str| {
                app.analysis
                    .get(id)
                    .map(|a| Tempo {
                        head: a.head.map(|g| g.bpm),
                        tail: a.tail.map(|g| g.bpm),
                        key: a.key,
                    })
                    .unwrap_or_default()
            };
            let harmonic = app.settings.lock().harmonic_mix;
            let prev = tempo(&queue[index + 1].id);
            let items: Vec<Tempo> = ids.iter().map(|id| tempo(id)).collect();
            let ord = order_mixed(&prev, &items, harmonic);
            last_fp = Some(fp);
            if ord.iter().enumerate().all(|(k, &i)| k == i) {
                continue;
            }
            let new_ids: Vec<String> = ord.iter().map(|&i| ids[i].clone()).collect();
            if app.player.reorder_window(start, &ids, &new_ids) {
                crate::diag::log(format!(
                    "auto-dj: reordered {} upcoming tracks by tempo{} ({})",
                    ids.len(),
                    if harmonic { " and key" } else { "" },
                    ord.iter()
                        .filter_map(|&i| {
                            let b = items[i].start()?;
                            Some(match items[i].key {
                                Some(k) if harmonic => format!("{b:.0} {}", k.camelot_str()),
                                _ => format!("{b:.0}"),
                            })
                        })
                        .collect::<Vec<_>>()
                        .join(" → ")
                ));
            }
            if tries.len() > 500 {
                tries.clear();
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::key::Key;

    fn t(b: f64) -> Tempo {
        Tempo { head: Some(b), tail: Some(b), key: None }
    }

    fn tk(b: f64, tonic: u8, minor: bool) -> Tempo {
        Tempo {
            key: Some(Key { tonic, minor, confidence: 1.0 }),
            ..t(b)
        }
    }

    #[test]
    fn harmonic_prefers_compatible_key_at_similar_tempo() {
        // After C major at 124: 125 in F# major clashes, 127 in G major fits.
        let prev = tk(124.0, 0, false);
        let items = [tk(125.0, 6, false), tk(127.0, 7, false)];
        assert_eq!(order_mixed(&prev, &items, false), vec![0, 1]);
        assert_eq!(order_mixed(&prev, &items, true), vec![1, 0]);
    }

    #[test]
    fn harmonic_does_not_override_big_tempo_jumps() {
        // A matching key 30 % away in tempo still loses to a near tempo.
        let prev = tk(124.0, 0, false);
        let items = [tk(165.0, 0, false), tk(125.0, 6, false)];
        assert_eq!(order_mixed(&prev, &items, true), vec![1, 0]);
    }

    #[test]
    fn folds_half_and_double_time() {
        assert!(tempo_dist(120.0, 60.0) < 1e-9);
        assert!(tempo_dist(87.0, 174.0) < 1e-9);
        assert!(tempo_dist(120.0, 128.0) < tempo_dist(120.0, 100.0));
    }

    #[test]
    fn chains_nearest_tempo() {
        let items = [t(140.0), t(100.0), t(124.0), t(128.0), t(98.0)];
        assert_eq!(order(Some(122.0), &items), vec![2, 3, 0, 1, 4]);
    }

    #[test]
    fn unknown_tempo_goes_last_in_order() {
        let items = [Tempo::default(), t(90.0), Tempo::default(), t(120.0)];
        assert_eq!(order(Some(118.0), &items), vec![3, 1, 0, 2]);
    }

    #[test]
    fn uses_tail_tempo_for_the_next_pick() {
        let items = [
            Tempo { head: Some(100.0), tail: Some(140.0), key: None },
            t(101.0),
            t(139.0),
        ];
        assert_eq!(order(Some(100.0), &items), vec![0, 2, 1]);
    }

    #[test]
    fn no_previous_tempo_keeps_first_known() {
        let items = [t(150.0), t(90.0), t(148.0)];
        assert_eq!(order(None, &items), vec![0, 2, 1]);
    }
}
