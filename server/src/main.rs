//! tarn-jam-server: a tiny self-hosted relay for Tarn's Listen Along.
//!
//! The host's app pushes its playback state here once a second; guests poll
//! it (and may add songs to the host's queue). Nothing is stored on disk and
//! the server never touches audio, it only relays a few hundred bytes of JSON.
//!
//! Security model
//! * A room is a 6-digit code people can read out or type by hand. The code
//!   alone lets you listen along and add songs; only the host's random
//!   192-bit *host key* can push state or close the room.
//! * Guessing codes is throttled: each IP may try only a few distinct wrong
//!   codes per 10 minutes.
//! * Optionally `TARN_JAM_PASSWORD` is required to create rooms, so a public
//!   server can't be used by strangers.
//! * Per-IP rate limiting, hard caps on rooms/guests/body sizes, rooms die
//!   90 s after the host stops pushing (a host that comes back in time, or
//!   after a server restart, gets its room back with the same code).
//!   Secrets are compared in constant time.
//! * Run it behind a TLS terminator (the bundled docker-compose does this with
//!   Caddy and automatic HTTPS).

use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, DefaultBodyLimit, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Mutex;

/// A host that goes quiet (network blip, laptop asleep) keeps its room this long.
const ROOM_TTL: Duration = Duration::from_secs(90);
/// A guest still counts as listening this long after their last poll (loading
/// a song on the guest can pause polling for several seconds).
const GUEST_WINDOW: Duration = Duration::from_secs(30);
/// Distinct wrong room codes one IP may try per GUESS_WINDOW.
const MAX_WRONG_CODES: usize = 20;
const GUESS_WINDOW: Duration = Duration::from_secs(600);
const MAX_GUESTS: usize = 50;
const MAX_PENDING_ADDS: usize = 50;
const MAX_UPCOMING: usize = 20;
const MAX_BODY: usize = 64 * 1024;
/// Requests per second sustained / burst, per client IP.
const RATE_PER_SEC: f64 = 20.0;
const RATE_BURST: f64 = 60.0;
/// Room creation is limited much harder: this many per IP per minute.
const CREATE_PER_MIN: usize = 10;

struct Room {
    owner: IpAddr,
    host_key: String,
    /// Last state pushed by the host, plus when it arrived.
    state: Option<(Value, Instant)>,
    last_push: Instant,
    adds: VecDeque<Value>,
    host_name: String,
    /// guest id -> (display name, last seen)
    guests: HashMap<String, (String, Instant)>,
}

struct Config {
    password: Option<String>,
    max_rooms: usize,
    max_rooms_per_ip: usize,
    trust_proxy: bool,
}

struct Bucket {
    tokens: f64,
    at: Instant,
}

struct Inner {
    cfg: Config,
    rooms: Mutex<HashMap<String, Room>>,
    buckets: Mutex<HashMap<IpAddr, Bucket>>,
    creates: Mutex<HashMap<IpAddr, Vec<Instant>>>,
    /// Wrong room codes tried per IP: (code, when).
    misses: Mutex<HashMap<IpAddr, Vec<(String, Instant)>>>,
}

type Shared = Arc<Inner>;

fn token(n: usize) -> String {
    let mut b = vec![0u8; n];
    rand::rngs::OsRng.fill_bytes(&mut b);
    hex::encode(b)
}

/// A fresh 6-digit room code not in use.
fn new_code(rooms: &HashMap<String, Room>) -> String {
    loop {
        let mut b = [0u8; 4];
        rand::rngs::OsRng.fill_bytes(&mut b);
        let c = format!("{:06}", u32::from_le_bytes(b) % 1_000_000);
        if !rooms.contains_key(&c) {
            return c;
        }
    }
}

fn valid_code(c: &str) -> bool {
    c.len() == 6 && c.bytes().all(|b| b.is_ascii_digit())
}

