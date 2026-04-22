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
    bash -euxo pipefail -c '
        apt-get update
        apt-get install -y --no-install-recommends ca-certificates curl libsqlite3-0
        rm -rf /var/lib/apt/lists/*

        export HOME=/tmp/axon-home
        export PATH=/tmp/axon-stubs:$HOME/.local/bin:$PATH
        mkdir -p /tmp/axon-stubs "$HOME"

        cat >/tmp/axon-stubs/systemctl <<'"'"'SH'"'"'
#!/bin/sh
printf "%s\n" "$*" >>/tmp/systemctl.log
exit 0
SH
        chmod +x /tmp/axon-stubs/systemctl

        /work/scripts/install.sh
        test -x "$HOME/.local/bin/axon"
        axon --version

        axon server install
        unit="$HOME/.config/systemd/user/axon.service"
        test -f "$unit"
        grep -F "Description=Axon Data Store" "$unit"
        grep -F "ExecStart=$HOME/.local/bin/axon serve --no-auth --tls-self-signed --sqlite-path $HOME/.local/share/axon/axon.db --control-plane-path $HOME/.local/share/axon/axon-control-plane.db" "$unit"
        grep -F "WantedBy=default.target" "$unit"
        grep -F -- "--user daemon-reload" /tmp/systemctl.log
        grep -F -- "--user enable axon" /tmp/systemctl.log

        axon server start
        axon server status
        axon server stop
        axon server restart
        grep -F -- "--user start axon" /tmp/systemctl.log
        grep -F -- "--user status axon" /tmp/systemctl.log
        grep -F -- "--user stop axon" /tmp/systemctl.log
        grep -F -- "--user restart axon" /tmp/systemctl.log

        axon server uninstall
        test ! -e "$unit"
        grep -F -- "--user disable axon" /tmp/systemctl.log
    '
