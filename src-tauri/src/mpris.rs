//! OS media controls: MPRIS on Linux, SMTC on Windows, Now Playing on macOS.
//! Failures are non-fatal. Media-key events are applied directly to the
//! player (and to next/prev via the async advance path) so they do not
//! depend on a frontend event listener.

use std::sync::Arc;
use std::time::Duration;

use tauri::AppHandle;

use crate::models::Track;
use crate::player::NextChoice;

/// Native window handle required by Windows SMTC. Ignored on other OS.
/// Native HWND stored as usize so it can cross threads. Ignored off Windows.
pub type WindowHandle = Option<usize>;

/// Set once the MPRIS / SMTC session is registered. On Linux the desktop
/// already routes media keys through it, so a second X11 key grab would only
/// double-fire (and does nothing on Wayland).
static MEDIA_SESSION_OK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn start(app: Arc<crate::App>, handle: AppHandle, hwnd: WindowHandle) {
    let _ = &handle;
    // Windows: SystemMediaTransportControls must be created and attached on
    // a thread that pumps window messages - ideally the window's UI thread.
    // A background thread with just `sleep` never delivers ButtonPressed,
    // which is why media keys appeared to "do nothing" even with a valid HWND.
    #[cfg(windows)]
    {
        let app_w = app.clone();
        let handle_w = handle.clone();
        match start_smtc_ui(app_w, handle_w, hwnd) {
            Ok(()) => eprintln!("media controls: SMTC attached on the UI thread"),
            Err(e) => {
                eprintln!("media controls: UI-thread attach failed: {e}; trying STA pump");
                let app_s = app.clone();
                std::thread::Builder::new()
                    .name("tarn-smtc".into())
                    .spawn(move || {
                        if let Err(e) = run_windows_smtc(app_s, hwnd) {
                            eprintln!("media controls (sta): {e}");
                        }
                    })
                    .ok();
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
        let app_m = app.clone();
        std::thread::Builder::new()
            .name("tarn-mpris".into())
            .spawn(move || {
                if let Err(e) = run_souvlaki(app_m, None) {
                    eprintln!("media controls: {e}");
                }
            })
            .ok();
    }

    // Global media-key shortcuts as a fallback when the OS does not
    // route keys through MPRIS/SMTC (some compositors, unlocked HUDs).
    // Registered one-by-one: register_all is all-or-nothing, and on Linux
    // MediaPlayPause often has no scancode, which disabled every key.
    // The same thread also owns the user's own global hotkeys: the hotkey
    // event receiver is process-wide, so a second loop would steal events.
    let app_h = app.clone();
    let handle_h = handle.clone();
    std::thread::Builder::new()
        .name("tarn-mediakeys".into())
        .spawn(move || {
            #[allow(unused_mut)]
            let mut media_keys = true;
            #[cfg(target_os = "linux")]
            {
                // Give MPRIS time to register, then prefer it over key grabs.
                std::thread::sleep(Duration::from_secs(4));
                if MEDIA_SESSION_OK.load(std::sync::atomic::Ordering::Relaxed) {
                    eprintln!("media key shortcuts: skipped, MPRIS handles media keys");
                    media_keys = false;
                }
            }
            if let Err(e) = run_global_hotkeys(app_h, handle_h, media_keys) {
                eprintln!("media key shortcuts: {e}");
            }
        })
        .ok();
}

fn dispatch(app: &Arc<crate::App>, ev: souvlaki::MediaControlEvent) {
    use souvlaki::{MediaControlEvent, SeekDirection};
    {
        let snap = app.player.snapshot();
        crate::diag::log(format!(
            "media key: {ev:?} (playing={}, pos={:.1})",
            snap.playing, snap.position
        ));
    }
    // The desktop picks Play/Pause from the PlaybackStatus it last saw, which
    // lags behind a quick double press - it then sends the same command twice.
    // A repeat of the same Play/Pause within a short window means "toggle".
    let ev = {
        use std::sync::Mutex;
        static LAST: Mutex<Option<(bool, std::time::Instant)>> = Mutex::new(None);
        let now = std::time::Instant::now();
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        let kind = match ev {
            MediaControlEvent::Play => Some(true),
            MediaControlEvent::Pause => Some(false),
            _ => None,
        };
        match kind {
            Some(k) => {
                let repeat = matches!(*last, Some((lk, t)) if lk == k && now.duration_since(t) < Duration::from_millis(700));
                // What we actually do: flip on a quick repeat, else as asked.
                let do_play = if repeat { !k } else { k };
                *last = Some((do_play, now));
                if do_play { MediaControlEvent::Play } else { MediaControlEvent::Pause }
            }
            None => ev,
        }
    };
    match ev {
        MediaControlEvent::Toggle => app.player.toggle(),
        MediaControlEvent::Play => app.player.resume(),
        MediaControlEvent::Pause => app.player.pause(),
        MediaControlEvent::Next => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = crate::advance(&app, true).await {
                    eprintln!("media next: {e}");
                }
            });
        }
        MediaControlEvent::Previous => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                match app.player.prev_choice() {
                    NextChoice::SeekZero => app.player.seek(0.0),
                    NextChoice::Play(i) => {
                        let (q, _) = app.player.queue_and_index();
                        if let Some(t) = q.get(i).cloned() {
                            let _ = crate::start_playback(&app, t, q, i).await;
                        }
                    }
                    _ => {}
                }
            });
        }
        MediaControlEvent::Stop => app.player.stop(),
        MediaControlEvent::SetPosition(p) => app.player.seek(p.0.as_secs_f64()),
        MediaControlEvent::Seek(d) => {
            let snap = app.player.snapshot();
            let delta = match d {
                SeekDirection::Forward => 10.0,
                SeekDirection::Backward => -10.0,
            };
            app.player.seek((snap.position + delta).max(0.0));
        }
        MediaControlEvent::SeekBy(d, dur) => {
            let snap = app.player.snapshot();
            let secs = dur.as_secs_f64();
            let delta = match d {
                SeekDirection::Forward => secs,
                SeekDirection::Backward => -secs,
            };
            app.player.seek((snap.position + delta).max(0.0));
        }
        MediaControlEvent::SetVolume(v) => app.player.set_volume(v as f32),
        MediaControlEvent::Quit | MediaControlEvent::Raise | MediaControlEvent::OpenUri(_) => {}
    }
}

