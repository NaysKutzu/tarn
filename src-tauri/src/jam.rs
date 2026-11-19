//! Listen together ("jam"), desktop only and off by default. A minimal
//! host-as-server design for one local network:
//!
//! * The **host** runs a small HTTP endpoint and shares a join code
//!   (`<lan-ip>:<port>#<token>`). It keeps full control of playback.
//! * **Guests** poll the host once a second and follow its track, position
//!   and play/pause state (seeking when they drift more than ~1.5 s). They
//!   can add songs to the host's queue.
//!
//! The token is random per session and only kept in memory. In LAN mode only
//! private, link-local and loopback peers are accepted.
//!
//! For sessions over the internet the host can instead push its state to a
//! self-hosted relay (see `server/` in the repo). The join code then looks like
//! `https://jam.example.com/r/<room>#<token>` and guests poll the relay, so
//! nobody needs an open port at home.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::models::Track;

const DEFAULT_PORT: u16 = 47_811;
const MAX_HEADER: usize = 8 * 1024;
const MAX_BODY: usize = 16 * 1024;
const MAX_CONNS: usize = 32;
/// Guests further off than this get seeked back in line.
const DRIFT_S: f64 = 1.5;
const POLL: Duration = Duration::from_millis(1000);

#[derive(Clone, Debug, Default, Serialize)]
pub struct JamInfo {
    /// "off" | "hosting" | "joined"
    pub mode: String,
    /// Join code to share while hosting.
    pub code: Option<String>,
    /// Guests seen in the last 10 s (host).
    pub guests: usize,
    /// Host's current track title (guest).
    pub following: Option<String>,
    /// Round trip to the host in ms (guest).
    pub rtt_ms: Option<u32>,
    pub error: Option<String>,
}

/// What the host publishes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JamState {
    pub track: Option<Track>,
    pub playing: bool,
    pub position: f64,
    pub loading: bool,
    /// Up to 20 songs after the current one.
    pub upcoming: Vec<Track>,
}

enum Session {
    Host {
        code: String,
        stop: tokio::sync::watch::Sender<bool>,
        guests: Arc<Mutex<HashMap<IpAddr, Instant>>>,
        relay: Arc<RelayStatus>,
    },
    Guest { stop: tokio::sync::watch::Sender<bool>, status: Arc<Mutex<GuestStatus>> },
}

/// What the relay pusher reports back to the host's UI.
#[derive(Default)]
struct RelayStatus {
    guests: std::sync::atomic::AtomicUsize,
    error: Mutex<Option<String>>,
}

#[derive(Default, Clone)]
struct GuestStatus {
    following: Option<String>,
    rtt_ms: Option<u32>,
    error: Option<String>,
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);
/// Where a guest sends "add to queue" (base url, token).
static GUEST_TARGET: Mutex<Option<Target>> = Mutex::new(None);

pub fn info() -> JamInfo {
    match &*SESSION.lock() {
        None => JamInfo { mode: "off".into(), ..Default::default() },
        Some(Session::Host { code, guests, relay, .. }) => {
            let mut g = guests.lock();
            g.retain(|_, t| t.elapsed() < Duration::from_secs(10));
            let n = g.len() + relay.guests.load(std::sync::atomic::Ordering::Relaxed);
            JamInfo {
                mode: "hosting".into(),
                code: Some(code.clone()),
                guests: n,
                error: relay.error.lock().clone(),
                ..Default::default()
            }
        }
        Some(Session::Guest { status, .. }) => {
            let s = status.lock().clone();
            JamInfo { mode: "joined".into(), following: s.following, rtt_ms: s.rtt_ms, error: s.error, ..Default::default() }
        }
    }
}

pub fn stop() {
    *GUEST_TARGET.lock() = None;
    match SESSION.lock().take() {
        Some(Session::Host { stop, .. }) | Some(Session::Guest { stop, .. }) => {
            let _ = stop.send(true);
        }
        None => {}
    }
}

// ------------------------------------------------------------------ host

