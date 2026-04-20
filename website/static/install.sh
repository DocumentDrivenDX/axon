#!/bin/sh
set -eu

REPO="${AXON_INSTALL_REPO:-DocumentDrivenDX/axon}"
INSTALL_DIR="${AXON_INSTALL_DIR:-$HOME/.local/bin}"
CONFIG_DIR="${AXON_CONFIG_DIR:-${XDG_CONFIG_HOME:-$HOME/.config}/axon}"
DATA_DIR="${AXON_DATA_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/axon}"
TMPFILE=""

cleanup() {
    if [ -n "$TMPFILE" ] && [ -f "$TMPFILE" ]; then
        rm -f "$TMPFILE"
    fi
}

trap cleanup EXIT

err() {
    printf "error: %s\n" "$1" >&2
    exit 1
}

detect_platform() {
    OS="$(uname -s)"
    ARCH="$(uname -m)"

    case "$OS" in
        Linux)  OS="linux" ;;
        Darwin) OS="darwin" ;;
        *)      err "unsupported operating system: $OS (expected Linux or Darwin)" ;;
    esac

    case "$ARCH" in
        x86_64)         ARCH="amd64" ;;
        aarch64|arm64)  ARCH="arm64" ;;
        *)              err "unsupported architecture: $ARCH (expected x86_64, aarch64, or arm64)" ;;
    esac

    ARTIFACT="axon-${OS}-${ARCH}"
    if [ -n "${AXON_INSTALL_URL:-}" ]; then
        URL="$AXON_INSTALL_URL"
    elif [ -n "${AXON_INSTALL_VERSION:-}" ]; then
        URL="https://github.com/${REPO}/releases/download/${AXON_INSTALL_VERSION}/${ARTIFACT}"
    else
        URL="https://github.com/${REPO}/releases/latest/download/${ARTIFACT}"
    fi
    printf "detected platform: %s/%s\n" "$OS" "$ARCH"
}

download_binary() {
    TMPFILE="$(mktemp)"

    printf "downloading %s ...\n" "$URL"

    if command -v curl >/dev/null 2>&1; then
        if ! curl -fsSL -o "$TMPFILE" "$URL"; then
            err "download failed — check that a release exists for ${ARTIFACT}"
        fi
    elif command -v wget >/dev/null 2>&1; then
        if ! wget -q -O "$TMPFILE" "$URL"; then
            err "download failed — check that a release exists for ${ARTIFACT}"
        fi
    else
        err "neither curl nor wget found — install one and try again"
    fi
}

install_binary() {
    mkdir -p "$INSTALL_DIR"
    mv "$TMPFILE" "${INSTALL_DIR}/axon"
    chmod +x "${INSTALL_DIR}/axon"
    TMPFILE=""
    printf "installed axon to %s/axon\n" "$INSTALL_DIR"
}

create_dirs() {
    mkdir -p "$CONFIG_DIR"
    mkdir -p "$DATA_DIR"
    printf "created %s\n" "$CONFIG_DIR"
    printf "created %s\n" "$DATA_DIR"
}

check_path() {
    case ":${PATH}:" in
        *":${INSTALL_DIR}:"*) ;;
        *)
            printf "\nwarning: %s is not in your PATH\n" "$INSTALL_DIR"
            printf "add it by appending this line to your shell profile:\n"
            printf "\n  export PATH=\"%s:\$PATH\"\n\n" "$INSTALL_DIR"
            ;;
    esac
}

print_success() {
    printf "\naxon installed successfully!\n"
    if command -v axon >/dev/null 2>&1; then
        printf "version: %s\n" "$(axon --version)"
    elif [ -x "${INSTALL_DIR}/axon" ]; then
        printf "version: %s\n" "$("${INSTALL_DIR}/axon" --version 2>/dev/null || echo "unknown")"
    fi
}

main() {
    detect_platform
    download_binary
    install_binary
    create_dirs
    check_path
    print_success
}

main "$@"
