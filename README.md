# Tarn

A fast YouTube Music desktop client, built with Tauri 2, Rust and Svelte. It works with or without a Google account.

<p align="left"><img src="public/logo.png" width="72" alt="Tarn"></p>

![Home](docs/home.png)
![Playing](docs/playing.png)

Website: [rexsystems.github.io/tarn](https://rexsystems.github.io/tarn)

## Features

- Play YouTube Music without a Google account, or sign in to use your library, likes and playlists.
- Playback with crossfade, shuffle and repeat, a queue, and a seek bar with a time preview.
- Synced lyrics when they are available.
- Built-in equalizer.
- Offline downloads for playback without a connection.
- Media keys and desktop media controls on Linux (MPRIS).
- Discord Rich Presence.
- Restores your session on launch: the current track and position (paused, ready to play), the queue, volume, shuffle and repeat, the page you were on, and the window size and position.

## Install

Downloads are on the [releases](https://github.com/rexsystems/tarn/releases) page:

- Linux: AppImage, `.rpm` and `.deb`
- Windows: NSIS and MSI installers
- macOS: `.dmg`

Packaged builds check for signed updates on their own. You can also press "Check for updates" in Settings.

Tarn does not need ffmpeg, yt-dlp or any other extra tools installed. Audio is decoded inside the app, and a current yt-dlp is bundled and kept up to date automatically.

## Build from source

You need Rust 1.77 or newer, Node.js 20 or newer, and on Linux the Tauri system libraries:

```sh
# Fedora
sudo dnf install webkit2gtk4.1-devel gtk3-devel libappindicator-gtk3-devel librsvg2-devel patchelf openssl-devel alsa-lib-devel

# Debian / Ubuntu
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev patchelf libssl-dev libasound2-dev
```

Then:

```sh
npm install
npm run tauri build    # packages land in src-tauri/target/release/bundle/
npm run tauri dev      # development session
```

The build downloads an official [yt-dlp](https://github.com/yt-dlp/yt-dlp) sidecar automatically. End users of the packages don't need it.

## Project layout

- `src/`: Svelte frontend (UI, player controls, lists and settings).
- `src-tauri/src/`: Rust backend (playback, decoding, streams, YouTube Music API access, equalizer, downloads, integrations).
- `scripts/`: build helpers.
- `packaging/`: packaging assets, including macOS.
- `.github/workflows/`: CI, release and website deployment.

## Disclaimer

Tarn is an unofficial client. It is not affiliated with or endorsed by YouTube or Google.

## License

[GPL-3.0](LICENSE)