/// Start hosting. With `server` set, rooms live on that relay (URL, optional
/// password); otherwise guests connect straight to this machine on the LAN.
pub async fn host(app: Arc<crate::App>, server: Option<(String, String)>) -> JamInfo {
    stop();
    if let Some((url, password)) = server.filter(|(u, _)| !u.trim().is_empty()) {
        return host_relay(app, &url, &password).await;
    }
    let listener = match TcpListener::bind(("0.0.0.0", DEFAULT_PORT)).await {
        Ok(l) => l,
        Err(_) => match TcpListener::bind(("0.0.0.0", 0)).await {
            Ok(l) => l,
            Err(e) => return err_info(format!("Couldn't open a port: {e}")),
        },
    };
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(DEFAULT_PORT);
    let Some(ip) = lan_ip() else { return err_info("No local network found".into()) };
    let token = new_token();
    let code = format!("{}#{token}", SocketAddr::new(ip, port));
    let (tx, rx) = tokio::sync::watch::channel(false);
    let guests: Arc<Mutex<HashMap<IpAddr, Instant>>> = Default::default();
    *SESSION.lock() = Some(Session::Host { code, stop: tx, guests: guests.clone(), relay: Default::default() });
    crate::diag::log(format!("jam: hosting on port {port}"));
    let srv = Arc::new(Host { app, token, guests, conns: Default::default() });
    tauri::async_runtime::spawn(serve(listener, srv, rx));
    info()
}

/// Normalise what the user typed into `https://host[:port]` (no trailing slash).
/// Plain http is only allowed for local addresses: the join token and the
/// server password must not cross the internet unencrypted.
pub fn normalize_server(input: &str) -> Result<String, String> {
    let raw = input.trim();
    let with_scheme = if raw.contains("://") { raw.to_string() } else { format!("https://{raw}") };
    let u = url::Url::parse(&with_scheme).map_err(|_| "That server address doesn't look right".to_string())?;
    let host = u.host_str().ok_or("That server address doesn't look right")?;
    let local = host == "localhost"
        || host.parse::<IpAddr>().is_ok_and(is_local_peer)
        || host.trim_matches(['[', ']']).parse::<IpAddr>().is_ok_and(is_local_peer);
    match u.scheme() {
        "https" => {}
        "http" if local => {}
        "http" => return Err("Use https:// for a server outside your network".into()),
        _ => return Err("That server address doesn't look right".into()),
    }
    let mut base = format!("{}://{}", u.scheme(), host);
    if let Some(p) = u.port() {
        base.push_str(&format!(":{p}"));
    }
    Ok(base)
}

#[derive(Deserialize)]
struct Created {
    room: String,
    host_key: String,
    token: String,
}

#[derive(Deserialize, Default)]
struct PushReply {
    #[serde(default)]
    guests: usize,
    #[serde(default)]
    adds: Vec<Track>,
}

async fn host_relay(app: Arc<crate::App>, server: &str, password: &str) -> JamInfo {
    let base = match normalize_server(server) {
        Ok(b) => b,
        Err(e) => return err_info(e),
    };
    let http = reqwest::Client::new();
    let mut req = http.post(format!("{base}/v1/rooms")).timeout(Duration::from_secs(8));
    if !password.is_empty() {
        req = req.bearer_auth(password);
    }
    let created: Created = match req.send().await {
        Ok(r) if r.status() == reqwest::StatusCode::UNAUTHORIZED => return err_info("Wrong server password".into()),
        Ok(r) if r.status().is_success() => match r.json().await {
            Ok(c) => c,
            Err(_) => return err_info("That doesn't look like a Tarn jam server".into()),
        },
        Ok(r) => return err_info(format!("The server answered {}", r.status())),
        Err(_) => return err_info("Can't reach the server".into()),
    };
    let code = format!("{base}/r/{}#{}", created.room, created.token);
    let (tx, rx) = tokio::sync::watch::channel(false);
    let relay: Arc<RelayStatus> = Default::default();
    *SESSION.lock() = Some(Session::Host { code, stop: tx, guests: Default::default(), relay: relay.clone() });
    crate::diag::log("jam: hosting on a relay");
    tauri::async_runtime::spawn(push_loop(app, http, base, created, relay, rx));
    info()
}

