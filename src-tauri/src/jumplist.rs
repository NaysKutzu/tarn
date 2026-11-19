//! Windows taskbar Jump List: "Recently played" like Spotify.
//!
//! Each entry is an IShellLink that re-launches Tarn with `--play <id>`.
//! The running instance (via single-instance) or a cold start picks that up.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use parking_lot::Mutex;

use crate::models::Track;

const MAX: usize = 8;
const FILE: &str = "jumplist-recent.json";

/// In-memory + on-disk ring of recently played tracks (newest first).
pub struct Recent {
    path: PathBuf,
    items: Mutex<Vec<Track>>,
    /// Last id pushed this session - skip no-op updates every emit tick.
    last_id: Mutex<String>,
}

impl Recent {
    pub fn open(dir: &Path) -> Self {
        let path = dir.join(FILE);
        let items = load(&path);
        Self {
            path,
            items: Mutex::new(items),
            last_id: Mutex::new(String::new()),
        }
    }

    /// Seed from local listen log when the jump-list file is empty.
    pub fn seed_from_listens(&self, listens: &[crate::stats::Listen]) {
        let mut items = self.items.lock();
        if !items.is_empty() {
            return;
        }
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for l in listens.iter().rev() {
            if l.track.id.is_empty() || !seen.insert(l.track.id.clone()) {
                continue;
            }
            out.push(l.track.clone());
            if out.len() >= MAX {
                break;
            }
        }
        if out.is_empty() {
            return;
        }
        save(&self.path, &out);
        *items = out;
    }

    pub fn list(&self) -> Vec<Track> {
        self.items.lock().clone()
    }

    pub fn get(&self, id: &str) -> Option<Track> {
        self.items.lock().iter().find(|t| t.id == id).cloned()
    }

    /// Record a play. Returns true when the list changed (caller should publish).
    pub fn note(&self, track: &Track) -> bool {
        if track.id.is_empty() {
            return false;
        }
        {
            let mut last = self.last_id.lock();
            if *last == track.id {
                return false;
            }
            *last = track.id.clone();
        }
        let mut items = self.items.lock();
        items.retain(|t| t.id != track.id);
        let mut t = track.clone();
        t.ensure_cover();
        items.insert(0, t);
        items.truncate(MAX);
        save(&self.path, &items);
        true
    }
}

fn load(path: &Path) -> Vec<Track> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<Track>>(&text)
        .unwrap_or_default()
        .into_iter()
        .filter(|t| !t.id.is_empty())
        .take(MAX)
        .collect()
}

fn save(path: &Path, items: &[Track]) {
    if let Ok(text) = serde_json::to_string(items) {
        let _ = std::fs::write(path, text);
    }
}

/// `--play <id>` or `--play=<id>` from argv (with or without exe path).
pub fn play_id_from_args(args: &[String]) -> Option<String> {
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a == "--play" {
            let id = args.get(i + 1)?.trim();
            if !id.is_empty() && !id.starts_with('-') {
                return Some(id.to_string());
            }
            return None;
        }
        if let Some(id) = a.strip_prefix("--play=") {
            let id = id.trim();
            if !id.is_empty() {
                return Some(id.to_string());
            }
        }
        i += 1;
    }
    None
}

pub fn publish(tracks: &[Track]) {
    #[cfg(windows)]
    win::publish(tracks);
    #[cfg(not(windows))]
    let _ = tracks;
}

#[cfg(windows)]
mod win {
    use super::*;
    use windows::{
        core::{Interface, HSTRING, PCWSTR},
        Win32::{
            Storage::EnhancedStorage::PKEY_Title,
            System::Com::{
                CoCreateInstance, CoInitializeEx, CoUninitialize, StructuredStorage::PROPVARIANT,
                CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
            },
            UI::Shell::{
                Common::{IObjectArray, IObjectCollection},
                DestinationList, EnumerableObjectCollection, ICustomDestinationList, IShellLinkW,
                PropertiesSystem::IPropertyStore, ShellLink,
            },
        },
    };

    pub fn publish(tracks: &[Track]) {
        if let Err(e) = publish_inner(tracks) {
            eprintln!("jumplist: {e}");
        }
    }