fn valid_host_key(k: &str) -> bool {
    k.len() == 48 && k.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Too many different wrong codes from this IP lately? Re-polling the same dead
/// room (a guest whose host left) only counts once.
fn guessing(s: &Inner, ip: IpAddr) -> bool {
    let mut m = s.misses.lock().unwrap();
    m.get_mut(&ip).is_some_and(|v| {
        v.retain(|(_, t)| t.elapsed() < GUESS_WINDOW);
        v.len() >= MAX_WRONG_CODES
    })
}

fn note_miss(s: &Inner, ip: IpAddr, code: &str) {
    let mut m = s.misses.lock().unwrap();
    if m.len() > 10_000 {
        m.retain(|_, v| v.iter().any(|(_, t)| t.elapsed() < GUESS_WINDOW));
    }
    let v = m.entry(ip).or_default();
    if !v.iter().any(|(c, _)| c == code) {
        v.push((code.chars().take(16).collect(), Instant::now()));
    }
}

fn eq_ct(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Names are shown to other people: strip control chars, cap the length.
fn clean_name(raw: &str) -> String {
    raw.chars().filter(|c| !c.is_control()).collect::<String>().trim().chars().take(24).collect()
}

/// Header values are percent-encoded UTF-8 (names can have any characters).
fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn guest_names(r: &Room) -> Vec<String> {
    let mut v: Vec<_> = r.guests.values().filter(|(_, t)| t.elapsed() < GUEST_WINDOW).map(|(n, _)| n.clone()).collect();
    v.sort();
    v
}

fn err(code: StatusCode, msg: &'static str) -> Response {
    (code, Json(serde_json::json!({ "error": msg }))).into_response()
}

fn header<'a>(h: &'a HeaderMap, name: &str) -> &'a str {
    h.get(name).and_then(|v| v.to_str().ok()).unwrap_or("")
}

fn client_ip(s: &Inner, peer: SocketAddr, h: &HeaderMap) -> IpAddr {
    if s.cfg.trust_proxy {
        // The last hop we trust (our own reverse proxy) appends the real client.
        if let Some(ip) = header(h, "x-forwarded-for").rsplit(',').next().and_then(|v| v.trim().parse().ok()) {
            return ip;
        }
    }
    peer.ip()
}

/// Token bucket per IP. Returns false when the caller should back off.
fn allow(s: &Inner, ip: IpAddr) -> bool {
    let now = Instant::now();
    let mut m = s.buckets.lock().unwrap();
    if m.len() > 10_000 {
        m.retain(|_, b| now.duration_since(b.at) < Duration::from_secs(60));
    }
    let b = m.entry(ip).or_insert(Bucket { tokens: RATE_BURST, at: now });
    b.tokens = (b.tokens + now.duration_since(b.at).as_secs_f64() * RATE_PER_SEC).min(RATE_BURST);
    b.at = now;
    if b.tokens >= 1.0 {
        b.tokens -= 1.0;
        true
    } else {
        false
    }
}

macro_rules! gate {
    ($s:expr, $peer:expr, $h:expr) => {{
        let ip = client_ip(&$s, $peer.0, &$h);
        if !allow(&$s, ip) {
            return err(StatusCode::TOO_MANY_REQUESTS, "slow down");
        }
        ip
    }};
}

fn sweep(rooms: &mut HashMap<String, Room>) {
    rooms.retain(|_, r| r.last_push.elapsed() < ROOM_TTL);
}

// ------------------------------------------------------------- handlers

#[derive(Serialize)]
struct Created {
    room: String,
    host_key: String,
    /// Unused since rooms are joined by code; kept so older apps that expect
    /// `https://server/r/<room>#<token>` links still work.
    token: String,
}

async fn create(State(s): State<Shared>, peer: ConnectInfo<SocketAddr>, h: HeaderMap) -> Response {
    let ip = gate!(s, peer, h);
    if let Some(pw) = &s.cfg.password {
        let given = header(&h, "authorization").strip_prefix("Bearer ").unwrap_or("");
        if !eq_ct(given, pw) {
            // Slow wrong guesses down a little more.
            tokio::time::sleep(Duration::from_millis(400)).await;
            return err(StatusCode::UNAUTHORIZED, "wrong server password");
        }
    }
    {
        let mut c = s.creates.lock().unwrap();
        let v = c.entry(ip).or_default();
        v.retain(|t| t.elapsed() < Duration::from_secs(60));
        if v.len() >= CREATE_PER_MIN {
            return err(StatusCode::TOO_MANY_REQUESTS, "too many rooms, try again in a minute");
        }
        v.push(Instant::now());
        if c.len() > 10_000 {
            c.retain(|_, v| v.iter().any(|t| t.elapsed() < Duration::from_secs(60)));
        }
    }
    let mut rooms = s.rooms.lock().unwrap();
    sweep(&mut rooms);
    if rooms.len() >= s.cfg.max_rooms {
        return err(StatusCode::SERVICE_UNAVAILABLE, "server is full");
    }
    if rooms.values().filter(|r| r.owner == ip).count() >= s.cfg.max_rooms_per_ip {
        return err(StatusCode::TOO_MANY_REQUESTS, "you already have the maximum number of rooms open");
    }
    let id = new_code(&rooms);
    let room = Room {
        owner: ip,
        host_key: token(24),
        state: None,
        last_push: Instant::now(),
        adds: VecDeque::new(),
        host_name: String::new(),
        guests: HashMap::new(),
    };
    let out = Created { room: id.clone(), host_key: room.host_key.clone(), token: token(16) };
    rooms.insert(id, room);
    (StatusCode::CREATED, Json(out)).into_response()
}