async fn push_loop(
    app: Arc<crate::App>,
    http: reqwest::Client,
    base: String,
    room: Created,
    status: Arc<RelayStatus>,
    mut stop: tokio::sync::watch::Receiver<bool>,
) {
    let url = format!("{base}/v1/rooms/{}", room.room);
    loop {
        tokio::select! {
            _ = stop.changed() => break,
            _ = tokio::time::sleep(POLL) => {}
        }
        let res = http
            .put(format!("{url}/state"))
            .header("X-Tarn-Host", &room.host_key)
            .timeout(Duration::from_secs(4))
            .json(&host_state(&app))
            .send()
            .await;
        match res {
            Ok(r) if r.status().is_success() => {
                let reply: PushReply = r.json().await.unwrap_or_default();
                status.guests.store(reply.guests, std::sync::atomic::Ordering::Relaxed);
                *status.error.lock() = None;
                let adds: Vec<Track> = reply.adds.into_iter().filter(|t| !t.id.is_empty()).collect();
                if !adds.is_empty() {
                    crate::diag::log(format!("jam: {} song(s) added by guests", adds.len()));
                    app.player.append(adds);
                }
            }
            Ok(r) if r.status() == reqwest::StatusCode::NOT_FOUND => {
                *status.error.lock() = Some("The server dropped this room, stop and host again".into());
            }
            Ok(r) => *status.error.lock() = Some(format!("The server answered {}", r.status())),
            Err(_) => *status.error.lock() = Some("Can't reach the server".into()),
        }
    }
    let _ = http
        .delete(&url)
        .header("X-Tarn-Host", &room.host_key)
        .timeout(Duration::from_secs(3))
        .send()
        .await;
    crate::diag::log("jam: relay host stopped");
}

fn err_info(e: String) -> JamInfo {
    JamInfo { mode: "off".into(), error: Some(e), ..Default::default() }
}

struct Host {
    app: Arc<crate::App>,
    token: String,
    guests: Arc<Mutex<HashMap<IpAddr, Instant>>>,
    conns: std::sync::atomic::AtomicUsize,
}

fn host_state(app: &crate::App) -> JamState {
    let s = app.player.snapshot();
    let (queue, index) = app.player.queue_and_index();
    let upcoming = queue.iter().skip(index + 1).take(20).cloned().collect();
    JamState { track: s.track, playing: s.playing, position: s.position, loading: s.loading, upcoming }
}

async fn serve(listener: TcpListener, srv: Arc<Host>, mut stop: tokio::sync::watch::Receiver<bool>) {
    use std::sync::atomic::Ordering;
    loop {
        tokio::select! {
            _ = stop.changed() => break,
            acc = listener.accept() => {
                let Ok((sock, peer)) = acc else { continue };
                if !is_local_peer(peer.ip()) || srv.conns.load(Ordering::Relaxed) >= MAX_CONNS {
                    continue;
                }
                let srv = srv.clone();
                srv.conns.fetch_add(1, Ordering::Relaxed);
                tauri::async_runtime::spawn(async move {
                    let _ = tokio::time::timeout(Duration::from_secs(10), handle_conn(&srv, sock, peer.ip())).await;
                    srv.conns.fetch_sub(1, Ordering::Relaxed);
                });
            }
        }
    }
    crate::diag::log("jam: host stopped");
}

