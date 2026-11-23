//! Audio output device selection: open a named device (or the system
//! default), list devices for the settings picker, and (desktop) watch for
//! devices coming and going so playback follows headphones / Bluetooth.

use std::time::Duration;

use parking_lot::Mutex;
use rodio::cpal::traits::{DeviceTrait, HostTrait};
use rodio::{OutputStream, OutputStreamHandle};
use serde::Serialize;

/// User preference: device name (None = system default) and auto-switch.
static PREFS: Mutex<(Option<String>, bool)> = Mutex::new((None, false));
/// Device the audio thread has open right now (None = system default).
static CURRENT: Mutex<Option<String>> = Mutex::new(None);

#[derive(Debug, Clone, Serialize, Default)]
pub struct Devices {
    pub devices: Vec<String>,
    pub default: Option<String>,
    /// What is playing through now (None = system default).
    pub current: Option<String>,
}

/// Linux: ALSA prints "unable to open slave" / "no matching channel map" to
/// stderr for every PCM it can't open while cpal probes devices, every few
/// seconds while the picker or the device watcher runs. Harmless, but it buries
/// real logs, so swallow ALSA's own error printing (errors still come back to
/// cpal as return values).
#[cfg(target_os = "linux")]
fn quiet_alsa() {
    use std::os::raw::{c_char, c_int};
    static ONCE: std::sync::Once = std::sync::Once::new();
    // The real handler type is variadic; a callee that ignores the trailing
    // arguments is safe to call that way on the Linux C ABIs we ship.
    type Handler = unsafe extern "C" fn(*const c_char, c_int, *const c_char, c_int, *const c_char);
    unsafe extern "C" fn silent(_: *const c_char, _: c_int, _: *const c_char, _: c_int, _: *const c_char) {}
    // libasound is already linked through cpal (alsa-sys).
    extern "C" {
        fn snd_lib_error_set_handler(handler: Option<Handler>) -> c_int;
    }
    ONCE.call_once(|| unsafe {
        snd_lib_error_set_handler(Some(silent));
    });
}

#[cfg(not(target_os = "linux"))]
fn quiet_alsa() {}

