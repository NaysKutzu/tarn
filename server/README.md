# Tarn jam server

A tiny self-hosted relay for **Listen Along** over the internet. The host's Tarn app pushes its playback state (track, position, play/pause, up next) to this server once a second, and guests follow it. No audio goes through the server, no ports to open at home, nothing is written to disk.

## One command

On any Linux box with Docker and a domain pointing at it (ports 80 and 443 open):

```sh
git clone --depth 1 https://github.com/rexsystems/tarn && cd tarn/server && DOMAIN=jam.example.com TARN_JAM_PASSWORD='pick-a-long-password' docker compose up -d --build
```

That builds the server, puts [Caddy](https://caddyserver.com) in front of it and gets a Let's Encrypt certificate for `DOMAIN` automatically. Check it with `curl https://jam.example.com/healthz` (answers `ok`).

No domain? Leave `DOMAIN` out and it serves plain HTTP on port 80, fine for a LAN or for putting your own reverse proxy in front. Tarn refuses plain `http://` for servers outside your network.

Update: `git pull && docker compose up -d --build`. Stop: `docker compose down`.

## Direct on a port (no Docker)

On a fresh Linux VPS with Rust installed:

```sh
git clone --depth 1 https://github.com/rexsystems/tarn && cd tarn/server && sudo ./install.sh jam.example.com
```

It builds the binary, installs a locked-down systemd service (`tarn-jam`, own user, no privileges, no filesystem access) and, if [Caddy](https://caddyserver.com/docs/install) is installed, adds the HTTPS reverse proxy for the domain. Without a domain it just listens on `:47821`. Config lives in `/etc/tarn-jam/env`; logs via `journalctl -u tarn-jam -f`. Update by pulling and running the script again.

Quick manual run, no service: `cargo build --release && ./target/release/tarn-jam-server` (port 47821), and for HTTPS in one line: `caddy reverse-proxy --from jam.example.com --to 127.0.0.1:47821`.

## Running a public server for everyone

Leave `TARN_JAM_PASSWORD` unset and anyone can host and join, which is what a community server wants. Abuse protection is built in: per-IP rate limit, **3 open rooms per IP** (`TARN_JAM_MAX_ROOMS_PER_IP`), 1000 rooms total, 10 s timeouts, tiny bodies, and secret room links. Then set `PUBLIC_JAM_SERVER` in [src/lib/jam.svelte.ts](../src/lib/jam.svelte.ts) to your URL and ship a release: Tarn shows a **Use the public server** toggle (on by default; off keeps Listen along on the local network only).

## Using it in Tarn

1. **Settings → Listen together**, fill in *Server* (`https://jam.example.com`) and *Server password*, then press **Host**.
2. Share the join code it shows (`https://jam.example.com/r/<room>#<token>`).
3. Friends paste it into the *Join code* box in their own Tarn.

Leave the server field empty to keep the original same-Wi-Fi mode.

## Without Docker

```sh
cargo build --release          # needs Rust 1.82+
TARN_JAM_PASSWORD=... ./target/release/tarn-jam-server
```

It listens on plain HTTP (`0.0.0.0:47821`), so put a TLS proxy (Caddy, nginx, Traefik) in front and set `TARN_JAM_TRUST_PROXY=1`.

## Configuration

| Variable | Default | Meaning |
| --- | --- | --- |
| `DOMAIN` | *(none, HTTP on :80)* | Domain for automatic HTTPS (compose only). |
| `TARN_JAM_PASSWORD` | *(none)* | Required to **host**. Guests only need the join code. Set it on any public server. |
| `TARN_JAM_MAX_ROOMS` | `1000` | Maximum simultaneous rooms. |
| `TARN_JAM_MAX_ROOMS_PER_IP` | `3` | Open rooms one IP may host at once. |
| `TARN_JAM_BIND` | `0.0.0.0:47821` | Listen address. |
| `TARN_JAM_TRUST_PROXY` | `0` (`1` in compose) | Read the client IP from `X-Forwarded-For`. Only enable behind your own proxy. |

## Security

- Each room has two random 192-bit secrets: a **host key** (push state, close the room, receive queue additions) and a **join token** (read state, add songs), compared in constant time. The join token only travels in the join code and a header, never in a URL the server logs.
- The server password gates room creation; wrong guesses are delayed.
- Per-IP rate limits (20 req/s, 10 new rooms/min, 3 open rooms), caps on rooms, guests per room, queued additions, and body size (64 KB).
- Rooms vanish 10 seconds after the host stops pushing. State lives in memory only.
- The container runs as non-root on a read-only filesystem with all capabilities dropped; only Caddy is exposed.
- Always use HTTPS on the internet (the compose file does it for you): the password and tokens are sent as headers.

## API

For the curious or for writing your own client. Guest header: `X-Tarn-Token`; host header: `X-Tarn-Host`.

| Request | Who | What |
| --- | --- | --- |
| `POST /v1/rooms` (`Authorization: Bearer <password>`) | host | Create a room → `{room, host_key, token}` |
| `PUT /v1/rooms/{room}/state` | host | Push `{track, playing, position, loading, upcoming}` → `{guests, adds}` |
| `GET /v1/rooms/{room}/state` | guest | Current state, position aged to now |
| `POST /v1/rooms/{room}/add` | guest | Queue a track for the host |
| `DELETE /v1/rooms/{room}` | host | Close the room |
| `GET /healthz` | anyone | `ok` |