async fn handle_conn(srv: &Host, mut sock: TcpStream, peer: IpAddr) {
    let Some(req) = read_request(&mut sock).await else { return };
    if !req.token.as_deref().is_some_and(|t| token_ok(t, &srv.token)) {
        respond(&mut sock, "401 Unauthorized", b"bad token").await;
        return;
    }
    srv.guests.lock().insert(peer, Instant::now());
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/jam/state") => {
            let b = serde_json::to_vec(&host_state(&srv.app)).unwrap_or_default();
            respond(&mut sock, "200 OK", &b).await;
        }
        ("POST", "/jam/add") => match serde_json::from_slice::<Track>(&req.body) {
            Ok(t) if !t.id.is_empty() => {
                crate::diag::log(format!("jam: guest added {}", t.id));
                srv.app.player.append(vec![t]);
                respond(&mut sock, "204 No Content", b"").await;
            }
            _ => respond(&mut sock, "400 Bad Request", b"bad track").await,
        },
        _ => respond(&mut sock, "404 Not Found", b"not found").await,
    }
}

// ----------------------------------------------------------------- guest

/// Where a guest talks to: the host directly (LAN) or a relay room.
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub base: String,
    pub state_path: String,
    pub add_path: String,
    pub token: String,
}

fn good_token(t: &str) -> bool {
    t.len() >= 16 && t.chars().all(|c| c.is_ascii_hexdigit())
}

/// Parse "192.168.1.5:47811#token" (LAN, also accepts a leading http://) or
/// "https://server/r/<room>#token" (relay).
pub fn parse_code(code: &str) -> Option<Target> {
    let c = code.trim();
    if let Some(rest) = c.strip_prefix("https://").or_else(|| c.strip_prefix("http://")) {
        if let Some((hostpart, tail)) = rest.split_once("/r/") {
            let (room, token) = tail.split_once('#')?;
            let room = room.trim_end_matches('/');
            let token = token.trim();
            if room.is_empty() || !room.chars().all(|c| c.is_ascii_alphanumeric()) || !good_token(token) {
                return None;
            }
            let scheme = if c.starts_with("https://") { "https" } else { "http" };
            let base = normalize_server(&format!("{scheme}://{hostpart}")).ok()?;
            return Some(Target {
                base,
                state_path: format!("/v1/rooms/{room}/state"),
                add_path: format!("/v1/rooms/{room}/add"),
                token: token.to_string(),
            });
        }
    }
    let c = c.trim_start_matches("http://").trim_start_matches("tarn-jam://");
    let (addr, token) = c.split_once('#')?;
    let addr: SocketAddr = addr.trim_end_matches('/').parse().ok()?;
    let token = token.trim();
    if !good_token(token) || !is_local_peer(addr.ip()) {
        return None;
    }
    Some(Target {
        base: format!("http://{addr}"),
        state_path: "/jam/state".into(),
        add_path: "/jam/add".into(),
        token: token.to_string(),
    })
}

pub fn join(app: Arc<crate::App>, code: &str) -> JamInfo {
    stop();
    let Some(target) = parse_code(code) else {
        return err_info("That join code doesn't look right".into());
    };
    let (tx, rx) = tokio::sync::watch::channel(false);
    let status: Arc<Mutex<GuestStatus>> = Default::default();
    *SESSION.lock() = Some(Session::Guest { stop: tx, status: status.clone() });
    *GUEST_TARGET.lock() = Some(target.clone());
    crate::diag::log("jam: joined a session");
    tauri::async_runtime::spawn(follow(app, target, status, rx));
    info()
}

pub async fn add_to_host(track: Track) -> Result<(), String> {
    let Some(t) = GUEST_TARGET.lock().clone() else {
        return Err("Not in a listen-together session".into());
    };
    let r = reqwest::Client::new()
        .post(format!("{}{}", t.base, t.add_path))
        .header("X-Tarn-Token", t.token)
        .timeout(Duration::from_secs(4))
        .json(&track)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if r.status().is_success() { Ok(()) } else { Err(format!("Host answered {}", r.status())) }
}

/// What a guest should do to match the host. Pure, for tests.
#[derive(Debug, PartialEq)]
pub enum SyncAction {
    None,
    Pause,
    Resume,
    Seek(f64),
    /// Load the host's track at this position (paused if the host is).
    Load { start: f64, paused: bool },
}

