//! Installs desktop updates on RPM-based distros (Fedora, openSUSE, ...).
//!
//! tauri-plugin-updater installs `.rpm` updates with a bare `rpm -U`. Since
//! Fedora 45, rpm enforces package signatures by default
//! (`%_pkgverify_level all`), so our GPG-unsigned rpm is rejected right after
//! the user typed their password. The plugin then falls back to a second
//! kdialog/zenity password prompt and finally terminal sudo, which all fail the
//! same way, so it looks like the password was wrong.
//!
//! We still let the plugin download the update, which verifies it against the
//! minisign public key pinned in `tauri.conf.json`, and then install that exact
//! verified file ourselves with `rpm -U --nosignature` behind a single pkexec
//! prompt. No fallbacks: if the user cancels, we stop.

use serde::Serialize;
use tauri::{ipc::Channel, AppHandle};

/// Same shape as the updater plugin's JS `DownloadEvent`, so the frontend can
/// reuse one progress handler for both install paths.
#[derive(Clone, Serialize)]
#[serde(tag = "event", content = "data")]
pub enum UpdateEvent {
    #[serde(rename_all = "camelCase")]
    Started { content_length: Option<u64> },
    #[serde(rename_all = "camelCase")]
    Progress { chunk_length: usize },
    Finished,
}

/// True when this build was installed from the `.rpm` package.
#[tauri::command]
pub fn rpm_update_supported() -> bool {
    imp::supported()
}

/// Download (signature-verified) and install the pending update via pkexec.
#[tauri::command]
pub async fn install_rpm_update(
    app: AppHandle,
    on_event: Channel<UpdateEvent>,
) -> Result<(), String> {
    imp::install(app, on_event).await
}

#[cfg(target_os = "linux")]
mod imp {
    use super::UpdateEvent;
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};
    use tauri::{
        ipc::Channel,
        utils::{config::BundleType, platform::bundle_type},
        AppHandle,
    };
    use tauri_plugin_updater::UpdaterExt;

    const RPM_MAGIC: [u8; 4] = [0xED, 0xAB, 0xEE, 0xDB];

    pub fn supported() -> bool {
        matches!(bundle_type(), Some(BundleType::Rpm))
    }

    pub async fn install(app: AppHandle, on_event: Channel<UpdateEvent>) -> Result<(), String> {
        if !supported() {
            return Err("This copy of Tarn was not installed from an .rpm".into());
        }
        let update = app
            .updater()
            .map_err(|e| e.to_string())?
            .check()
            .await
            .map_err(|e| e.to_string())?
            .ok_or("No update available")?;

        // `download` verifies the minisign signature before returning the bytes.
        let mut started = false;
        let progress = on_event.clone();
        let bytes = update
            .download(
                move |chunk_length, content_length| {
                    if !started {
                        started = true;
                        let _ = progress.send(UpdateEvent::Started { content_length });
                    }
                    let _ = progress.send(UpdateEvent::Progress { chunk_length });
                },
                || {
                    let _ = on_event.send(UpdateEvent::Finished);
                },
            )
            .await
            .map_err(|e| e.to_string())?;

        if !bytes.starts_with(&RPM_MAGIC) {
            return Err("The downloaded update is not an .rpm package".into());
        }

        tauri::async_runtime::spawn_blocking(move || install_verified(&bytes))
            .await
            .map_err(|e| e.to_string())?
    }

    /// Writes the verified package into a fresh 0700 directory (only this user
    /// and root can touch it between verification and install) and installs it.
    fn install_verified(bytes: &[u8]) -> Result<(), String> {
        let dir = private_temp_dir()?;
        let result = (|| {
            let pkg = dir.join("tarn-update.rpm");
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&pkg)
                .map_err(|e| format!("Couldn't save the update: {e}"))?;
            f.write_all(bytes)
                .and_then(|_| f.sync_all())
                .map_err(|e| format!("Couldn't save the update: {e}"))?;
            drop(f);
            run_pkexec_rpm(&pkg)
        })();
        let _ = std::fs::remove_dir_all(&dir);
        result
    }

    fn private_temp_dir() -> Result<PathBuf, String> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "tarn-rpm-update-{}-{nanos}",
            std::process::id()
        ));
        // create (not create_all): fails if something already sits at this path.
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&dir)
            .map_err(|e| format!("Couldn't create a temporary folder: {e}"))?;
        Ok(dir)
    }

    fn run_pkexec_rpm(pkg: &Path) -> Result<(), String> {
        let status = std::process::Command::new("pkexec")
            .args(["rpm", "-U", "--nosignature"])
            .arg(pkg)
            .status()
            .map_err(|e| format!("Couldn't start pkexec: {e}"))?;
        match status.code() {
            Some(0) => Ok(()),
            // pkexec: 126 = dialog dismissed, 127 = not authorized / wrong password
            Some(126) => Err("cancelled".into()),
            Some(127) => Err("not-authorized".into()),
            Some(code) => Err(format!("rpm exited with code {code}")),
            None => Err("rpm was interrupted".into()),
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use super::UpdateEvent;
    use tauri::{ipc::Channel, AppHandle};

    pub fn supported() -> bool {
        false
    }

    pub async fn install(_app: AppHandle, _on_event: Channel<UpdateEvent>) -> Result<(), String> {
        Err("RPM updates are only supported on Linux".into())
    }
}
