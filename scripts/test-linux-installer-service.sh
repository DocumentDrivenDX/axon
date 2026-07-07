#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
AXON_BIN="${AXON_BIN:-"$ROOT/target/release/axon"}"
IMAGE="${AXON_INSTALLER_TEST_IMAGE:-ubuntu:24.04}"

if ! command -v docker >/dev/null 2>&1; then
    echo "error: docker is required for the Linux installer/service test" >&2
    exit 1
fi

if [ "${AXON_INSTALLER_SKIP_BUILD:-0}" != "1" ]; then
    cargo build --release -p axon-cli
elif [ ! -x "$AXON_BIN" ]; then
    cargo build --release -p axon-cli
fi

docker run --rm \
    -v "$ROOT:/work:ro" \
    -w /work \
    -e AXON_INSTALL_URL="file:///work/target/release/axon" \
    "$IMAGE" \
    bash -euxo pipefail <<'SH'
apt-get update
apt-get install -y --no-install-recommends ca-certificates curl libsqlite3-0
rm -rf /var/lib/apt/lists/*

export HOME=/tmp/axon-home
export PATH=/tmp/axon-stubs:$HOME/.local/bin:$PATH
mkdir -p /tmp/axon-stubs "$HOME"
log_dir="$HOME/.cache/axon-installer-service-test"
mkdir -p "$log_dir"

install_log="$log_dir/install.log"
pre_doctor_log="$log_dir/doctor-installed.log"
service_install_log="$log_dir/service-install.log"
service_uninstall_log="$log_dir/service-uninstall.log"
running_doctor_log="$log_dir/doctor-running.log"
stopped_doctor_log="$log_dir/doctor-stopped.log"
serve_log="$log_dir/serve.log"
unit="$HOME/.config/systemd/user/axon.service"
probe_pid=""

assert_contains() {
    grep -F -- "$2" "$1" >/dev/null
}

cleanup() {
    if [ -n "$probe_pid" ]; then
        kill "$probe_pid" >/dev/null 2>&1 || true
        wait "$probe_pid" >/dev/null 2>&1 || true
    fi
}

trap cleanup EXIT

cat >/tmp/axon-stubs/systemctl <<'STUB'
#!/bin/sh
printf "%s\n" "$*" >>/tmp/systemctl.log
exit 0
STUB
chmod +x /tmp/axon-stubs/systemctl

/work/scripts/install.sh | tee "$install_log"
assert_contains "$install_log" "detected platform: linux/amd64"
assert_contains "$install_log" "installed axon to $HOME/.local/bin/axon"
assert_contains "$install_log" "created $HOME/.config/axon"
assert_contains "$install_log" "created $HOME/.local/share/axon"
assert_contains "$install_log" "version: axon 0.4.0"
test -x "$HOME/.local/bin/axon"
axon --version

axon doctor | tee "$pre_doctor_log"
assert_contains "$pre_doctor_log" "Config file: $HOME/.config/axon/config.toml"
assert_contains "$pre_doctor_log" "  exists: false"
assert_contains "$pre_doctor_log" "Data directory: $HOME/.local/share/axon"
assert_contains "$pre_doctor_log" "  exists: true"
assert_contains "$pre_doctor_log" "Storage backend: sqlite"
assert_contains "$pre_doctor_log" "HTTP port: 4170"
assert_contains "$pre_doctor_log" "gRPC: disabled"
assert_contains "$pre_doctor_log" "Server (http://localhost:4170): not reachable"

axon server install | tee "$service_install_log"
assert_contains "$service_install_log" "wrote $unit"
assert_contains "$service_install_log" "enabled axon.service (user)"
test -f "$unit"
grep -F "Description=Axon Data Store" "$unit"
grep -F "ExecStart=$HOME/.local/bin/axon serve --tls-self-signed" "$unit"
grep -F "StandardOutput=journal" "$unit"
grep -F "StandardError=journal" "$unit"
! grep -F -- "--no-auth" "$unit"
grep -F -- "--sqlite-path" "$unit"
grep -F -- "/axon.db" "$unit"
grep -F -- "--control-plane-path" "$unit"
grep -F -- "/axon-control-plane.db" "$unit"
grep -F "WantedBy=default.target" "$unit"
grep -F -- "--user daemon-reload" /tmp/systemctl.log
grep -F -- "--user enable axon" /tmp/systemctl.log

RUST_LOG=info axon serve --guest-role=read \
    --sqlite-path "$HOME/.local/share/axon/axon.db" \
    --control-plane-path "$HOME/.local/share/axon/axon-control-plane.db" \
    >"$serve_log" 2>&1 &
probe_pid=$!

for _ in 1 2 3 4 5 6 7 8 9 10; do
    if axon doctor | tee "$running_doctor_log" | grep -F "Server (http://localhost:4170): reachable" >/dev/null; then
        break
    fi
    sleep 1
done
assert_contains "$running_doctor_log" "Config file: $HOME/.config/axon/config.toml"
assert_contains "$running_doctor_log" "  exists: false"
assert_contains "$running_doctor_log" "Server (http://localhost:4170): reachable"
assert_contains "$serve_log" "control-plane database opened at $HOME/.local/share/axon/axon-control-plane.db"
assert_contains "$serve_log" "HTTP gateway listening on 0.0.0.0:4170"

kill "$probe_pid"
wait "$probe_pid" || true
probe_pid=""

axon doctor | tee "$stopped_doctor_log"
assert_contains "$stopped_doctor_log" "Server (http://localhost:4170): not reachable"

axon server start
axon server status
axon server stop
axon server restart
grep -F -- "--user start axon" /tmp/systemctl.log
grep -F -- "--user status axon" /tmp/systemctl.log
grep -F -- "--user stop axon" /tmp/systemctl.log
grep -F -- "--user restart axon" /tmp/systemctl.log

axon server uninstall | tee "$service_uninstall_log"
assert_contains "$service_uninstall_log" "removed $unit"
test ! -e "$unit"
grep -F -- "--user stop axon" /tmp/systemctl.log
grep -F -- "--user disable axon" /tmp/systemctl.log
grep -F -- "--user daemon-reload" /tmp/systemctl.log
SH
