//! Android media notification bridge.
//!
//! The Kotlin `PlaybackService` (foreground service + MediaSession) polls
//! `nativeState()` about once a second and sends button presses back through
//! `nativeAction()`. Keeping it pull-based means Rust never needs a JavaVM
//! handle or an Activity reference.

use std::sync::{Arc, OnceLock};

use jni::objects::{JClass, JString};
use jni::sys::jstring;
use jni::JNIEnv;

use crate::player::NextChoice;

static APP: OnceLock<Arc<crate::App>> = OnceLock::new();

pub fn start(app: Arc<crate::App>) {
    let _ = APP.set(app);
}

fn state_json() -> String {
    let Some(app) = APP.get() else {
        return "{}".into();
    };
    let snap = app.player.snapshot();
    match snap.track.as_ref() {
        Some(t) => serde_json::json!({
            "has": true,
            "id": t.id,
            "title": t.title,
            "artist": t.artists.join(", "),
            "album": t.album,
            "cover": t.cover,
            "playing": snap.playing,
            "position": snap.position.max(0.0),
            "duration": snap.duration.max(0.0),
        })
        .to_string(),
        None => serde_json::json!({ "has": false }).to_string(),
    }
}

fn act(action: &str) {
    let Some(app) = APP.get().cloned() else {
        return;
    };
    match action {
        "toggle" => app.player.toggle(),
        "play" => app.player.resume(),
        "pause" => app.player.pause(),
        "stop" => app.player.stop(),
        "next" => {
            tauri::async_runtime::spawn(async move {
                if let Err(e) = crate::advance(&app, true).await {
                    eprintln!("media next: {e}");
                }
            });
        }
        "prev" => {
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
        other => {
            if let Some(secs) = other.strip_prefix("seek:").and_then(|s| s.parse::<f64>().ok()) {
                app.player.seek(secs.max(0.0));
            }
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_app_tarn_desktop_MediaBridge_nativeState(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    env.new_string(state_json())
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

#[no_mangle]
pub extern "system" fn Java_app_tarn_desktop_MediaBridge_nativeAction(
    mut env: JNIEnv,
    _class: JClass,
    action: JString,
) {
    if let Ok(s) = env.get_string(&action) {
        let a: String = s.into();
        act(&a);
    }
}