#[derive(Deserialize)]
struct Push {
    track: Option<Value>,
    #[serde(default)]
    playing: bool,
    #[serde(default)]
    position: f64,
    #[serde(default)]
    loading: bool,
    #[serde(default)]
    upcoming: Vec<Value>,
    #[serde(default)]
    host_name: String,
}

fn valid_track(v: &Value) -> bool {
    v.as_object().is_some_and(|o| {
        o.get("id").and_then(Value::as_str).is_some_and(|i| !i.is_empty() && i.len() <= 64)
            && o.get("title").is_some_and(Value::is_string)
    })
}

async fn push(
    State(s): State<Shared>,
    peer: ConnectInfo<SocketAddr>,
    Path(id): Path<String>,
    h: HeaderMap,
    Json(p): Json<Push>,
) -> Response {
    let ip = gate!(s, peer, h);
    let ok = p.position.is_finite()
        && (0.0..=86_400.0).contains(&p.position)
        && p.track.as_ref().is_none_or(valid_track)
        && p.upcoming.len() <= MAX_UPCOMING
        && p.upcoming.iter().all(valid_track);
    if !ok {
        return err(StatusCode::BAD_REQUEST, "bad state");
    }
    let key = header(&h, "x-tarn-host");
    let mut rooms = s.rooms.lock().unwrap();
    sweep(&mut rooms);
    // The room expired or the server restarted: give the host its code back
    // (same key), so the people listening never notice.
    if !rooms.contains_key(&id) && valid_code(&id) && valid_host_key(key) {
        let mine = rooms.values().filter(|r| r.owner == ip).count();
        if rooms.len() < s.cfg.max_rooms && mine < s.cfg.max_rooms_per_ip {
            rooms.insert(
                id.clone(),
                Room {
                    owner: ip,
                    host_key: key.to_string(),
                    state: None,
                    last_push: Instant::now(),
                    adds: VecDeque::new(),
                    host_name: String::new(),
                    guests: HashMap::new(),
                },
            );
        }
    }
    let Some(r) = rooms.get_mut(&id).filter(|r| eq_ct(key, &r.host_key)) else {
        return err(StatusCode::NOT_FOUND, "no such room");
    };
    let state = serde_json::json!({
        "track": p.track, "playing": p.playing, "position": p.position,
        "loading": p.loading, "upcoming": p.upcoming,
    });
    r.state = Some((state, Instant::now()));
    r.last_push = Instant::now();
    r.host_name = clean_name(&p.host_name);
    r.guests.retain(|_, (_, t)| t.elapsed() < GUEST_WINDOW);
    let adds: Vec<Value> = r.adds.drain(..).collect();
    Json(serde_json::json!({ "guests": r.guests.len(), "names": guest_names(r), "adds": adds })).into_response()
}

async fn read_state(
    State(s): State<Shared>,
    peer: ConnectInfo<SocketAddr>,
    Path(id): Path<String>,
    h: HeaderMap,
) -> Response {
    let ip = gate!(s, peer, h);
    if guessing(&s, ip) {
        return err(StatusCode::TOO_MANY_REQUESTS, "too many wrong codes, try again later");
    }
    let mut rooms = s.rooms.lock().unwrap();
    sweep(&mut rooms);
    let Some(r) = rooms.get_mut(&id) else {
        drop(rooms);
        note_miss(&s, ip, &id);
        return err(StatusCode::NOT_FOUND, "no such room");
    };
    // Guests identify themselves with a random id (so two people behind one
    // router count separately) and a display name.
    let gid: String = header(&h, "x-tarn-guest").chars().filter(char::is_ascii_alphanumeric).take(32).collect();
    let gid = if gid.is_empty() { ip.to_string() } else { gid };
    let name = clean_name(&pct_decode(header(&h, "x-tarn-name")));
    let name = if name.is_empty() { "Guest".to_string() } else { name };
    r.guests.retain(|_, (_, t)| t.elapsed() < GUEST_WINDOW);
    if r.guests.len() >= MAX_GUESTS && !r.guests.contains_key(&gid) {
        return err(StatusCode::SERVICE_UNAVAILABLE, "room is full");
    }
    r.guests.insert(gid, (name, Instant::now()));
    let people = guest_names(r);
    let host_name = r.host_name.clone();
    let Some((state, at)) = &r.state else {
        return no_store(Json(serde_json::json!({ "track": null, "playing": false, "position": 0.0, "loading": true, "upcoming": [], "host_name": host_name, "guests": people })));
    };
    // Age the host's position so a late poll still lines up.
    let mut out = state.clone();
    out["host_name"] = serde_json::json!(host_name);
    out["guests"] = serde_json::json!(people);
    if out["playing"] == Value::Bool(true) {
        let pos = out["position"].as_f64().unwrap_or(0.0) + at.elapsed().as_secs_f64();
        out["position"] = serde_json::json!(pos);
    }
    no_store(Json(out))
}