fn run_souvlaki(app: Arc<crate::App>, hwnd: WindowHandle) -> Result<(), String> {
    use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, PlatformConfig};

    #[cfg(windows)]
    let hwnd = Some(
        hwnd.ok_or_else(|| {
            "Windows SMTC requires a window HWND; none was provided".to_string()
        })? as *mut std::ffi::c_void,
    );
    #[cfg(not(windows))]
    let hwnd = {
        let _ = hwnd;
        None
    };

    // Short, valid MPRIS instance name: org.mpris.MediaPlayer2.tarn
    // (dots inside the instance part confuse some playerctl versions).
    // Session bus may not be ready at the exact moment of first startup.
    let mut last_err = None;
    let mut controls = None;
    for attempt in 0..8 {
        let config = PlatformConfig {
            dbus_name: "tarn",
            display_name: "Tarn",
            hwnd,
        };
        match MediaControls::new(config) {
            Ok(c) => {
                controls = Some(c);
                break;
            }
            Err(e) => {
                last_err = Some(format!("{e:?}"));
                std::thread::sleep(Duration::from_millis(250 * (attempt + 1) as u64));
            }
        }
    }
    let mut controls = controls.ok_or_else(|| {
        last_err.unwrap_or_else(|| "failed to create media controls".into())
    })?;

    let (tx, rx) = std::sync::mpsc::channel::<MediaControlEvent>();
    controls
        .attach(move |e| {
            let _ = tx.send(e);
        })
        .map_err(|e| format!("attach: {e:?}"))?;

    MEDIA_SESSION_OK.store(true, std::sync::atomic::Ordering::Relaxed);
    eprintln!("media controls: registered (MPRIS name org.mpris.MediaPlayer2.tarn / SMTC)");

    // Paused (not Stopped) so OS media keys are routed to us before play.
    let _ = controls.set_playback(MediaPlayback::Paused { progress: None });
    let _ = controls.set_metadata(MediaMetadata {
        title: Some("Tarn"),
        artist: Some(""),
        ..Default::default()
    });

    let mut last_id: Option<String> = None;
    let mut last_playing: Option<bool> = None;
    // Position is extrapolated by the OS from the last value we sent, so only
    // resend it on a real jump (seek). Re-announcing every 500ms spams
    // PropertiesChanged and makes some desktops hand the media keys to
    // another player.
    let mut sent_pos: f64 = 0.0;
    let mut sent_at = std::time::Instant::now();
    // Metadata carries the cover URL: the OS re-downloads the art every time
    // it is set, so only send it when the track (or its duration) changes.
    let mut last_meta: Option<(String, u64)> = None;

    loop {
        // Wake the moment a key arrives so the new status is published at
        // once (the desktop reads it to decide between Play and Pause).
        if let Ok(ev) = rx.recv_timeout(Duration::from_millis(100)) {
            dispatch(&app, ev);
            while let Ok(ev) = rx.try_recv() {
                dispatch(&app, ev);
            }
        }

        let snap = app.player.snapshot();
        let playing = snap.playing;
        let id = snap.track.as_ref().map(|t| t.id.clone());
        let expected = if playing {
            sent_pos + sent_at.elapsed().as_secs_f64()
        } else {
            sent_pos
        };
        let jumped = (snap.position - expected).abs() > 2.0;
        let changed = id != last_id || Some(playing) != last_playing || jumped;

        if changed {
            if let Some(t) = &snap.track {
                let dur = snap.duration;
                let meta_key = (t.id.clone(), dur.max(0.0).round() as u64);
                if last_meta.as_ref() != Some(&meta_key) {
                    let artists = t.artists.join(", ");
                    let _ = controls.set_metadata(MediaMetadata {
                        title: Some(&t.title),
                        artist: Some(&artists),
                        album: t.album.as_deref(),
                        duration: if dur > 0.0 {
                            Some(Duration::from_secs_f64(dur))
                        } else {
                            None
                        },
                        cover_url: t.cover.as_deref(),
                    });
                    last_meta = Some(meta_key);
                }
                if id != last_id {
                    maybe_notify(&app, t);
                }
            }
            let progress = Some(souvlaki::MediaPosition(Duration::from_secs_f64(
                snap.position.max(0.0),
            )));
            let _ = controls.set_playback(if playing {
                MediaPlayback::Playing { progress }
            } else {
                MediaPlayback::Paused { progress }
            });
            last_id = id;
            last_playing = Some(playing);
            sent_pos = snap.position.max(0.0);
            sent_at = std::time::Instant::now();
        }

    }
}

