#!/bin/sh
# Installs tarn-jam-server as a hardened systemd service on 127.0.0.1:47821.
# Point your tunnel / reverse proxy at it.
#
#   sudo ./install.sh
#
# Optional: TARN_JAM_PASSWORD=... to require a password for hosting (leave it
# unset for an open public server).
set -eu
[ "$(id -u)" = 0 ] || { echo "run as root (sudo)"; exit 1; }
cd "$(dirname "$0")"

command -v cargo >/dev/null 2>&1 || { echo "Rust is needed to build: https://rustup.rs (or: apt/dnf install cargo)"; exit 1; }
cargo build --release --locked
install -m 0755 target/release/tarn-jam-server /usr/local/bin/tarn-jam-server

id tarn-jam >/dev/null 2>&1 || useradd --system --no-create-home --shell /usr/sbin/nologin tarn-jam
mkdir -p /etc/tarn-jam
ENVF=/etc/tarn-jam/env
if [ ! -f "$ENVF" ]; then
  {
    echo "TARN_JAM_BIND=127.0.0.1:47821"
    echo "TARN_JAM_TRUST_PROXY=1"
    [ -n "${TARN_JAM_PASSWORD:-}" ] && echo "TARN_JAM_PASSWORD=$TARN_JAM_PASSWORD"
    echo "# TARN_JAM_MAX_ROOMS=1000"
    echo "# TARN_JAM_MAX_ROOMS_PER_IP=3"
  } > "$ENVF"
  chmod 600 "$ENVF"
fi

cat > /etc/systemd/system/tarn-jam.service <<'UNIT'
[Unit]
Description=Tarn Listen Along relay
After=network-online.target
Wants=network-online.target

[Service]
User=tarn-jam
EnvironmentFile=/etc/tarn-jam/env
ExecStart=/usr/local/bin/tarn-jam-server
Restart=always
RestartSec=2
LimitNOFILE=65536
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
PrivateDevices=true
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectControlGroups=true
RestrictAddressFamilies=AF_INET AF_INET6
RestrictNamespaces=true
LockPersonality=true
MemoryDenyWriteExecute=true
CapabilityBoundingSet=
SystemCallFilter=@system-service

[Install]
WantedBy=multi-user.target
UNIT
systemctl daemon-reload
systemctl enable --now tarn-jam
systemctl restart tarn-jam

echo "Done: listening on $(grep ^TARN_JAM_BIND= $ENVF | cut -d= -f2). Put your tunnel/HTTPS proxy in front before using it over the internet."
