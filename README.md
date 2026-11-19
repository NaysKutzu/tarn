<div align="center">

<img src="public/logo.png" width="96" alt="Tarn logo">

# Tarn

**YouTube Music, on your desktop.**
A fast, self-contained player for Linux, Windows and macOS. Sign in with Google, or don't.

[![Latest release](https://img.shields.io/github/v/release/rexsystems/tarn?include_prereleases&label=release&color=e0a45a&labelColor=181715)](https://github.com/rexsystems/tarn/releases/latest)
[![License](https://img.shields.io/badge/license-GPL--3.0-e0a45a?labelColor=181715)](LICENSE)
[![Platforms](https://img.shields.io/badge/Linux%20%C2%B7%20Windows%20%C2%B7%20macOS-e0a45a?labelColor=181715)](https://github.com/rexsystems/tarn/releases/latest)
[![Built with](https://img.shields.io/badge/Tauri%202%20%C2%B7%20Rust%20%C2%B7%20Svelte-e0a45a?labelColor=181715)](#build-from-source)

[**Download**](https://github.com/rexsystems/tarn/releases/latest) · [Features](#features) · [Build from source](#build-from-source)

<br>

<img src="docs/home.png" width="860" alt="Tarn home screen">

</div>

<br>

## Why Tarn

- **Nothing else to install.** Audio is decoded inside the app, and a current yt-dlp is bundled and kept up to date. No ffmpeg, no extra tools.
- **Works with or without an account.** Play without signing in, or sign in to get your library, likes and playlists.
- **Picks up where you left off.** The track and position, queue, volume, shuffle and repeat, the page you were on, and the window itself all come back on launch.
- **Light and quick.** Tauri, Rust and Svelte instead of a bundled browser.

<div align="center">
<img src="docs/playing.png" width="860" alt="Tarn playing a track">
</div>

## Features

**Playback**
- Crossfade, shuffle, repeat and a queue you can reorder.
- Seek bar with a time preview.
- Built-in equalizer.
- Sleep timer.

**Fullscreen player**
- Big cover art over a blurred backdrop that follows the track.
- Synced lyrics when they are available, with the queue ("Up next") beside them.

**Your library**
- Liked songs, playlists, albums and artists once you sign in.
- Playlists and liked songs open instantly, even the big ones.
- Offline downloads, so playback keeps working without a connection.

**Everywhere on your desktop**
- Tray mini player.
- Configurable keyboard shortcuts, plus global hotkeys that work when Tarn is in the background.
- Media keys and desktop media controls on Linux (MPRIS).
- Discord Rich Presence and scrobbling.
- Listening stats.

**Listen along**
- Play the same music in sync with friends, on your Wi-Fi or over the internet. Host your own tiny relay with [one command](#self-hosted-listen-along-server) so nothing depends on anyone else's server.

**Updates**
- Packaged builds check for signed updates on their own. You can also press "Check for updates" in Settings.

## Install

Grab the latest build from the [releases page](https://github.com/rexsystems/tarn/releases/latest):

| Platform | Files |
| --- | --- |
| **Windows** | NSIS installer (`.exe`) or MSI |
| **macOS** | `.dmg` for Apple silicon or Intel |
| **Linux** | AppImage, `.deb` or `.rpm`, for x86_64 and ARM64 |
| **Android** | APK, when the release includes one (early build) |
| **iOS** | Unsigned IPA for sideloading, when the release includes one |

> **macOS:** the app is not notarized yet. The first time, right-click it and choose **Open**.

## Self-hosted Listen Along server

Listen Along works on the same Wi-Fi out of the box. To listen together over the internet, run the small relay in [`server/`](server/) on any Linux box with a domain. One command, direct on a port as a hardened systemd service, with automatic HTTPS through Caddy:

```sh
git clone --depth 1 https://github.com/rexsystems/tarn && cd tarn/server && sudo ./install.sh jam.example.com
```

Prefer Docker?

```sh
git clone --depth 1 https://github.com/rexsystems/tarn && cd tarn/server && DOMAIN=jam.example.com TARN_JAM_PASSWORD='pick-a-long-password' docker compose up -d --build
```

Then in Tarn open **Settings → Listen together**, enter `https://jam.example.com` and the password, press **Host** and share the join code. Details, configuration and the security model are in [server/README.md](server/README.md).

## Build from source

You need Rust 1.77 or newer, Node.js 20 or newer, [pnpm](https://pnpm.io) 12, and on Linux the Tauri system libraries:

```sh
# Fedora
sudo dnf install webkit2gtk4.1-devel gtk3-devel libappindicator-gtk3-devel librsvg2-devel patchelf openssl-devel alsa-lib-devel

# Debian / Ubuntu
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev patchelf libssl-dev libasound2-dev
```

Then:

```sh
pnpm install
pnpm run tauri dev      # development session
pnpm run tauri build    # packages land in src-tauri/target/release/bundle/
```

The build downloads an official [yt-dlp](https://github.com/yt-dlp/yt-dlp) sidecar automatically. End users of the packages don't need it.

## Project layout

| Path | What's in it |
| --- | --- |
| `src/` | Svelte frontend: UI, player controls, lists and settings. |
| `src-tauri/src/` | Rust backend: playback, decoding, streams, YouTube Music API access, equalizer, downloads, integrations. |
| `server/` | Self-hosted Listen Along relay (Rust + Docker). |
| `site/` | The static website, including the download page. |
| `scripts/` | Build helpers. |
| `packaging/` | Packaging assets, including macOS. |
| `.github/workflows/` | CI, release and mobile builds. |

## Contributing

Issues and pull requests are welcome. For a change to the player or the backend, run `pnpm run tauri dev` and try it in the real app before opening a PR.

## Disclaimer

Tarn is an unofficial client. It is not affiliated with or endorsed by YouTube or Google.

## License

[GPL-3.0](LICENSE)
