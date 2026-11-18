//! SponsorBlock "music_offtopic" skipping (off by default): music videos
//! often open with a skit or close with credits. Segments come from the
//! community SponsorBlock API, looked up by a 4-char SHA-256 prefix of the
//! video id (k-anonymity: the server never learns which video plays).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Deserialize;
use sha2::{Digest, Sha256};

const API: &str = "https://sponsor.ajay.app/api/skipSegments";
/// Ignore crumbs: a skip shorter than this is more jarring than useful.
const MIN_SEGMENT_S: f64 = 1.0;
/// Land a hair past the end so float rounding can't re-trigger the skip.
const LAND_PAST_S: f64 = 0.05;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub start: f64,
    pub end: f64,
}

#[derive(Deserialize)]
struct ApiVideo {
    #[serde(rename = "videoID")]
    video_id: String,
    #[serde(default)]
    segments: Vec<ApiSegment>,
}

#[derive(Deserialize)]
struct ApiSegment {
    segment: [f64; 2],
    #[serde(default)]
    category: String,
    #[serde(rename = "actionType", default)]
    action_type: String,
}

#[derive(Default)]
pub struct SponsorBlock {
    cache: Mutex<HashMap<String, Arc<Vec<Segment>>>>,
    inflight: Mutex<HashSet<String>>,
    /// Failed lookups wait before retrying.
    failed: Mutex<HashMap<String, Instant>>,
    /// Last skip (video id, segment end, when) to never loop on one segment.
    last: Mutex<Option<(String, f64, Instant)>>,
}

pub enum Action {
    Seek(f64),
    /// The segment runs to the end of the track: go to the next song.
    Next,
}

impl SponsorBlock {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// What to do at `pos` (seconds) of video `id`, or None. Starts a lookup
    /// in the background the first time a video is seen.
    pub fn check(self: &Arc<Self>, http: &reqwest::Client, id: &str, pos: f64, duration: f64) -> Option<Action> {
        let segs = self.cache.lock().get(id).cloned();
        let Some(segs) = segs else {
            self.fetch(http, id);
            return None;
        };
        let end = skip_target(&segs, pos)?;
        {
            let mut last = self.last.lock();
            if let Some((lid, lend, at)) = last.as_ref() {
                if lid == id && (lend - end).abs() < 0.01 && at.elapsed() < Duration::from_secs(3) {
                    return None;
                }
            }
            *last = Some((id.to_string(), end, Instant::now()));
        }
        if duration > 0.0 && end >= duration - 1.0 {
            Some(Action::Next)
        } else {
            Some(Action::Seek(end + LAND_PAST_S))
        }
    }

    fn fetch(self: &Arc<Self>, http: &reqwest::Client, id: &str) {
        if id.is_empty() {
            return;
        }
        if let Some(t) = self.failed.lock().get(id) {
            if t.elapsed() < Duration::from_secs(120) {
                return;
            }
        }
        if !self.inflight.lock().insert(id.to_string()) {
            return;
        }
        let me = self.clone();
        let http = http.clone();
        let id = id.to_string();
        tauri::async_runtime::spawn(async move {
            match lookup(&http, &id).await {
                Ok(segs) => {
                    if !segs.is_empty() {
                        crate::diag::log(format!("sponsorblock: {} off-topic segment(s) for {id}", segs.len()));
                    }
                    let mut c = me.cache.lock();
                    if c.len() > 2000 {
                        c.clear();
                    }
                    c.insert(id.clone(), Arc::new(segs));
                }
                Err(e) => {
                    crate::diag::log(format!("sponsorblock: lookup failed: {e}"));
                    me.failed.lock().insert(id.clone(), Instant::now());
                }
            }
            me.inflight.lock().remove(&id);
        });
    }
}

fn hash_prefix(id: &str) -> String {
    hex::encode(Sha256::digest(id.as_bytes()))[..4].to_string()
}

async fn lookup(http: &reqwest::Client, id: &str) -> anyhow::Result<Vec<Segment>> {
    let url = format!(
        "{API}/{}?categories=%5B%22music_offtopic%22%5D&actionTypes=%5B%22skip%22%5D",
        hash_prefix(id)
    );
    let r = http.get(url).timeout(Duration::from_secs(8)).send().await?;
    if r.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(Vec::new());
    }
    let r = r.error_for_status()?;
    let videos: Vec<ApiVideo> = r.json().await?;
    Ok(parse(videos, id))
}

fn parse(videos: Vec<ApiVideo>, id: &str) -> Vec<Segment> {
    let mut out: Vec<Segment> = videos
        .into_iter()
        .filter(|v| v.video_id == id)
        .flat_map(|v| v.segments)
        .filter(|s| s.category == "music_offtopic" && (s.action_type.is_empty() || s.action_type == "skip"))
        .map(|s| Segment { start: s.segment[0].max(0.0), end: s.segment[1] })
        .filter(|s| s.start.is_finite() && s.end.is_finite() && s.end - s.start >= MIN_SEGMENT_S)
        .collect();
    out.sort_by(|a, b| a.start.total_cmp(&b.start));
    // Merge overlaps so one skip covers back-to-back segments.
    let mut merged: Vec<Segment> = Vec::with_capacity(out.len());
    for s in out {
        match merged.last_mut() {
            Some(m) if s.start <= m.end + 0.5 => m.end = m.end.max(s.end),
            _ => merged.push(s),
        }
    }
    merged
}

/// End of the segment `pos` is inside (not in its last 0.25 s), if any.
pub fn skip_target(segs: &[Segment], pos: f64) -> Option<f64> {
    segs.iter().find(|s| pos >= s.start && pos < s.end - 0.25).map(|s| s.end)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(start: f64, end: f64, cat: &str) -> ApiSegment {
        ApiSegment { segment: [start, end], category: cat.into(), action_type: "skip".into() }
    }

    #[test]
    fn parses_filters_and_merges() {
        let v = vec![
            ApiVideo { video_id: "other".into(), segments: vec![seg(0.0, 30.0, "music_offtopic")] },
            ApiVideo {
                video_id: "abc".into(),
                segments: vec![
                    seg(200.0, 240.0, "music_offtopic"),
                    seg(0.0, 20.0, "music_offtopic"),
                    seg(19.8, 25.0, "music_offtopic"),
                    seg(50.0, 50.4, "music_offtopic"),
                    seg(60.0, 70.0, "sponsor"),
                ],
            },
        ];
        let s = parse(v, "abc");
        assert_eq!(s, vec![Segment { start: 0.0, end: 25.0 }, Segment { start: 200.0, end: 240.0 }]);
    }

    #[test]
    fn target() {
        let s = [Segment { start: 0.0, end: 25.0 }, Segment { start: 200.0, end: 240.0 }];
        assert_eq!(skip_target(&s, 0.1), Some(25.0));
        assert_eq!(skip_target(&s, 24.9), None);
        assert_eq!(skip_target(&s, 100.0), None);
        assert_eq!(skip_target(&s, 210.0), Some(240.0));
    }

    #[test]
    fn prefix_is_sha256() {
        assert_eq!(hash_prefix("dQw4w9WgXcQ").len(), 4);
        assert_eq!(hash_prefix("a"), "ca97");
    }
}