fn maybe_notify(app: &crate::App, t: &Track) {
    if !app.notifications_enabled() {
        return;
    }
    let artists = t.artists.join(", ");
    let mut n = notify_rust::Notification::new();
    n.appname(crate::win::APP_NAME)
        .summary(&t.title)
        .body(&artists)
        .timeout(notify_rust::Timeout::Milliseconds(4000));
    #[cfg(windows)]
    {
        n.app_id(crate::win::APP_USER_MODEL_ID);
    }
    if let Some(icon) = crate::notify_icon() {
        n.icon(&icon);
        n.image_path(&icon);
    }
    if let Err(e) = n.show() {
        eprintln!("notify: {e}");
    }
}

/// User-assigned global hotkeys: action name → accelerator (for example
/// "Ctrl+Alt+KeyP"). Sent to the hotkey thread when settings change.
static USER_KEYS: parking_lot::Mutex<Option<std::sync::mpsc::Sender<HotkeyMap>>> =
    parking_lot::Mutex::new(None);
/// "action: reason" for hotkeys that could not be registered.
static HOTKEY_ERRORS: parking_lot::Mutex<Vec<String>> = parking_lot::Mutex::new(Vec::new());

pub type HotkeyMap = std::collections::BTreeMap<String, String>;

/// Actions a global hotkey can trigger.
pub const HOTKEY_ACTIONS: &[&str] = &[
    "play_pause",
    "next",
    "prev",
    "seek_forward",
    "seek_back",
    "volume_up",
    "volume_down",
    "like",
    "show_window",
];