/// Live state: never let a proxy or CDN in front of us cache it.
fn no_store(r: impl IntoResponse) -> Response {
    ([("cache-control", "no-store")], r).into_response()
}

async fn add(
    State(s): State<Shared>,
    peer: ConnectInfo<SocketAddr>,
    Path(id): Path<String>,
    h: HeaderMap,
    Json(t): Json<Value>,
) -> Response {
    let ip = gate!(s, peer, h);
    if !valid_track(&t) {
        return err(StatusCode::BAD_REQUEST, "bad track");
    }
    if guessing(&s, ip) {
        return err(StatusCode::TOO_MANY_REQUESTS, "too many wrong codes, try again later");
    }
    let mut rooms = s.rooms.lock().unwrap();
    let Some(r) = rooms.get_mut(&id) else {
        drop(rooms);
        note_miss(&s, ip, &id);
        return err(StatusCode::NOT_FOUND, "no such room");
    };
    if r.adds.len() >= MAX_PENDING_ADDS {
        return err(StatusCode::TOO_MANY_REQUESTS, "queue is busy");
    }
    r.adds.push_back(t);
    StatusCode::NO_CONTENT.into_response()
}

async fn close(
    State(s): State<Shared>,
    peer: ConnectInfo<SocketAddr>,
    Path(id): Path<String>,
    h: HeaderMap,
) -> Response {
    gate!(s, peer, h);
    let mut rooms = s.rooms.lock().unwrap();
    if rooms.get(&id).is_some_and(|r| eq_ct(header(&h, "x-tarn-host"), &r.host_key)) {
        rooms.remove(&id);
    }
    StatusCode::NO_CONTENT.into_response()
}

/// Invite landing page opened from a shared link (`/r/<code>`).
async fn invite(State(s): State<Shared>, peer: ConnectInfo<SocketAddr>, h: HeaderMap) -> Response {
    gate!(s, peer, h);
    (
        [
            ("content-type", "text/html; charset=utf-8"),
            ("cache-control", "public, max-age=300"),
            ("referrer-policy", "no-referrer"),
            ("x-content-type-options", "nosniff"),
            ("x-frame-options", "DENY"),
            (
                "content-security-policy",
                "default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
            ),
        ],
        include_str!("invite.html"),
    )
        .into_response()
}

async fn health() -> &'static str {
    "ok"
}

// ----------------------------------------------------------------- main

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

#[tokio::main]
async fn main() {
    let bind = env("TARN_JAM_BIND").unwrap_or_else(|| "0.0.0.0:47821".into());
    let cfg = Config {
        password: env("TARN_JAM_PASSWORD"),
        max_rooms: env("TARN_JAM_MAX_ROOMS").and_then(|v| v.parse().ok()).unwrap_or(1000),
        max_rooms_per_ip: env("TARN_JAM_MAX_ROOMS_PER_IP").and_then(|v| v.parse().ok()).unwrap_or(3),
        trust_proxy: env("TARN_JAM_TRUST_PROXY").is_some_and(|v| v == "1" || v == "true"),
    };
    if cfg.password.is_none() {
        eprintln!("note: TARN_JAM_PASSWORD is not set, so this is an open public server (rate limits and per-IP room caps apply)");
    }
    let state: Shared = Arc::new(Inner {
        cfg,
        rooms: Default::default(),
        buckets: Default::default(),
        creates: Default::default(),
        misses: Default::default(),
    });
    let app = Router::new()
        .route("/healthz", get(health))
        .route("/r/:id", get(invite))
        .route("/v1/rooms", post(create))
        .route("/v1/rooms/:id", delete(close))
        .route("/v1/rooms/:id/state", put(push).get(read_state))
        .route("/v1/rooms/:id/add", post(add))
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .layer(tower_http::timeout::TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, Duration::from_secs(10)))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(&bind).await.unwrap_or_else(|e| {
        eprintln!("can't listen on {bind}: {e}");
        std::process::exit(1);
    });
    eprintln!("tarn-jam-server {} listening on {bind}", env!("CARGO_PKG_VERSION"));
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .ok();
}
