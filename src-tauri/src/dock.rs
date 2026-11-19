//! macOS Dock menu: right-click the Tarn icon for Play / Previous / Next / Like.
//! Closest native cousin of the Windows taskbar thumbnail toolbar.

use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager};

use crate::models::PlayerState;

#[cfg(not(target_os = "macos"))]
pub fn start(_: Arc<crate::App>, _: &AppHandle) {}

#[cfg(not(target_os = "macos"))]
pub fn update(_: &AppHandle, _: &PlayerState) {}

#[cfg(target_os = "macos")]
pub fn start(app: Arc<crate::App>, handle: &AppHandle) {
    let _ = app;
    handle.on_menu_event(|app_handle, ev| {
        let id = ev.id().as_ref();
        if id.starts_with("dock-") {
            on_click(app_handle, id);
        }
    });
    let h = handle.clone();
    let _ = handle.run_on_main_thread(move || {
        if let Err(e) = install(&h) {
            eprintln!("dock menu: {e}");
        }
    });
}

#[cfg(target_os = "macos")]
pub fn update(handle: &AppHandle, st: &PlayerState) {
    use parking_lot::Mutex;
    static LAST: Mutex<(String, bool, bool)> = Mutex::new((String::new(), false, false));

    let tip = match st.track.as_ref() {
        Some(t) if t.artists.is_empty() => t.title.clone(),
        Some(t) => format!("{} - {}", t.title, t.artists.join(", ")),
        None => String::new(),
    };
    let now = (tip, st.playing, st.liked);
    {
        let mut last = LAST.lock();
        if *last == now {
            return;
        }
        *last = now.clone();
    }
    let _ = handle.run_on_main_thread(move || update_main(now));
}

#[cfg(target_os = "macos")]
struct DockItems {
    track: muda::MenuItem,
    play: muda::MenuItem,
    like: muda::MenuItem,
    /// Keep the muda menu (and its NSMenu) alive for the process.
    _menu: muda::Menu,
    /// Extra retain so AppKit's dock menu pointer stays valid.
    _ns: objc2::rc::Retained<objc2_app_kit::NSMenu>,
    last: (String, bool, bool),
}

/// Leaked on the UI thread; only read/written from `run_on_main_thread`.
#[cfg(target_os = "macos")]
static STATE: std::sync::atomic::AtomicPtr<DockItems> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

#[cfg(target_os = "macos")]
fn install(handle: &AppHandle) -> Result<(), String> {
    use muda::{ContextMenu, Menu, MenuItem, PredefinedMenuItem};
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2_app_kit::{NSApplication, NSMenu};
    use objc2_foundation::MainThreadMarker;
    use std::sync::atomic::Ordering;

    let _ = handle;
    let mtm = MainThreadMarker::new().ok_or("dock menu: not on main thread")?;

    let track = MenuItem::with_id("dock-track", "Tarn", false, None);
    let play = MenuItem::with_id("dock-play", "Play", true, None);
    let next = MenuItem::with_id("dock-next", "Next", true, None);
    let prev = MenuItem::with_id("dock-prev", "Previous", true, None);
    let like = MenuItem::with_id("dock-like", "Like", true, None);
    let show = MenuItem::with_id("dock-show", "Show Tarn", true, None);

    let menu = Menu::new();
    menu.append(&track).map_err(|e| e.to_string())?;
    menu.append(&PredefinedMenuItem::separator())
        .map_err(|e| e.to_string())?;
    menu.append(&play).map_err(|e| e.to_string())?;
    menu.append(&prev).map_err(|e| e.to_string())?;
    menu.append(&next).map_err(|e| e.to_string())?;
    menu.append(&like).map_err(|e| e.to_string())?;
    menu.append(&PredefinedMenuItem::separator())
        .map_err(|e| e.to_string())?;
    menu.append(&show).map_err(|e| e.to_string())?;

    let ptr = menu.ns_menu() as *mut NSMenu;
    let ns = unsafe { Retained::retain(ptr) }.ok_or("dock menu: retain NSMenu failed")?;

    let ns_app = NSApplication::sharedApplication(mtm);
    // Undocumented but stable; Electron and similar apps use the same hook.
    let _: () = unsafe { msg_send![&*ns_app, setDockMenu: &*ns] };

    let boxed = Box::new(DockItems {
        track,
        play,
        like,
        _menu: menu,
        _ns: ns,
        last: (String::new(), false, false),
    });
    STATE.store(Box::into_raw(boxed), Ordering::Release);
    eprintln!("dock menu: attached");
    Ok(())
}

#[cfg(target_os = "macos")]
fn update_main(now: (String, bool, bool)) {
    use std::sync::atomic::Ordering;
    let p = STATE.load(Ordering::Acquire);
    if p.is_null() {
        return;
    }
    let s = unsafe { &mut *p };
    if now == s.last {
        return;
    }
    let last = std::mem::replace(&mut s.last, now.clone());
    if now.0 != last.0 {
        let label = if now.0.is_empty() {
            "Tarn".to_string()
        } else {
            now.0.chars().take(40).collect()
        };
        s.track.set_text(label);
        s.track.set_enabled(!now.0.is_empty());
    }
    if now.1 != last.1 {
        s.play.set_text(if now.1 { "Pause" } else { "Play" });
    }
    if now.2 != last.2 {
        s.like.set_text(if now.2 { "Remove like" } else { "Like" });
    }
}

#[cfg(target_os = "macos")]
fn on_click(h: &AppHandle, id: &str) {
    match id {
        "dock-show" => {
            if let Some(w) = h.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }
        "dock-like" => {
            let _ = h.emit("tray", "like");
        }
        "dock-play" | "dock-next" | "dock-prev" => {
            let Some(app) = h.try_state::<Arc<crate::App>>().map(|s| s.inner().clone()) else {
                return;
            };
            use souvlaki::MediaControlEvent as E;
            let ev = match id {
                "dock-play" => E::Toggle,
                "dock-next" => E::Next,
                _ => E::Previous,
            };
            crate::mpris::dispatch(&app, ev);
        }
        _ => {}
    }
}
