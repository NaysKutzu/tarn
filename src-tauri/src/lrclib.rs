//! Synced-lyrics fallback from LRCLIB (https://lrclib.net), a free, open
//! lyrics database. Used only when YouTube Music has no timed lyrics for a
//! track and the user left the option on.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;

use parking_lot::Mutex;
use serde::Deserialize;

use crate::models::{Lyrics, Track};

const API: &str = "https://lrclib.net/api";
const CACHE_MAX: usize = 300;

#[derive(Debug, Deserialize)]
struct Record {
    #[serde(default, rename = "trackName")]
    track_name: Option<String>,
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default)]
    instrumental: bool,
    #[serde(default, rename = "plainLyrics")]
    plain: Option<String>,
    #[serde(default, rename = "syncedLyrics")]
    synced: Option<String>,
}

fn client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .user_agent(concat!(
                "tarn/",
                env!("CARGO_PKG_VERSION"),
                " (https://github.com/rexsystems/tarn)"
            ))
            .build()
            .unwrap_or_default()
    })
}

fn cache() -> &'static Mutex<HashMap<String, Option<Lyrics>>> {
    static C: OnceLock<Mutex<HashMap<String, Option<Lyrics>>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Strip the video-title noise YouTube uploads carry ("(Official Video)",
/// "[4K]", "| Lyrics", "ft. X") so LRCLIB's exact match has a chance.
pub fn clean_title(title: &str) -> String {
    const NOISE: [&str; 14] = [
        "official",
        "video",
        "audio",
        "lyric",
        "visualizer",
        "visualiser",
        "mv",
        "hd",
        "4k",
        "remaster",
        "music",
        "clip",
        "explicit",
        "live",
    ];
    let mut out = String::new();
    let mut depth = 0usize;
    let mut group = String::new();
    for c in title.chars() {
        match c {
            '(' | '[' => {
                if depth == 0 {
                    group.clear();
                }
                depth += 1;
                if depth > 1 {
                    group.push(c);
                }
            }
            ')' | ']' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    let g = group.to_lowercase();
                    if !NOISE
                        .iter()
                        .any(|n| g.split(|c: char| !c.is_alphanumeric()).any(|w| w == *n))
                    {
                        out.push('(');
                        out.push_str(&group);
                        out.push(')');
                    }
                } else {
                    group.push(c);
                }
            }
            _ if depth > 0 => group.push(c),
            _ => out.push(c),
        }
    }
    let mut s = out.as_str();
    if let Some((a, _)) = s.split_once(" | ") {
        s = a;
    }
    let lower = s.to_lowercase();
    for sep in [" ft. ", " feat. ", " ft ", " feat "] {
        if let Some(i) = lower.find(sep) {
            s = &s[..i];
            break;
        }
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// "Artist - Title" uploads: split when the artist field is the channel.
fn split_artist(track: &Track) -> (String, String) {
    let artist = track.artists.first().cloned().unwrap_or_default();
    let title = clean_title(&track.title);
    if track.is_video {
        if let Some((a, t)) = title.split_once(" - ") {
            return (a.trim().to_string(), t.trim().to_string());
        }
    }
    (artist, title)
}

fn to_lyrics(r: &Record) -> Option<Lyrics> {
    if let Some(s) = r.synced.as_deref().filter(|s| !s.trim().is_empty()) {
        let mut l = crate::innertube::lyrics_from_body(s, "");
        if l.synced && !l.lines.is_empty() {
            l.source = Some("Lyrics from LRCLIB".into());
            return Some(l);
        }
    }
    if let Some(p) = r.plain.as_deref().filter(|s| !s.trim().is_empty()) {
        let mut l = crate::innertube::lyrics_from_body(p, "");
        l.source = Some("Lyrics from LRCLIB".into());
        if !l.lines.is_empty() {
            return Some(l);
        }
    }
    None
}

/// Pick the best search hit: close duration first, synced over plain.
fn best<'a>(hits: &'a [Record], duration: Option<u32>, title: &str) -> Option<&'a Record> {
    let t = title.to_lowercase();
    hits.iter()
        .filter(|r| !r.instrumental)
        .filter(|r| match (duration, r.duration) {
            (Some(d), Some(x)) => (d as f64 - x).abs() <= 4.0,
            _ => true,
        })
        .max_by_key(|r| {
            let synced = r.synced.as_deref().is_some_and(|s| !s.trim().is_empty()) as i32;
            let name = r
                .track_name
                .as_deref()
                .is_some_and(|n| n.to_lowercase() == t) as i32;
            let close = match (duration, r.duration) {
                (Some(d), Some(x)) => 10 - ((d as f64 - x).abs() * 2.0) as i32,
                _ => 0,
            };
            synced * 100 + name * 20 + close
        })
}

