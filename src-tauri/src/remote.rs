//! Phone remote (desktop only, off by default): a tiny LAN-only HTTP server
//! that serves one control page. The desktop shows a QR code with
//! `http://<lan-ip>:<port>/#<token>`; every API call must carry that random
//! token in the `X-Tarn-Token` header. The token is regenerated each time the
//! remote is turned on and lives only in memory. Connections from public
//! (non-LAN) addresses are refused.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::models::{Lyrics, Track};

const PAGE: &str = include_str!("remote/index.html");
/// Tried first so the QR stays the same between runs; falls back to any port.
const DEFAULT_PORT: u16 = 47_801;
const MAX_HEADER: usize = 8 * 1024;
const MAX_BODY: usize = 4 * 1024;
const MAX_CONNS: usize = 16;

#[derive(Clone, Debug, Default, Serialize)]
pub struct RemoteInfo {
    pub running: bool,
    pub url: Option<String>,
    /// QR code of `url` as an SVG document.
    pub qr_svg: Option<String>,
    pub error: Option<String>,
}

struct Running {
    url: String,
    qr_svg: String,
    stop: tokio::sync::watch::Sender<bool>,
}

static STATE: Mutex<Option<Running>> = Mutex::new(None);

pub fn info() -> RemoteInfo {
    match &*STATE.lock() {
        Some(r) => RemoteInfo { running: true, url: Some(r.url.clone()), qr_svg: Some(r.qr_svg.clone()), error: None },
        None => RemoteInfo::default(),
    }
}

pub fn stop() {
    if let Some(r) = STATE.lock().take() {
        let _ = r.stop.send(true);
    }
}

pub async fn start(app: Arc<crate::App>, handle: AppHandle) -> RemoteInfo {
    stop();
    let listener = match TcpListener::bind(("0.0.0.0", DEFAULT_PORT)).await {
        Ok(l) => l,
        Err(_) => match TcpListener::bind(("0.0.0.0", 0)).await {
            Ok(l) => l,
            Err(e) => return RemoteInfo { error: Some(format!("Couldn't open a port: {e}")), ..Default::default() },
        },
    };
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(DEFAULT_PORT);
    let Some(ip) = lan_ip() else {
        return RemoteInfo { error: Some("No local network found".into()), ..Default::default() };
    };
    let token = new_token();
    let url = format!("http://{}/#{token}", SocketAddr::new(ip, port));
    let qr_svg = match qr(&url) {
        Some(s) => s,
        None => return RemoteInfo { error: Some("Couldn't make the QR code".into()), ..Default::default() },
    };
    let (tx, rx) = tokio::sync::watch::channel(false);
    *STATE.lock() = Some(Running { url: url.clone(), qr_svg: qr_svg.clone(), stop: tx });
    crate::diag::log(format!("remote: listening on port {port}"));
    let srv = Arc::new(Server { app, handle, token, lyrics: Mutex::new(None), conns: Default::default() });
    tauri::async_runtime::spawn(serve(listener, srv, rx));
    RemoteInfo { running: true, url: Some(url), qr_svg: Some(qr_svg), error: None }
}

fn new_token() -> String {
    let mut b = [0u8; 24];
    rand::rngs::OsRng.fill_bytes(&mut b);
    hex::encode(b)
}

/// The address other devices on the LAN reach us at. Connecting a UDP
/// socket sends nothing; it only asks the OS which interface it would use.
fn lan_ip() -> Option<IpAddr> {
    let s = std::net::UdpSocket::bind(("0.0.0.0", 0)).ok()?;
    s.connect(("192.0.2.1", 9)).ok()?; // TEST-NET-1, never routed
    let ip = s.local_addr().ok()?.ip();
    (!ip.is_unspecified() && !ip.is_loopback()).then_some(ip)
}

fn qr(text: &str) -> Option<String> {
    use qrcode::render::svg;
    let code = qrcode::QrCode::new(text.as_bytes()).ok()?;
    Some(
        code.render::<svg::Color>()
            .min_dimensions(220, 220)
            .quiet_zone(true)
            .dark_color(svg::Color("#000000"))
            .light_color(svg::Color("#ffffff"))
            .build(),
    )
}

/// Only private, link-local and loopback peers may talk to the remote.
pub(crate) fn is_local_peer(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => v.is_private() || v.is_loopback() || v.is_link_local(),
        IpAddr::V6(v) => {
            if let Some(v4) = v.to_ipv4_mapped() {
                return is_local_peer(IpAddr::V4(v4));
            }
            let s = v.segments()[0];
            v.is_loopback() || (s & 0xfe00) == 0xfc00 || (s & 0xffc0) == 0xfe80
        }
    }
}