pub fn plan(host: &JamState, host_pos: f64, mine: &crate::models::PlayerState) -> SyncAction {
    let Some(ht) = host.track.as_ref() else {
        return if mine.playing { SyncAction::Pause } else { SyncAction::None };
    };
    if mine.track.as_ref().map(|t| t.id.as_str()) != Some(ht.id.as_str()) {
        return SyncAction::Load { start: host_pos.max(0.0), paused: !host.playing };
    }
    if mine.loading || host.loading {
        return SyncAction::None;
    }
    if host.playing != mine.playing {
        return if host.playing { SyncAction::Resume } else { SyncAction::Pause };
    }
    if (mine.position - host_pos).abs() > DRIFT_S {
        return SyncAction::Seek(host_pos.max(0.0));
    }
    SyncAction::None
}

async fn follow(
    app: Arc<crate::App>,
    target: Target,
    status: Arc<Mutex<GuestStatus>>,
    mut stop: tokio::sync::watch::Receiver<bool>,
) {
    let http = reqwest::Client::new();
    let mut last_upcoming: Vec<String> = Vec::new();
    loop {
        tokio::select! {
            _ = stop.changed() => break,
            _ = tokio::time::sleep(POLL) => {}
        }
        let t0 = Instant::now();
        let res = http
            .get(format!("{}{}", target.base, target.state_path))
            .header("X-Tarn-Token", &target.token)
            .timeout(Duration::from_secs(3))
            .send()
            .await;
        let host: JamState = match res {
            Ok(r)
                if r.status() == reqwest::StatusCode::UNAUTHORIZED || r.status() == reqwest::StatusCode::NOT_FOUND =>
            {
                status.lock().error = Some("The host ended the session".into());
                continue;
            }
            Ok(r) => match r.json().await {
                Ok(s) => s,
                Err(e) => {
                    status.lock().error = Some(format!("Bad answer from host: {e}"));
                    continue;
                }
            },
            Err(_) => {
                status.lock().error = Some("Can't reach the host".into());
                continue;
            }
        };
        let rtt = t0.elapsed();
        {
            let mut s = status.lock();
            s.error = None;
            s.rtt_ms = Some(rtt.as_millis().min(u32::MAX as u128) as u32);
            s.following = host.track.as_ref().map(|t| t.title.clone());
        }
        // The host's position was read about half a round trip ago.
        let host_pos = host.position + if host.playing { rtt.as_secs_f64() / 2.0 } else { 0.0 };
        let mine = app.player.snapshot();
        let action = plan(&host, host_pos, &mine);
        let upcoming: Vec<String> = host.upcoming.iter().map(|t| t.id.clone()).collect();
        match action {
            SyncAction::None => {}
            SyncAction::Pause => app.player.pause(),
            SyncAction::Resume => app.player.resume(),
            SyncAction::Seek(p) => app.player.seek(p),
            SyncAction::Load { start, paused } => {
                let Some(t) = host.track.clone() else { continue };
                let mut queue = vec![t.clone()];
                queue.extend(host.upcoming.iter().cloned());
                last_upcoming = upcoming.clone();
                if let Err(e) = crate::start_playback_at(&app, t, queue, 0, start, paused, true, false).await {
                    status.lock().error = Some(format!("Couldn't play the host's song: {e}"));
                }
                continue;
            }
        }
        // Keep the guest's "up next" in step with the host.
        if upcoming != last_upcoming {
            last_upcoming = upcoming;
            app.player.clear_upcoming();
            app.player.append(host.upcoming);
        }
    }
    crate::diag::log("jam: left the session");
}

// ---------------------------------------------------------------- shared

fn new_token() -> String {
    let mut b = [0u8; 24];
    rand::rngs::OsRng.fill_bytes(&mut b);
    hex::encode(b)
}

fn lan_ip() -> Option<IpAddr> {
    let s = std::net::UdpSocket::bind(("0.0.0.0", 0)).ok()?;
    s.connect(("192.0.2.1", 9)).ok()?; // TEST-NET-1: nothing is sent
    let ip = s.local_addr().ok()?.ip();
    (!ip.is_unspecified() && !ip.is_loopback()).then_some(ip)
}

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