pub fn list() -> Devices {
    quiet_alsa();
    let host = rodio::cpal::default_host();
    let mut devices: Vec<String> = host
        .output_devices()
        .map(|it| it.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default();
    devices.dedup();
    Devices {
        devices,
        default: host.default_output_device().and_then(|d| d.name().ok()),
        current: CURRENT.lock().clone(),
    }
}

/// Open `name`, or the system default when None / not found. Returns the
/// stream, its handle and the device actually opened (None = default).
pub fn open(name: Option<&str>) -> Option<(OutputStream, OutputStreamHandle, Option<String>)> {
    quiet_alsa();
    if let Some(want) = name {
        let host = rodio::cpal::default_host();
        let dev = host
            .output_devices()
            .ok()
            .and_then(|mut it| it.find(|d| d.name().ok().as_deref() == Some(want)));
        match dev.map(|d| OutputStream::try_from_device(&d)) {
            Some(Ok((s, h))) => {
                *CURRENT.lock() = Some(want.to_string());
                return Some((s, h, Some(want.to_string())));
            }
            Some(Err(e)) => eprintln!("audio output: {want}: {e}; using the default"),
            None => eprintln!("audio output: {want} not found; using the default"),
        }
    }
    match OutputStream::try_default() {
        Ok((s, h)) => {
            *CURRENT.lock() = None;
            Some((s, h, None))
        }
        Err(e) => {
            eprintln!("audio output: {e}");
            None
        }
    }
}

pub fn set_prefs(device: Option<String>, auto_switch: bool) {
    *PREFS.lock() = (device.filter(|d| !d.is_empty()), auto_switch);
}

pub fn prefs() -> (Option<String>, bool) {
    PREFS.lock().clone()
}

/// What the output should be given the devices present, or None to stay.
/// `last_default` is the system default seen on the previous check.
pub fn decide(
    prefs: &(Option<String>, bool),
    current: &Option<String>,
    now: &Devices,
    last_default: &Option<String>,
) -> Option<Option<String>> {
    let (want, auto) = prefs;
    let present = |n: &str| now.devices.iter().any(|d| d == n);
    // The device we play through vanished: fall back to the default.
    if let Some(cur) = current {
        if !present(cur) {
            return Some(None);
        }
    }
    match want {
        Some(w) if present(w) && current.as_deref() != Some(w) => {
            // Back on the chosen device when it reappears (auto), or right
            // after the user picked it (current is the fallback default).
            auto.then(|| Some(w.clone()))
        }
        // Prefs say system default but the stream is still on a named
        // device (e.g. user switched the picker back to System default).
        None if current.is_some() => Some(None),
        // "System default" means follow Windows/macOS/Linux default. cpal
        // pins the device at open time, so a new default needs a reopen —
        // even when auto-switch is off (that toggle is for a *named* pick).
        None if now.default != *last_default && last_default.is_some() => Some(None),
        _ => None,
    }
}

/// Desktop: poll devices every few seconds and ask the player to move.
#[cfg(desktop)]
pub fn watch(player: crate::player::Player) {
    std::thread::Builder::new()
        .name("tarn-output-watch".into())
        .spawn(move || {
            let mut last_default: Option<String> = None;
            loop {
                std::thread::sleep(Duration::from_secs(2));
                let prefs = prefs();
                // Named device, auto-switch off, already on that device: idle.
                // System default (want=None) always polls so a Windows default
                // change reopens the stream.
                if let (Some(want), false) = (&prefs.0, prefs.1) {
                    if CURRENT.lock().as_deref() == Some(want.as_str()) {
                        continue;
                    }
                }
                let now = list();
                // Prefer the live CURRENT over list()'s clone so a mid-loop
                // Output command is not overwritten by a stale snapshot.
                let current = CURRENT.lock().clone();
                if let Some(target) = decide(&prefs, &current, &now, &last_default) {
                    crate::diag::log(format!(
                        "audio output: switching to {} (was default {:?})",
                        target.as_deref().unwrap_or("system default"),
                        last_default
                    ));
                    player.set_output_device(target);
                }
                last_default = now.default;
            }
        })
        .ok();
}

#[cfg(mobile)]
pub fn watch(_player: crate::player::Player) {
    let _ = Duration::ZERO;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn devs(list: &[&str], default: &str) -> Devices {
        Devices {
            devices: list.iter().map(|s| s.to_string()).collect(),
            default: Some(default.into()),
            current: None,
        }
    }

    #[test]
    fn falls_back_when_the_device_disappears() {
        let now = devs(&["Speakers"], "Speakers");
        let got = decide(
            &(Some("Headphones".into()), false),
            &Some("Headphones".into()),
            &now,
            &None,
        );
        assert_eq!(got, Some(None));
    }

    #[test]
    fn returns_to_the_chosen_device_only_with_auto_switch() {
        let now = devs(&["Speakers", "Headphones"], "Speakers");
        let want = Some("Headphones".to_string());
        assert_eq!(
            decide(&(want.clone(), true), &None, &now, &None),
            Some(want.clone())
        );
        assert_eq!(decide(&(want.clone(), false), &None, &now, &None), None);
        // Already there: nothing to do.
        assert_eq!(decide(&(want.clone(), true), &want, &now, &None), None);
    }

    #[test]
    fn follows_a_new_system_default_even_without_auto_switch() {
        let now = devs(&["Speakers", "BT"], "BT");
        let last = Some("Speakers".to_string());
        // System default mode always reopens when the OS default changes.
        assert_eq!(decide(&(None, true), &None, &now, &last), Some(None));
        assert_eq!(decide(&(None, false), &None, &now, &last), Some(None));
        assert_eq!(decide(&(None, false), &None, &now, &Some("BT".into())), None);
        // Stuck on a named device while prefs say system default.
        assert_eq!(
            decide(&(None, false), &Some("Speakers".into()), &now, &Some("BT".into())),
            Some(None)
        );
        // A pinned named device does not chase the OS default.
        assert_eq!(
            decide(&(Some("Speakers".into()), false), &Some("Speakers".into()), &now, &last),
            None
        );
    }
}