    fn publish_inner(tracks: &[Track]) -> windows::core::Result<()> {
        let exe = std::env::current_exe()
            .map_err(|_| windows::core::Error::from_win32())?;
        let exe = HSTRING::from(exe.as_os_str());

        // Best-effort: main thread may already own COM.
        let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        let need_uninit = com.is_ok();

        let result = (|| {
            let list: ICustomDestinationList =
                unsafe { CoCreateInstance(&DestinationList, None, CLSCTX_INPROC_SERVER) }?;
            unsafe { list.SetAppID(&HSTRING::from(crate::win::APP_USER_MODEL_ID)) }?;

            let mut slots = 0u32;
            let removed: IObjectArray = unsafe { list.BeginList(&mut slots) }?;
            let skip = removed_play_ids(&removed);

            let collection: IObjectCollection =
                unsafe { CoCreateInstance(&EnumerableObjectCollection, None, CLSCTX_INPROC_SERVER) }?;

            let mut added = 0u32;
            for t in tracks {
                if skip.contains(&t.id) {
                    continue;
                }
                let link = make_link(&exe, t)?;
                unsafe { collection.AddObject(&link) }?;
                added += 1;
                if added >= slots.max(1) {
                    break;
                }
            }

            if added > 0 {
                unsafe {
                    list.AppendCategory(&HSTRING::from("Recently played"), &collection)?;
                }
            }
            unsafe { list.CommitList() }?;
            Ok(())
        })();

        if need_uninit {
            unsafe { CoUninitialize() };
        }
        result
    }

    fn removed_play_ids(removed: &IObjectArray) -> HashSet<String> {
        let mut out = HashSet::new();
        let Ok(count) = (unsafe { removed.GetCount() }) else {
            return out;
        };
        for i in 0..count {
            let Ok(link) = (unsafe { removed.GetAt::<IShellLinkW>(i) }) else {
                continue;
            };
            let mut buf = [0u16; 2048];
            if unsafe { link.GetArguments(&mut buf) }.is_err() {
                continue;
            }
            let args = String::from_utf16_lossy(&buf)
                .trim_end_matches('\0')
                .to_string();
            // Arguments are a single string: `--play VIDEOID`
            let parts: Vec<String> = args
                .split_whitespace()
                .map(|s| s.to_string())
                .collect();
            if let Some(id) = play_id_from_args(&parts) {
                out.insert(id);
            }
        }
        out
    }

    fn make_link(exe: &HSTRING, track: &Track) -> windows::core::Result<IShellLinkW> {
        let link: IShellLinkW =
            unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }?;
        unsafe {
            link.SetPath(PCWSTR(exe.as_ptr()))?;
            link.SetArguments(&HSTRING::from(format!("--play {}", track.id)))?;
            link.SetIconLocation(PCWSTR(exe.as_ptr()), 0)?;
        }

        let label = display_label(track);
        let tip = {
            let artists = track.artists.join(", ");
            if artists.is_empty() {
                track.title.clone()
            } else {
                format!("{} — {}", track.title, artists)
            }
        };
        unsafe { link.SetDescription(&HSTRING::from(tip)) }?;

        let store: IPropertyStore = link.cast()?;
        let title = PROPVARIANT::from(label.as_str());
        unsafe {
            store.SetValue(&PKEY_Title, &title)?;
            store.Commit()?;
        }
        Ok(link)
    }

    fn display_label(track: &Track) -> String {
        let title = track.title.trim();
        if title.is_empty() {
            return track.id.clone();
        }
        let artists = track.artists.join(", ");
        if artists.is_empty() {
            title.to_string()
        } else {
            // Spotify-style: song first, then artist (truncated if huge).
            let s = format!("{title} — {artists}");
            if s.chars().count() > 64 {
                format!("{title} — {}", truncate(&artists, 24))
            } else {
                s
            }
        }
    }

    fn truncate(s: &str, max: usize) -> String {
        let n = s.chars().count();
        if n <= max {
            s.to_string()
        } else {
            let t: String = s.chars().take(max.saturating_sub(1)).collect();
            format!("{t}…")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_play_flag() {
        assert_eq!(
            play_id_from_args(&[
                "tarn.exe".into(),
                "--play".into(),
                "abcdefghijk".into()
            ]),
            Some("abcdefghijk".into())
        );
        assert_eq!(
            play_id_from_args(&["tarn.exe".into(), "--play=xyz".into()]),
            Some("xyz".into())
        );
        assert_eq!(play_id_from_args(&["tarn.exe".into()]), None);
    }

    #[test]
    fn recent_dedupes_and_caps() {
        let dir = std::env::temp_dir().join(format!("tarn-jl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let r = Recent::open(&dir);
        assert!(r.note(&Track {
            id: "a".into(),
            title: "A".into(),
            ..Default::default()
        }));
        assert!(!r.note(&Track {
            id: "a".into(),
            title: "A".into(),
            ..Default::default()
        }));
        for i in 0..12 {
            r.note(&Track {
                id: format!("t{i}"),
                title: format!("T{i}"),
                ..Default::default()
            });
        }
        assert_eq!(r.list().len(), MAX);
        assert_eq!(r.list()[0].id, "t11");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