fn token_ok(given: &str, want: &str) -> bool {
    let (a, b) = (given.as_bytes(), want.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

struct Request {
    method: String,
    path: String,
    token: Option<String>,
    body: Vec<u8>,
}

async fn read_request(sock: &mut TcpStream) -> Option<Request> {
    let mut buf = Vec::with_capacity(1024);
    let mut tmp = [0u8; 2048];
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
    let path = first.next()?.split('?').next()?.to_string();
    let (mut len, mut token) = (0usize, None);
    for l in lines {
        let Some((k, v)) = l.split_once(':') else { continue };
        match k.trim().to_ascii_lowercase().as_str() {
            "content-length" => len = v.trim().parse().ok()?,
            "x-tarn-token" => token = Some(v.trim().to_string()),
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

async fn respond(sock: &mut TcpStream, status: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = sock.write_all(head.as_bytes()).await;
    let _ = sock.write_all(body).await;
    let _ = sock.flush().await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::PlayerState;

    fn track(id: &str) -> Track {
        Track { id: id.into(), title: id.into(), ..Default::default() }
    }

    fn host(id: Option<&str>, playing: bool, pos: f64) -> JamState {
        JamState { track: id.map(track), playing, position: pos, loading: false, upcoming: vec![] }
    }

    fn me(id: Option<&str>, playing: bool, pos: f64) -> PlayerState {
        PlayerState { track: id.map(track), playing, position: pos, ..Default::default() }
    }

    #[test]
    fn plans_sync() {
        assert_eq!(plan(&host(Some("a"), true, 10.0), 10.0, &me(None, false, 0.0)), SyncAction::Load { start: 10.0, paused: false });
        assert_eq!(plan(&host(Some("a"), false, 5.0), 5.0, &me(Some("b"), true, 1.0)), SyncAction::Load { start: 5.0, paused: true });
        assert_eq!(plan(&host(Some("a"), true, 10.0), 10.0, &me(Some("a"), false, 10.0)), SyncAction::Resume);
        assert_eq!(plan(&host(Some("a"), false, 10.0), 10.0, &me(Some("a"), true, 10.0)), SyncAction::Pause);
        assert_eq!(plan(&host(Some("a"), true, 10.0), 10.0, &me(Some("a"), true, 10.8)), SyncAction::None);
        assert_eq!(plan(&host(Some("a"), true, 10.0), 10.0, &me(Some("a"), true, 14.0)), SyncAction::Seek(10.0));
        assert_eq!(plan(&host(None, false, 0.0), 0.0, &me(Some("a"), true, 3.0)), SyncAction::Pause);
    }

    #[test]
    fn join_codes() {
        let tok = "0123456789abcdef0123456789abcdef";
        let t = parse_code(&format!(" 192.168.1.5:47811#{tok} ")).unwrap();
        assert_eq!((t.base.as_str(), t.token.as_str()), ("http://192.168.1.5:47811", tok));
        let r = parse_code(&format!("https://jam.example.com/r/abc123#{tok}")).unwrap();
        assert_eq!((r.base.as_str(), r.state_path.as_str()), ("https://jam.example.com", "/v1/rooms/abc123/state"));
        assert!(parse_code(&format!("http://jam.example.com/r/abc123#{tok}")).is_none(), "plain http to the internet is refused");
        assert!(parse_code(&format!("https://jam.example.com/r/ab/../c#{tok}")).is_none());
        assert!(parse_code(&format!("http://127.0.0.1:8080/r/abc123#{tok}")).is_some());
        assert!(parse_code(&format!("http://10.0.0.2:5000/#{tok}")).is_some());
        assert!(parse_code(&format!("8.8.8.8:47811#{tok}")).is_none(), "public hosts are refused");
        assert!(parse_code("192.168.1.5:47811#short").is_none());
        assert!(parse_code("192.168.1.5:47811").is_none());
    }

    #[test]
    fn tokens() {
        let t = new_token();
        assert!(token_ok(&t, &t) && !token_ok(&t[1..], &t) && t.len() == 48);
    }
}