/// Constant-time token check.
fn token_ok(given: &str, want: &str) -> bool {
    let (a, b) = (given.as_bytes(), want.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

struct Server {
    app: Arc<crate::App>,
    handle: AppHandle,
    token: String,
    lyrics: Mutex<Option<(String, Lyrics)>>,
    conns: std::sync::atomic::AtomicUsize,
}

async fn serve(listener: TcpListener, srv: Arc<Server>, mut stop: tokio::sync::watch::Receiver<bool>) {
    use std::sync::atomic::Ordering;
    loop {
        tokio::select! {
            _ = stop.changed() => break,
            acc = listener.accept() => {
                let Ok((sock, peer)) = acc else { continue };
                if !is_local_peer(peer.ip()) || srv.conns.load(Ordering::Relaxed) >= MAX_CONNS {
                    drop(sock);
                    continue;
                }
                let srv = srv.clone();
                srv.conns.fetch_add(1, Ordering::Relaxed);
                tauri::async_runtime::spawn(async move {
                    let _ = tokio::time::timeout(Duration::from_secs(10), handle_conn(&srv, sock)).await;
                    srv.conns.fetch_sub(1, Ordering::Relaxed);
                });
            }
        }
    }
    crate::diag::log("remote: stopped");
}

struct Request {
    method: String,
    path: String,
    token: Option<String>,
    body: Vec<u8>,
}

async fn read_request(sock: &mut TcpStream) -> Option<Request> {
    let mut buf = Vec::with_capacity(1024);
    let mut tmp = [0u8; 1024];
    let head_end = loop {
        let n = sock.read(&mut tmp).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break p;
        }
        if buf.len() > MAX_HEADER {
            return None;
        }
    };
    let head = std::str::from_utf8(&buf[..head_end]).ok()?;
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let method = first.next()?.to_string();
    let path = first.next()?.to_string();
    let mut len = 0usize;
    let mut token = None;
    for l in lines {
        let Some((k, v)) = l.split_once(':') else { continue };
        let v = v.trim();
        match k.trim().to_ascii_lowercase().as_str() {
            "content-length" => len = v.parse().ok()?,
            "x-tarn-token" => token = Some(v.to_string()),
            _ => {}
        }
    }
    if len > MAX_BODY {
        return None;
    }
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < len {
        let n = sock.read(&mut tmp).await.ok()?;
        if n == 0 {
            return None;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    body.truncate(len);
    Some(Request { method, path, token, body })
}

async fn respond(sock: &mut TcpStream, status: &str, ctype: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\n\
         X-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nX-Frame-Options: DENY\r\n\
         Content-Security-Policy: default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src https: data:; connect-src 'self'\r\n\
         Connection: close\r\n\r\n",
        body.len()
    );
    let _ = sock.write_all(head.as_bytes()).await;
    let _ = sock.write_all(body).await;
    let _ = sock.flush().await;
}

async fn json<T: Serialize>(sock: &mut TcpStream, v: &T) {
    let b = serde_json::to_vec(v).unwrap_or_default();
    respond(sock, "200 OK", "application/json", &b).await;
}

#[derive(Serialize)]
struct RemoteState<'a> {
    track: Option<&'a Track>,
    playing: bool,
    position: f64,
    duration: f64,
    volume: f32,
    liked: bool,
    shuffle: bool,
    repeat: &'a str,
    index: usize,
    /// Up to 50 songs from the current one on.
    queue: &'a [Track],
    queue_start: usize,
}

#[derive(Deserialize)]
struct Cmd {
    cmd: String,
    #[serde(default)]
    value: Option<f64>,
}

async fn handle_conn(srv: &Server, mut sock: TcpStream) {
    let Some(req) = read_request(&mut sock).await else { return };
    let path = req.path.split('?').next().unwrap_or("/");
    if req.method == "GET" && (path == "/" || path == "/index.html") {
        respond(&mut sock, "200 OK", "text/html; charset=utf-8", PAGE.as_bytes()).await;
        return;
    }
    if !path.starts_with("/api/") {
        respond(&mut sock, "404 Not Found", "text/plain", b"not found").await;
        return;
    }
    if !req.token.as_deref().is_some_and(|t| token_ok(t, &srv.token)) {
        respond(&mut sock, "401 Unauthorized", "text/plain", b"bad token").await;
        return;
    }
    let app = &srv.app;
    match (req.method.as_str(), path) {
        ("GET", "/api/state") => {
            let s = app.player.snapshot();
            let (queue, index) = app.player.queue_and_index();
            let start = index.min(queue.len());
            let end = (start + 50).min(queue.len());
            json(&mut sock, &RemoteState {
                track: s.track.as_ref(),
                playing: s.playing,
                position: s.position,
                duration: s.duration,
                volume: s.volume,
                liked: s.liked,
                shuffle: s.shuffle,
                repeat: &s.repeat,
                index,
                queue: &queue[start..end],
                queue_start: start,
            })
            .await;
        }
        ("GET", "/api/lyrics") => {
            let id = app.player.snapshot().track.map(|t| t.id).unwrap_or_default();
            if id.is_empty() {
                json(&mut sock, &Lyrics::default()).await;
                return;
            }
            let cached = srv.lyrics.lock().as_ref().filter(|(i, _)| *i == id).map(|(_, l)| l.clone());
            let l = match cached {
                Some(l) => l,
                None => {
                    let l = app.ytm.lyrics(&id).await.unwrap_or_default();
                    *srv.lyrics.lock() = Some((id, l.clone()));
                    l
                }
            };
            json(&mut sock, &l).await;
        }
        ("POST", "/api/cmd") => {
            let Ok(c) = serde_json::from_slice::<Cmd>(&req.body) else {
                respond(&mut sock, "400 Bad Request", "text/plain", b"bad command").await;
                return;
            };
            run_cmd(srv, c).await;
            respond(&mut sock, "204 No Content", "text/plain", b"").await;
        }
        _ => respond(&mut sock, "404 Not Found", "text/plain", b"not found").await,
    }
}

async fn run_cmd(srv: &Server, c: Cmd) {
    use souvlaki::MediaControlEvent as E;
    let app = &srv.app;
    match c.cmd.as_str() {
        "toggle" => app.player.toggle(),
        "next" => crate::mpris::dispatch(app, E::Next),
        "prev" => crate::mpris::dispatch(app, E::Previous),
        "seek" => {
            if let Some(v) = c.value.filter(|v| v.is_finite() && *v >= 0.0) {
                app.player.seek(v);
            }
        }
        "volume" => {
            if let Some(v) = c.value.filter(|v| v.is_finite()) {
                let v = v.clamp(0.0, 1.0) as f32;
                // The window's slider follows the player state.
                app.player.set_volume(v);
            }
        }
        // The page owns like state (optimistic UI, local likes), as with the tray.
        "like" => {
            let _ = srv.handle.emit("tray", "like");
        }
        "play_index" => {
            let Some(i) = c.value.filter(|v| v.is_finite() && *v >= 0.0).map(|v| v as usize) else { return };
            let (queue, _) = app.player.queue_and_index();
            if let Some(t) = queue.get(i).cloned() {
                if let Err(e) = crate::start_playback(app, t, queue, i).await {
                    crate::diag::log(format!("remote: play_index failed: {e}"));
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_lan_peers() {
        for ok in ["192.168.1.20", "10.0.0.5", "172.16.3.4", "127.0.0.1", "169.254.1.1", "::1", "fe80::1", "fd00::2", "::ffff:192.168.0.9"] {
            assert!(is_local_peer(ok.parse().unwrap()), "{ok}");
        }
        for bad in ["8.8.8.8", "1.1.1.1", "2001:4860::8888", "::ffff:8.8.8.8", "172.32.0.1"] {
            assert!(!is_local_peer(bad.parse().unwrap()), "{bad}");
        }
    }

    #[test]
    fn token_compare() {
        let t = new_token();
        assert_eq!(t.len(), 48);
        assert!(token_ok(&t, &t));
        assert!(!token_ok("", &t));
        assert!(!token_ok(&t[..47], &t));
        assert_ne!(new_token(), t);
    }

    #[tokio::test]
    async fn parses_request_with_token_and_body() {
        let l = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = l.local_addr().unwrap();
        let client = tokio::spawn(async move {
            let mut c = TcpStream::connect(addr).await.unwrap();
            let body = br#"{"cmd":"seek","value":12.5}"#;
            let head = format!(
                "POST /api/cmd HTTP/1.1\r\nHost: x\r\nX-Tarn-Token: abc\r\nContent-Length: {}\r\n\r\n",
                body.len()
            );
            c.write_all(head.as_bytes()).await.unwrap();
            c.write_all(body).await.unwrap();
            c
        });
        let (mut s, _) = l.accept().await.unwrap();
        let r = read_request(&mut s).await.unwrap();
        let _c = client.await.unwrap();
        assert_eq!((r.method.as_str(), r.path.as_str(), r.token.as_deref()), ("POST", "/api/cmd", Some("abc")));
        let c: Cmd = serde_json::from_slice(&r.body).unwrap();
        assert_eq!((c.cmd.as_str(), c.value), ("seek", Some(12.5)));
    }

    #[test]
    fn page_has_no_external_assets() {
        assert!(!PAGE.contains("src=\"http"));
        assert!(PAGE.contains("X-Tarn-Token"));
    }

    #[test]
    fn qr_renders() {
        let s = qr("http://192.168.1.2:47801/#abc").unwrap();
        assert!(s.contains("<svg"));
    }
}