async fn query(track: &Track) -> Option<Lyrics> {
    let (artist, title) = split_artist(track);
    if title.is_empty() {
        return None;
    }
    let http = client();
    if !artist.is_empty() {
        let mut q: Vec<(&str, String)> = vec![
            ("track_name", title.clone()),
            ("artist_name", artist.clone()),
        ];
        if let Some(a) = track.album.as_deref().filter(|a| !a.is_empty()) {
            q.push(("album_name", a.to_string()));
        }
        if let Some(d) = track.duration.filter(|d| *d > 0) {
            q.push(("duration", d.to_string()));
        }
        if let Ok(r) = http.get(format!("{API}/get")).query(&q).send().await {
            if r.status().is_success() {
                if let Ok(rec) = r.json::<Record>().await {
                    if rec.instrumental {
                        return None;
                    }
                    if rec.synced.as_deref().is_some_and(|s| !s.trim().is_empty()) {
                        return to_lyrics(&rec);
                    }
                }
            }
        }
    }
    let mut q = vec![("track_name", title.clone())];
    if !artist.is_empty() {
        q.push(("artist_name", artist));
    }
    let hits: Vec<Record> = http
        .get(format!("{API}/search"))
        .query(&q)
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;
    best(&hits, track.duration, &title).and_then(to_lyrics)
}

/// Lyrics for `track` from LRCLIB, cached per video id for the session.
pub async fn lyrics(track: &Track) -> Option<Lyrics> {
    if let Some(hit) = cache().lock().get(&track.id) {
        return hit.clone();
    }
    let got = query(track).await;
    crate::diag::debug(format!(
        "lrclib {}: {}",
        track.id,
        match &got {
            Some(l) if l.synced => format!("synced {} lines", l.lines.len()),
            Some(l) => format!("plain {} lines", l.lines.len()),
            None => "none".into(),
        }
    ));
    let mut c = cache().lock();
    if c.len() >= CACHE_MAX {
        c.clear();
    }
    c.insert(track.id.clone(), got.clone());
    got
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_video_noise() {
        assert_eq!(
            clean_title("Never Gonna Give You Up (Official Video) [4K Remaster]"),
            "Never Gonna Give You Up"
        );
        assert_eq!(clean_title("Song (Acoustic)"), "Song (Acoustic)");
        assert_eq!(clean_title("Song ft. Someone | Lyric Video"), "Song");
        assert_eq!(
            clean_title("Despacito (Official Music Video) ft. Daddy Yankee"),
            "Despacito"
        );
    }

    #[test]
    fn splits_artist_from_video_titles() {
        let t = Track {
            title: "Rick Astley - Never Gonna Give You Up (Official Video)".into(),
            artists: vec!["RickAstleyVEVO".into()],
            is_video: true,
            ..Default::default()
        };
        assert_eq!(
            split_artist(&t),
            ("Rick Astley".into(), "Never Gonna Give You Up".into())
        );
    }

    #[test]
    fn prefers_synced_with_matching_duration() {
        let rec = |d: f64, s: Option<&str>| Record {
            track_name: Some("x".into()),
            duration: Some(d),
            instrumental: false,
            plain: Some("a".into()),
            synced: s.map(String::from),
        };
        let hits = vec![
            rec(200.0, None),
            rec(300.0, Some("[00:01.00]a")),
            rec(201.0, Some("[00:01.00]b")),
        ];
        let b = best(&hits, Some(200), "x").unwrap();
        assert_eq!(b.duration, Some(201.0));
        let l = to_lyrics(b).unwrap();
        assert!(l.synced);
        assert_eq!(l.lines[0].start_ms, Some(1000));
    }

    /// Hits the network: `cargo test --lib lrclib -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn finds_a_real_song() {
        let t = Track {
            id: "dQw4w9WgXcQ".into(),
            title: "Never Gonna Give You Up".into(),
            artists: vec!["Rick Astley".into()],
            duration: Some(213),
            ..Default::default()
        };
        let l = lyrics(&t).await.expect("lrclib result");
        assert!(l.synced && l.lines.len() > 20);
        let v = Track {
            id: "x".into(),
            title: "Rick Astley - Never Gonna Give You Up (Official Video) (4K Remaster)".into(),
            artists: vec!["Rick Astley".into()],
            duration: Some(213),
            is_video: true,
            ..Default::default()
        };
        assert!(lyrics(&v).await.is_some_and(|l| l.synced));
    }
}
