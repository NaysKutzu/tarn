//! System tray icon (desktop only): show/hide, play/pause, next, previous,
//! like and quit, with the current track in the tooltip. Optional; off by
//! default. "Close to tray" hides the main window instead of quitting.

use std::sync::Arc;

use parking_lot::Mutex;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};

const TRAY_ID: &str = "tarn-tray";

struct Items {
    play: MenuItem<Wry>,
    like: MenuItem<Wry>,
    /// (tooltip, playing, liked) last pushed to the OS.
    last: (String, bool, bool),
}

static ITEMS: Mutex<Option<Items>> = Mutex::new(None);
/// Building the tray panicked once (e.g. no appindicator library on Linux);
/// don't try again this session.
static BROKEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Create or remove the tray icon to match the setting. Safe to call from
/// any thread; the work happens on the main thread.
pub fn apply(handle: &AppHandle, enabled: bool) {
    let h = handle.clone();
    let _ = handle.run_on_main_thread(move || {
        let exists = h.tray_by_id(TRAY_ID).is_some();
        if enabled && !exists {
            if BROKEN.load(std::sync::atomic::Ordering::Relaxed) {
                return;
            }
            // tray-icon panics on Linux when no (ayatana-)appindicator
            // library is installed; keep the app alive and just skip it.
            let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| build(&h)));
            match built {
                Ok(Ok(())) => {}
                Ok(Err(e)) => eprintln!("tray: {e}"),
                Err(_) => {
                    BROKEN.store(true, std::sync::atomic::Ordering::Relaxed);
                    eprintln!("tray: not available on this system");
                }
            }
        } else if !enabled && exists {
            *ITEMS.lock() = None;
            let _ = h.remove_tray_by_id(TRAY_ID);
        }
    });
}

/// True while the tray icon is up (so closing the window can hide to it).
pub fn active(handle: &AppHandle) -> bool {
    ITEMS.lock().is_some() && handle.tray_by_id(TRAY_ID).is_some()
}

fn build(h: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(h, "show", "Show / hide Tarn", true, None::<&str>)?;
    let play = MenuItem::with_id(h, "play", "Play", true, None::<&str>)?;
    let next = MenuItem::with_id(h, "next", "Next", true, None::<&str>)?;
    let prev = MenuItem::with_id(h, "prev", "Previous", true, None::<&str>)?;
    let like = MenuItem::with_id(h, "like", "Like", true, None::<&str>)?;
    let quit = MenuItem::with_id(h, "quit", "Quit Tarn", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(h)?;
    let sep2 = PredefinedMenuItem::separator(h)?;
    let menu = Menu::with_items(h, &[&show, &sep1, &play, &next, &prev, &like, &sep2, &quit])?;
    let mut b = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip("Tarn")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, ev| on_menu(app, ev.id().as_ref()))
        .on_tray_icon_event(|tray, ev| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = ev
            {
                toggle_window(tray.app_handle());
            }
        });
    if let Some(icon) = h.default_window_icon() {
        b = b.icon(icon.clone());
    }
    b.build(h)?;
    *ITEMS.lock() = Some(Items {
        play,
        like,
        last: (String::new(), false, false),
    });
    Ok(())
}

fn app_state(h: &AppHandle) -> Option<Arc<crate::App>> {
    h.try_state::<Arc<crate::App>>().map(|s| s.inner().clone())
}

fn on_menu(h: &AppHandle, id: &str) {
    match id {
        "show" => toggle_window(h),
        "quit" => h.exit(0),
        "like" => {
            // The frontend owns like state (optimistic UI, local likes for
            // signed-out users), so let it handle the click.
            let _ = h.emit("tray", "like");
        }
        "play" | "next" | "prev" => {
            let Some(app) = app_state(h) else { return };
            use souvlaki::MediaControlEvent as E;
            let ev = match id {
                "play" => E::Toggle,
                "next" => E::Next,
                _ => E::Previous,
            };
            crate::mpris::dispatch(&app, ev);
        }
        _ => {}
    }
}

pub fn show_window(h: &AppHandle) {
    if let Some(w) = h.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn toggle_window(h: &AppHandle) {
    let Some(w) = h.get_webview_window("main") else { return };
    let visible = w.is_visible().unwrap_or(false);
    let minimized = w.is_minimized().unwrap_or(false);
    if visible && !minimized {
        crate::persist::save(h);
        let _ = w.hide();
    } else {
        show_window(h);
    }
}

/// Reflect the player in the menu and tooltip (cheap when nothing changed).
pub fn update(h: &AppHandle, st: &crate::models::PlayerState) {
    let tip = match st.track.as_ref() {
        Some(t) if t.artists.is_empty() => format!("{}\nTarn", t.title),
        Some(t) => format!("{} - {}\nTarn", t.title, t.artists.join(", ")),
        None => "Tarn".to_string(),
    };
    let now = (tip, st.playing, st.liked);
    // Decide under the lock, talk to the OS after releasing it: the menu
    // calls wait on the main thread, which may want this lock in `apply`.
    let (play, like, last) = {
        let mut g = ITEMS.lock();
        let Some(items) = g.as_mut() else { return };
        if now == items.last {
            return;
        }
        let last = std::mem::replace(&mut items.last, now.clone());
        (items.play.clone(), items.like.clone(), last)
    };
    let first = last.0.is_empty();
    if first || now.1 != last.1 {
        let _ = play.set_text(if now.1 { "Pause" } else { "Play" });
    }
    if first || now.2 != last.2 {
        let _ = like.set_text(if now.2 { "Remove like" } else { "Like" });
    }
    if now.0 != last.0 {
        if let Some(tray) = h.tray_by_id(TRAY_ID) {
            // Tooltips are limited to 127 chars on Windows.
            let tip: String = now.0.chars().take(120).collect();
            let _ = tray.set_tooltip(Some(tip));
        }
    }
}
