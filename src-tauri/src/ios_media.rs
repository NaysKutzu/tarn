//! iOS: audio session (so music keeps playing in the background and with the
//! screen locked), the lock-screen / Control Center "Now Playing" card, and
//! the remote commands behind its buttons and the headphone controls.
//!
//! Kept free of any Tarn types on purpose: `lib.rs` feeds it plain data and
//! gets plain commands back.

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_avf_audio::{AVAudioSession, AVAudioSessionCategoryPlayback};
use objc2_foundation::{NSCopying, NSMutableDictionary, NSNumber, NSString};
use objc2_media_player::{
    MPChangePlaybackPositionCommandEvent, MPMediaItemPropertyAlbumTitle,
    MPMediaItemPropertyArtist, MPMediaItemPropertyPlaybackDuration, MPMediaItemPropertyTitle,
    MPNowPlayingInfoCenter, MPNowPlayingInfoPropertyElapsedPlaybackTime,
    MPNowPlayingInfoPropertyPlaybackRate, MPNowPlayingPlaybackState, MPRemoteCommand,
    MPRemoteCommandCenter, MPRemoteCommandEvent, MPRemoteCommandHandlerStatus,
};
use std::ptr::NonNull;
use std::sync::Arc;

/// What the lock screen should show.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Info {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: f64,
    pub position: f64,
    pub playing: bool,
}

/// What a lock-screen / headphone button asked for.
#[derive(Clone, Copy, Debug)]
pub enum Cmd {
    Play,
    Pause,
    Toggle,
    Next,
    Prev,
    Seek(f64),
}

/// Playback category: audio continues when the app is backgrounded or the
/// device is locked (needs `UIBackgroundModes: audio` in Info.plist) and
/// ignores the silent switch.
pub fn setup_audio_session() -> Result<(), String> {
    unsafe {
        let session = AVAudioSession::sharedInstance();
        let category = AVAudioSessionCategoryPlayback.ok_or("AVAudioSessionCategoryPlayback missing")?;
        session
            .setCategory_error(category)
            .map_err(|e| format!("setCategory: {}", e.localizedDescription()))?;
        session
            .setActive_error(true)
            .map_err(|e| format!("setActive: {}", e.localizedDescription()))?;
    }
    Ok(())
}

/// Re-activate the session (after a phone call or another app took audio).
pub fn reactivate_audio_session() {
    unsafe {
        let _ = AVAudioSession::sharedInstance().setActive_error(true);
    }
}

fn ok_cmd(cmd: &MPRemoteCommand, f: Arc<dyn Fn() + Send + Sync>) {
    unsafe {
        cmd.setEnabled(true);
        let block = RcBlock::new(move |_e: NonNull<MPRemoteCommandEvent>| {
            f();
            MPRemoteCommandHandlerStatus::Success
        });
        // The command center keeps the handler for the life of the app.
        let _target = cmd.addTargetWithHandler(&block);
    }
}

/// Hook the lock-screen buttons up to `handler`. Call once, on the main thread.
pub fn register_commands(handler: Arc<dyn Fn(Cmd) + Send + Sync>) {
    unsafe {
        let center = MPRemoteCommandCenter::sharedCommandCenter();
        let h = handler.clone();
        ok_cmd(&center.playCommand(), Arc::new(move || h(Cmd::Play)));
        let h = handler.clone();
        ok_cmd(&center.pauseCommand(), Arc::new(move || h(Cmd::Pause)));
        let h = handler.clone();
        ok_cmd(&center.togglePlayPauseCommand(), Arc::new(move || h(Cmd::Toggle)));
        let h = handler.clone();
        ok_cmd(&center.nextTrackCommand(), Arc::new(move || h(Cmd::Next)));
        let h = handler.clone();
        ok_cmd(&center.previousTrackCommand(), Arc::new(move || h(Cmd::Prev)));

        // Scrubbing the progress bar on the lock screen.
        let pos = center.changePlaybackPositionCommand();
        pos.setEnabled(true);
        let h = handler.clone();
        let block = RcBlock::new(move |e: NonNull<MPRemoteCommandEvent>| {
            let ev = e.cast::<MPChangePlaybackPositionCommandEvent>();
            h(Cmd::Seek(ev.as_ref().positionTime()));
            MPRemoteCommandHandlerStatus::Success
        });
        let _target = pos.addTargetWithHandler(&block);

        // We don't do fixed-interval skips; keep those buttons off.
        center.skipForwardCommand().setEnabled(false);
        center.skipBackwardCommand().setEnabled(false);
    }
}

fn put(d: &NSMutableDictionary<NSString, AnyObject>, key: &NSString, val: &AnyObject) {
    unsafe { d.setObject_forKey(val, ProtocolObject::<dyn NSCopying>::from_ref(key)) };
}

/// Push the current track to the lock screen. `None` clears the card.
pub fn publish(info: Option<&Info>) {
    unsafe {
        let center = MPNowPlayingInfoCenter::defaultCenter();
        let Some(i) = info else {
            center.setNowPlayingInfo(None);
            center.setPlaybackState(MPNowPlayingPlaybackState::Stopped);
            return;
        };
        let d: Retained<NSMutableDictionary<NSString, AnyObject>> = NSMutableDictionary::new();
        let title = NSString::from_str(&i.title);
        let artist = NSString::from_str(&i.artist);
        let album = NSString::from_str(&i.album);
        let dur = NSNumber::new_f64(i.duration.max(0.0));
        let pos = NSNumber::new_f64(i.position.max(0.0));
        let rate = NSNumber::new_f64(if i.playing { 1.0 } else { 0.0 });
        put(&d, MPMediaItemPropertyTitle, &title);
        put(&d, MPMediaItemPropertyArtist, &artist);
        if !i.album.is_empty() {
            put(&d, MPMediaItemPropertyAlbumTitle, &album);
        }
        if i.duration > 0.0 {
            put(&d, MPMediaItemPropertyPlaybackDuration, &dur);
        }
        put(&d, MPNowPlayingInfoPropertyElapsedPlaybackTime, &pos);
        put(&d, MPNowPlayingInfoPropertyPlaybackRate, &rate);
        center.setNowPlayingInfo(Some(&d));
        center.setPlaybackState(if i.playing {
            MPNowPlayingPlaybackState::Playing
        } else {
            MPNowPlayingPlaybackState::Paused
        });
    }
}