/// Apply a new set of user global hotkeys (no-op until the thread runs;
/// it reads the saved settings when it starts).
pub fn set_user_hotkeys(map: HotkeyMap) {
    if let Some(tx) = USER_KEYS.lock().as_ref() {
        let _ = tx.send(map);
    }
}

pub fn hotkey_errors() -> Vec<String> {
    HOTKEY_ERRORS.lock().clone()
}

/// Parse "Ctrl+Alt+KeyP"-style accelerators (modifiers + a KeyboardEvent
/// `code`). Empty means unassigned.
pub fn parse_accel(s: &str) -> Result<Option<global_hotkey::hotkey::HotKey>, String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(None);
    }
    s.parse::<global_hotkey::hotkey::HotKey>()
        .map(Some)
        .map_err(|e| e.to_string())
}

fn run_action(app: &Arc<crate::App>, handle: &AppHandle, action: &str) {
    use souvlaki::MediaControlEvent as E;
    use tauri::{Emitter, Manager};
    match action {
        "play_pause" => dispatch(app, E::Toggle),
        "next" => dispatch(app, E::Next),
        "prev" => dispatch(app, E::Previous),
        "seek_forward" | "seek_back" => {
            let snap = app.player.snapshot();
            let d = if action == "seek_forward" { 5.0 } else { -5.0 };
            app.player.seek((snap.position + d).max(0.0));
        }
        "volume_up" | "volume_down" => {
            let v = app.player.snapshot().volume;
            let d = if action == "volume_up" { 0.05 } else { -0.05 };
            app.player.set_volume((v + d).clamp(0.0, 1.0));
        }
        // The frontend owns like state (optimistic UI, local likes).
        "like" => {
            let _ = handle.emit("hotkey", "like");
        }
        "show_window" => {
            if let Some(w) = handle.get_webview_window("main") {
                let shown = w.is_visible().unwrap_or(false)
                    && !w.is_minimized().unwrap_or(false)
                    && w.is_focused().unwrap_or(false);
                if shown {
                    let _ = w.minimize();
                } else {
                    let _ = w.unminimize();
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
        }
        _ => {}
    }
}

fn run_global_hotkeys(
    app: Arc<crate::App>,
    handle: AppHandle,
    media_keys: bool,
) -> Result<(), String> {
    use global_hotkey::hotkey::{Code, HotKey, Modifiers};
    use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
    use souvlaki::MediaControlEvent;

    // On Windows, GlobalHotKeyManager creates an HWND and RegisterHotKey posts
    // WM_HOTKEY to *this* thread. Blocking on the channel without a message
    // pump means the WndProc never runs and keys appear "registered" but dead.
    let manager = GlobalHotKeyManager::new().map_err(|e| format!("init: {e}"))?;
    let play = HotKey::new(None::<Modifiers>, Code::MediaPlayPause);
    let stop = HotKey::new(None::<Modifiers>, Code::MediaStop);
    let next = HotKey::new(None::<Modifiers>, Code::MediaTrackNext);
    let prev = HotKey::new(None::<Modifiers>, Code::MediaTrackPrevious);
    // Some keyboards expose these as "Play" rather than PlayPause.
    let play2 = HotKey::new(None::<Modifiers>, Code::MediaPlay);
    let pause2 = HotKey::new(None::<Modifiers>, Code::MediaPause);
    let named = [
        ("MediaPlayPause", play),
        ("MediaStop", stop),
        ("MediaTrackNext", next),
        ("MediaTrackPrevious", prev),
        ("MediaPlay", play2),
        ("MediaPause", pause2),
    ];
    if media_keys {
        for (name, key) in &named {
            match manager.register(*key) {
                Ok(()) => eprintln!("media key shortcuts: registered {name}"),
                Err(e) => eprintln!("media key shortcuts: {name} failed: {e}"),
            }
        }
    }

    let map = |id: u32| -> Option<MediaControlEvent> {
        if !media_keys {
            None
        } else if id == play.id() || id == play2.id() {
            Some(MediaControlEvent::Toggle)
        } else if id == pause2.id() {
            Some(MediaControlEvent::Pause)
        } else if id == stop.id() {
            Some(MediaControlEvent::Stop)
        } else if id == next.id() {
            Some(MediaControlEvent::Next)
        } else if id == prev.id() {
            Some(MediaControlEvent::Previous)
        } else {
            None
        }
    };

    // User hotkeys: (hotkey, action), re-registered when settings change.
    let mut user: Vec<(HotKey, String)> = Vec::new();
    let apply_user = |user: &mut Vec<(HotKey, String)>, want: &HotkeyMap| {
        for (k, _) in user.drain(..) {
            let _ = manager.unregister(k);
        }
        let mut errors = Vec::new();
        for (action, accel) in want {
            if !HOTKEY_ACTIONS.contains(&action.as_str()) {
                continue;
            }
            match parse_accel(accel) {
                Ok(None) => {}
                Ok(Some(k)) => match manager.register(k) {
                    Ok(()) => {
                        eprintln!("global hotkey: {accel} → {action}");
                        user.push((k, action.clone()));
                    }
                    Err(e) => errors.push(format!("{action}: {e}")),
                },
                Err(e) => errors.push(format!("{action}: {e}")),
            }
        }
        for e in &errors {
            eprintln!("global hotkey: {e}");
        }
        *HOTKEY_ERRORS.lock() = errors;
    };
    let (cfg_tx, cfg_rx) = std::sync::mpsc::channel::<HotkeyMap>();
    *USER_KEYS.lock() = Some(cfg_tx);
    let initial = app.settings.lock().global_hotkeys.clone();
    apply_user(&mut user, &initial);

    let rx = GlobalHotKeyEvent::receiver();
    let mut last = std::time::Instant::now() - Duration::from_secs(1);
    loop {
        #[cfg(windows)]
        win_pump::pump();
        while let Ok(want) = cfg_rx.try_recv() {
            apply_user(&mut user, &want);
        }
        #[cfg(windows)]
        let got = rx.try_recv().ok();
        #[cfg(not(windows))]
        let got = rx.recv_timeout(Duration::from_millis(150)).ok();
        let Some(ev) = got else {
            #[cfg(windows)]
            std::thread::sleep(Duration::from_millis(8));
            continue;
        };
        if ev.state != HotKeyState::Pressed {
            continue;
        }
        let now = std::time::Instant::now();
        if now.duration_since(last) < Duration::from_millis(250) {
            continue;
        }
        if let Some(cmd) = map(ev.id) {
            last = now;
            eprintln!("media key (global): {cmd:?}");
            dispatch(&app, cmd);
        } else if let Some((_, action)) = user.iter().find(|(k, _)| k.id() == ev.id) {
            last = now;
            eprintln!("global hotkey: {action}");
            run_action(&app, &handle, action);
        }
    }
}

/// Create and attach SMTC on the calling thread (must pump messages -
/// Tauri's UI thread does). MediaControls stays on that thread; a
/// background loop posts metadata/status updates via `run_on_main_thread`.
#[cfg(windows)]
fn start_smtc_ui(
    app: Arc<crate::App>,
    handle: AppHandle,
    hwnd: WindowHandle,
) -> Result<(), String> {
    use souvlaki::{MediaControls, MediaMetadata, MediaPlayback, PlatformConfig};
    use std::cell::RefCell;

    let hwnd = hwnd.ok_or_else(|| {
        "Windows SMTC requires a window HWND; none was provided".to_string()
    })? as *mut std::ffi::c_void;
    eprintln!("media controls: GetForWindow hwnd={hwnd:?} (UI thread)");

    let mut controls = MediaControls::new(PlatformConfig {
        dbus_name: "tarn",
        display_name: "Tarn",
        hwnd: Some(hwnd),
    })
    .map_err(|e| format!("SMTC new: {e:?}"))?;

    let app_cb = app.clone();
    controls
        .attach(move |e| {
            eprintln!("media key (SMTC callback): {e:?}");
            dispatch(&app_cb, e);
        })
        .map_err(|e| format!("SMTC attach: {e:?}"))?;

    // Prefer Paused over Stopped so Windows routes hardware media keys to us
    // even before the first track is armed (Stopped sessions swallow them).
    let _ = controls.set_playback(MediaPlayback::Paused { progress: None });
    let _ = controls.set_metadata(MediaMetadata {
        title: Some("Tarn"),
        artist: Some(""),
        ..Default::default()
    });

    // Leak a RefCell so we can touch the object only on this thread.
    // `MediaControls` may not be Send; `run_on_main_thread` keeps access here.
    struct MainThreadSlot(RefCell<Option<MediaControls>>);
    // SAFETY: the slot is only ever borrowed inside `run_on_main_thread`
    // closures, so every access happens on the UI thread that created it.
    unsafe impl Sync for MainThreadSlot {}
    unsafe impl Send for MainThreadSlot {}
    let slot: &'static MainThreadSlot =
        Box::leak(Box::new(MainThreadSlot(RefCell::new(Some(controls)))));

    std::thread::Builder::new()
        .name("tarn-smtc-loop".into())
        .spawn(move || {
            let mut last_id: Option<String> = None;
            let mut last_playing: Option<bool> = None;
            let mut last_pos_bucket: u64 = u64::MAX;
            let mut last_meta: Option<(String, u64)> = None;
            loop {
                let snap = app.player.snapshot();
                let playing = snap.playing;
                let id = snap.track.as_ref().map(|t| t.id.clone());
                let pos_bucket = (snap.position.max(0.0) * 2.0) as u64;
                let changed =
                    id != last_id || Some(playing) != last_playing || pos_bucket != last_pos_bucket;
                if changed {
                    let id_changed = id != last_id;
                    last_id = id.clone();
                    last_playing = Some(playing);
                    last_pos_bucket = pos_bucket;
                    if id_changed {
                        if let Some(t) = &snap.track {
                            maybe_notify(&app, t);
                        }
                    }
                    // Only (re)send metadata when the track or duration changed;
                    // it carries the cover URL, which the OS re-downloads.
                    let send_meta = snap.track.as_ref().is_some_and(|t| {
                        let key = (t.id.clone(), snap.duration.max(0.0).round() as u64);
                        let fresh = last_meta.as_ref() != Some(&key);
                        if fresh {
                            last_meta = Some(key);
                        }
                        fresh
                    });
                    if let Err(e) = handle.run_on_main_thread(move || {
                        use souvlaki::{MediaMetadata, MediaPlayback};
                        let mut slot = slot.0.borrow_mut();
                        let Some(controls) = slot.as_mut() else {
                            return;
                        };
                        if let Some(t) = snap.track.as_ref().filter(|_| send_meta) {
                            let artists = t.artists.join(", ");
                            let dur = snap.duration;
                            if let Err(e) = controls.set_metadata(MediaMetadata {
                                title: Some(&t.title),
                                artist: Some(&artists),
                                album: t.album.as_deref(),
                                duration: if dur > 0.0 {
                                    Some(Duration::from_secs_f64(dur))
                                } else {
                                    None
                                },
                                cover_url: t.cover.as_deref(),
                            }) {
                                eprintln!("media controls: set_metadata: {e:?}");
                            }
                        }
                        let progress = Some(souvlaki::MediaPosition(Duration::from_secs_f64(
                            snap.position.max(0.0),
                        )));
                        // SMTC ignores media keys unless status is Playing
                        // or Paused (a Stopped session swallows them). Keep
                        // Paused with no track so keys still reach us.
                        let status = if playing {
                            MediaPlayback::Playing { progress }
                        } else {
                            MediaPlayback::Paused { progress }
                        };
                        if let Err(e) = controls.set_playback(status) {
                            eprintln!("media controls: set_playback: {e:?}");
                        }
                    }) {
                        eprintln!("media controls: run_on_main_thread: {e}");
                    }
                }
                std::thread::sleep(Duration::from_millis(150));
            }
        })
        .map_err(|e| format!("smtc loop: {e}"))?;
    Ok(())
}

/// Fallback: dedicated STA thread with a Win32 message pump, in case
/// attaching on the UI thread failed (COM apartment, etc.).
#[cfg(windows)]
fn run_windows_smtc(app: Arc<crate::App>, hwnd: WindowHandle) -> Result<(), String> {
    win_pump::co_init_sta();
    use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, PlatformConfig};

    let hwnd = hwnd.ok_or_else(|| {
        "Windows SMTC requires a window HWND; none was provided".to_string()
    })? as *mut std::ffi::c_void;
    eprintln!("media controls: STA GetForWindow hwnd={hwnd:?}");

    let mut controls = MediaControls::new(PlatformConfig {
        dbus_name: "tarn",
        display_name: "Tarn",
        hwnd: Some(hwnd),
    })
    .map_err(|e| format!("SMTC new: {e:?}"))?;

    let (tx, rx) = std::sync::mpsc::channel::<MediaControlEvent>();
    controls
        .attach(move |e| {
            eprintln!("media key (SMTC STA): {e:?}");
            let _ = tx.send(e);
        })
        .map_err(|e| format!("SMTC attach: {e:?}"))?;

    let _ = controls.set_playback(MediaPlayback::Paused { progress: None });
    let _ = controls.set_metadata(MediaMetadata {
        title: Some("Tarn"),
        artist: Some(""),
        ..Default::default()
    });

    eprintln!("media controls: SMTC attached on STA thread with message pump");
    smtc_update_loop_pumped(app, controls, rx);
    Ok(())
}

#[cfg(windows)]
fn smtc_update_loop(
    app: Arc<crate::App>,
    mut controls: souvlaki::MediaControls,
    rx: std::sync::mpsc::Receiver<souvlaki::MediaControlEvent>,
) {
    use souvlaki::{MediaMetadata, MediaPlayback};
    let mut last_id: Option<String> = None;
    let mut last_playing: Option<bool> = None;
    let mut last_pos_bucket: u64 = u64::MAX;
    let mut last_meta: Option<(String, u64)> = None;
    loop {
        while let Ok(ev) = rx.try_recv() {
            dispatch(&app, ev);
        }
        let snap = app.player.snapshot();
        let playing = snap.playing;
        let id = snap.track.as_ref().map(|t| t.id.clone());
        let pos_bucket = (snap.position.max(0.0) * 2.0) as u64;
        let changed = id != last_id || Some(playing) != last_playing || pos_bucket != last_pos_bucket;
        if changed {
            if let Some(t) = &snap.track {
                let artists = t.artists.join(", ");
                let dur = snap.duration;
                let key = (t.id.clone(), dur.max(0.0).round() as u64);
                if last_meta.as_ref() != Some(&key) {
                    last_meta = Some(key);
                    if let Err(e) = controls.set_metadata(MediaMetadata {
                        title: Some(&t.title),
                        artist: Some(&artists),
                        album: t.album.as_deref(),
                        duration: if dur > 0.0 {
                            Some(Duration::from_secs_f64(dur))
                        } else {
                            None
                        },
                        cover_url: t.cover.as_deref(),
                    }) {
                        eprintln!("media controls: set_metadata: {e:?}");
                    }
                }
                if id != last_id {
                    maybe_notify(&app, t);
                }
            }
            let progress = Some(souvlaki::MediaPosition(Duration::from_secs_f64(
                snap.position.max(0.0),
            )));
            let status = if playing {
                MediaPlayback::Playing { progress }
            } else {
                MediaPlayback::Paused { progress }
            };
            if let Err(e) = controls.set_playback(status) {
                eprintln!("media controls: set_playback: {e:?}");
            }
            last_id = id;
            last_playing = Some(playing);
            last_pos_bucket = pos_bucket;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

#[cfg(windows)]
fn smtc_update_loop_pumped(
    app: Arc<crate::App>,
    mut controls: souvlaki::MediaControls,
    rx: std::sync::mpsc::Receiver<souvlaki::MediaControlEvent>,
) {
    // Same as smtc_update_loop, but drain the Win32 queue between ticks so
    // ButtonPressed arrives on this STA thread.
    use souvlaki::{MediaMetadata, MediaPlayback};
    let mut last_id: Option<String> = None;
    let mut last_playing: Option<bool> = None;
    let mut last_pos_bucket: u64 = u64::MAX;
    let mut last_meta: Option<(String, u64)> = None;
    loop {
        win_pump::pump();
        while let Ok(ev) = rx.try_recv() {
            dispatch(&app, ev);
        }
        let snap = app.player.snapshot();
        let playing = snap.playing;
        let id = snap.track.as_ref().map(|t| t.id.clone());
        let pos_bucket = (snap.position.max(0.0) * 2.0) as u64;
        let changed = id != last_id || Some(playing) != last_playing || pos_bucket != last_pos_bucket;
        if changed {
            if let Some(t) = &snap.track {
                let artists = t.artists.join(", ");
                let dur = snap.duration;
                let key = (t.id.clone(), dur.max(0.0).round() as u64);
                if last_meta.as_ref() != Some(&key) {
                    last_meta = Some(key);
                    let _ = controls.set_metadata(MediaMetadata {
                        title: Some(&t.title),
                        artist: Some(&artists),
                        album: t.album.as_deref(),
                        duration: if dur > 0.0 {
                            Some(Duration::from_secs_f64(dur))
                        } else {
                            None
                        },
                        cover_url: t.cover.as_deref(),
                    });
                }
                if id != last_id {
                    maybe_notify(&app, t);
                }
            }
            let progress = Some(souvlaki::MediaPosition(Duration::from_secs_f64(
                snap.position.max(0.0),
            )));
            let status = if playing {
                MediaPlayback::Playing { progress }
            } else {
                MediaPlayback::Paused { progress }
            };
            let _ = controls.set_playback(status);
            last_id = id;
            last_playing = Some(playing);
            last_pos_bucket = pos_bucket;
        }
        std::thread::sleep(Duration::from_millis(16));
    }
}

/// Minimal Win32 FFI so we can pump messages without a new `windows` crate
/// feature set (and the version conflict that would come with it).
#[cfg(windows)]
mod win_pump {
    use std::ffi::c_void;

    #[repr(C)]
    struct Point {
        x: i32,
        y: i32,
    }
    #[repr(C)]
    struct Msg {
        hwnd: *mut c_void,
        message: u32,
        wparam: usize,
        lparam: isize,
        time: u32,
        pt: Point,
    }

    const COINIT_APARTMENTTHREADED: u32 = 0x2;
    const PM_REMOVE: u32 = 0x1;

    #[link(name = "ole32")]
    extern "system" {
        fn CoInitializeEx(pv: *mut c_void, dw: u32) -> i32;
    }
    #[link(name = "user32")]
    extern "system" {
        fn PeekMessageW(msg: *mut Msg, hwnd: *mut c_void, min: u32, max: u32, remove: u32) -> i32;
        fn TranslateMessage(msg: *const Msg) -> i32;
        fn DispatchMessageW(msg: *const Msg) -> isize;
    }

    pub fn co_init_sta() {
        unsafe {
            let hr = CoInitializeEx(std::ptr::null_mut(), COINIT_APARTMENTTHREADED);
            // S_OK (0) or S_FALSE (1, already initialized) are fine.
            if hr < 0 {
                eprintln!("media controls: CoInitializeEx failed: {hr:#x}");
            }
        }
    }

    pub fn pump() {
        unsafe {
            let mut msg = std::mem::zeroed::<Msg>();
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_frontend_accelerators() {
        for a in [
            "Ctrl+Alt+KeyP",
            "Shift+ArrowRight",
            "Cmd+Shift+Digit1",
            "F9",
            "Ctrl+Space",
            "Alt+Comma",
            "Ctrl+Shift+MediaPlayPause",
        ] {
            assert!(matches!(parse_accel(a), Ok(Some(_))), "{a}");
        }
        assert!(matches!(parse_accel(""), Ok(None)));
        assert!(parse_accel("Ctrl+Nope").is_err());
        // Same combo, same id: lookup by id must line up with registration.
        let a = parse_accel("Ctrl+Alt+KeyP").unwrap().unwrap();
        let b = parse_accel("ctrl+alt+keyp").unwrap().unwrap();
        assert_eq!(a.id(), b.id());
    }
}
