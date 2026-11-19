//! tarn-jam-server: a tiny self-hosted relay for Tarn's Listen Along.
//!
//! The host's app pushes its playback state here once a second; guests poll
//! it (and may add songs to the host's queue). Nothing is stored on disk and
//! the server never touches audio, it only relays a few hundred bytes of JSON.
//!
//! Security model
//! * A room has two random 192-bit secrets: a *host key* (push state, close the
//!   room, receive guest additions) and a *join token* (read state, add songs).
//! * Optionally `TARN_JAM_PASSWORD` is required to create rooms, so a public
//!   server can't be used by strangers.
//! * Per-IP rate limiting, hard caps on rooms/guests/body sizes, rooms die
//!   10 s after the host stops pushing. Secrets are compared in constant time.
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

const ROOM_TTL: Duration = Duration::from_secs(10);
const GUEST_WINDOW: Duration = Duration::from_secs(10);
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
    token: String,
    /// Last state pushed by the host, plus when it arrived.
    state: Option<(Value, Instant)>,
    last_push: Instant,
    adds: VecDeque<Value>,
    guests: HashMap<IpAddr, Instant>,
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
}

type Shared = Arc<Inner>;

fn token(n: usize) -> String {
    let mut b = vec![0u8; n];
    rand::rngs::OsRng.fill_bytes(&mut b);
    hex::encode(b)
}

fn eq_ct(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
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
    let id = token(6);
    let room = Room {
        owner: ip,
        host_key: token(24),
        token: token(24),
        state: None,
        last_push: Instant::now(),
        adds: VecDeque::new(),
        guests: HashMap::new(),
    };
    let out = Created { room: id.clone(), host_key: room.host_key.clone(), token: room.token.clone() };
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
    gate!(s, peer, h);
    let ok = p.position.is_finite()
        && (0.0..=86_400.0).contains(&p.position)
        && p.track.as_ref().is_none_or(valid_track)
        && p.upcoming.len() <= MAX_UPCOMING
        && p.upcoming.iter().all(valid_track);
    if !ok {
        return err(StatusCode::BAD_REQUEST, "bad state");
    }
    let mut rooms = s.rooms.lock().unwrap();
    sweep(&mut rooms);
    let Some(r) = rooms.get_mut(&id).filter(|r| eq_ct(header(&h, "x-tarn-host"), &r.host_key)) else {
        return err(StatusCode::NOT_FOUND, "no such room");
    };
    let state = serde_json::json!({
        "track": p.track, "playing": p.playing, "position": p.position,
        "loading": p.loading, "upcoming": p.upcoming,
    });
    r.state = Some((state, Instant::now()));
    r.last_push = Instant::now();
    r.guests.retain(|_, t| t.elapsed() < GUEST_WINDOW);
    let adds: Vec<Value> = r.adds.drain(..).collect();
    Json(serde_json::json!({ "guests": r.guests.len(), "adds": adds })).into_response()
}

async fn read_state(
    State(s): State<Shared>,
    peer: ConnectInfo<SocketAddr>,
    Path(id): Path<String>,
    h: HeaderMap,
) -> Response {
    let ip = gate!(s, peer, h);
    let mut rooms = s.rooms.lock().unwrap();
    sweep(&mut rooms);
    let Some(r) = rooms.get_mut(&id).filter(|r| eq_ct(header(&h, "x-tarn-token"), &r.token)) else {
        return err(StatusCode::NOT_FOUND, "no such room");
    };
    if r.guests.len() >= MAX_GUESTS && !r.guests.contains_key(&ip) {
        return err(StatusCode::SERVICE_UNAVAILABLE, "room is full");
    }
    r.guests.insert(ip, Instant::now());
    let Some((state, at)) = &r.state else {
        return Json(serde_json::json!({ "track": null, "playing": false, "position": 0.0, "loading": true, "upcoming": [] }))
            .into_response();
    };
    // Age the host's position so a late poll still lines up.
    let mut out = state.clone();
    if out["playing"] == Value::Bool(true) {
        let pos = out["position"].as_f64().unwrap_or(0.0) + at.elapsed().as_secs_f64();
        out["position"] = serde_json::json!(pos);
    }
    Json(out).into_response()
}

async fn add(
    State(s): State<Shared>,
    peer: ConnectInfo<SocketAddr>,
    Path(id): Path<String>,
    h: HeaderMap,
    Json(t): Json<Value>,
) -> Response {
    gate!(s, peer, h);
    if !valid_track(&t) {
        return err(StatusCode::BAD_REQUEST, "bad track");
    }
    let mut rooms = s.rooms.lock().unwrap();
    let Some(r) = rooms.get_mut(&id).filter(|r| eq_ct(header(&h, "x-tarn-token"), &r.token)) else {
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
    });
    let app = Router::new()
        .route("/healthz", get(health))
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
