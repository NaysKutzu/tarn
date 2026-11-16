//! iOS glue: keeps the lock-screen Now Playing card in sync with the player
//! and routes its buttons back. The Apple-specific part is in `ios_media`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::AppHandle;

use crate::ios_media::{self, Cmd, Info};
use crate::player::NextChoice;

fn act(app: &Arc<crate::App>, cmd: Cmd) {
    crate::diag::log(format!("ios media cmd: {cmd:?}"));
    match cmd {
        Cmd::Play => app.player.resume(),
        Cmd::Pause => app.player.pause(),
        Cmd::Toggle => app.player.toggle(),
        Cmd::Seek(secs) => app.player.seek(secs.max(0.0)),
        Cmd::Next => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = crate::advance(&app, true).await {
                    eprintln!("media next: {e}");
                }
            });
        }
        Cmd::Prev => {
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
    }
}

pub fn start(app: Arc<crate::App>, handle: AppHandle) {
    // Lock-screen buttons -> player. Registered on the main thread.
    let a = app.clone();
    let _ = handle.run_on_main_thread(move || {
        ios_media::register_commands(Arc::new(move |c| act(&a, c)));
        crate::diag::log("ios media: remote commands registered");
    });

    std::thread::Builder::new()
        .name("tarn-nowplaying".into())
        .spawn(move || {
            // The OS extrapolates the elapsed time from rate + the last value
            // we gave it, so only re-publish on a real change or a seek.
            let mut last: Option<(String, bool)> = None;
            let mut sent_pos = 0.0_f64;
            let mut sent_at = Instant::now();
            loop {
                let snap = app.player.snapshot();
                let key = snap.track.as_ref().map(|t| (t.id.clone(), snap.playing));
                let expected = if snap.playing {
                    sent_pos + sent_at.elapsed().as_secs_f64()
                } else {
                    sent_pos
                };
                let jumped = (snap.position - expected).abs() > 2.0;
                if key != last || jumped {
                    let info = snap.track.as_ref().map(|t| Info {
                        title: t.title.clone(),
                        artist: t.artists.join(", "),
                        album: t.album.clone().unwrap_or_default(),
                        duration: snap.duration.max(0.0),
                        position: snap.position.max(0.0),
                        playing: snap.playing,
                    });
                    let _ = handle.run_on_main_thread(move || ios_media::publish(info.as_ref()));
                    last = key;
                    sent_pos = snap.position.max(0.0);
                    sent_at = Instant::now();
                }
                std::thread::sleep(Duration::from_millis(400));
            }
        })
        .ok();
}
