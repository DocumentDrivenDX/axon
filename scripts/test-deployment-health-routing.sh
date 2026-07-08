#!/usr/bin/env bash
# test-deployment-health-routing.sh — reproducible proof for the four
# deployment-checklist verification rows that were previously prose-only:
# health, tenant routing, configured auth mode, and TLS behavior.
#
# Starts a single axon-serve instance over self-signed HTTPS in explicit
# guest(admin) auth mode (no Tailscale daemon required) and verifies:
#   1. GET /health over HTTPS returns 2xx with the expected fields.
#   2. The self-signed TLS certificate is bootstrapped and the handshake
#      verifies against it (no -k / insecure skip).
#   3. A plain-HTTP request against the HTTPS-only listener fails, instead
#      of being silently accepted.
#   4. The resolved auth mode is the explicit guest(admin) mode, not a
#      silent --no-auth fallback.
#   5. Data-plane writes to two distinct (tenant, database) pairs are
#      isolated from each other (tenant routing).
#   6. A malformed (tenant, database) path segment is rejected with 404,
#      not silently routed to the default/master database.
#
# Usage:
#   ./scripts/test-deployment-health-routing.sh
#
# Environment overrides:
#   AXON_HTTP_PORT — HTTPS port to use (default: 14173)

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HTTP_PORT="${AXON_HTTP_PORT:-14173}"
BASE_URL="https://127.0.0.1:${HTTP_PORT}"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'
pass() { echo -e "${GREEN}[PASS]${NC} $*"; }
fail() { echo -e "${RED}[FAIL]${NC} $*"; exit 1; }
info() { echo -e "${YELLOW}[INFO]${NC} $*"; }

info "Building axon-cli…"
cargo build -p axon-cli --quiet
AXON_BIN="$ROOT/target/debug/axon"
[[ -x "$AXON_BIN" ]] || fail "Binary not found at $AXON_BIN after build"

WORKDIR="$(mktemp -d)"
CERT="$WORKDIR/tls/cert.pem"
KEY="$WORKDIR/tls/key.pem"
SERVE_LOG="$WORKDIR/serve.log"
SERVER_PID=""

cleanup() {
    if [[ -n "$SERVER_PID" ]]; then
        kill "$SERVER_PID" >/dev/null 2>&1 || true
        wait "$SERVER_PID" >/dev/null 2>&1 || true
    fi
    rm -rf "$WORKDIR"
}
trap cleanup EXIT

info "Starting axon-serve on HTTPS :${HTTP_PORT} (guest-role=admin, self-signed TLS)…"
RUST_LOG=info "$AXON_BIN" serve \
    --guest-role=admin \
    --storage sqlite \
    --sqlite-path "$WORKDIR/axon.db" \
    --control-plane-path "$WORKDIR/axon-control-plane.db" \
    --http-port "$HTTP_PORT" \
    --tls-self-signed \
    --tls-cert "$CERT" \
    --tls-key "$KEY" \
    --tls-self-signed-san "127.0.0.1" \
    >"$SERVE_LOG" 2>&1 &
SERVER_PID=$!

READY=false
for i in $(seq 1 30); do
    if [[ -f "$CERT" ]] && curl -sS --cacert "$CERT" --max-time 1 "${BASE_URL}/health" >/dev/null 2>&1; then
        info "Server ready after ${i} attempt(s)"
        READY=true
        break
    fi
    sleep 0.3
done
if [[ "$READY" != "true" ]]; then
    cat "$SERVE_LOG" >&2
    fail "Server did not become ready in time"
fi

# ── Test 1 & 2: HTTPS health endpoint, verified against the bootstrapped cert ─
info "Test 1: GET /health over verified HTTPS returns 2xx with expected fields"
HEALTH_BODY_AND_STATUS=$(curl -sS --cacert "$CERT" -w '\n%{http_code}' "${BASE_URL}/health")
HEALTH_STATUS=$(echo "$HEALTH_BODY_AND_STATUS" | tail -n1)
HEALTH_BODY=$(echo "$HEALTH_BODY_AND_STATUS" | sed '$d')
[[ "$HEALTH_STATUS" == "200" ]] && pass "GET /health (TLS-verified) -> 200" || fail "Expected 200, got ${HEALTH_STATUS}: ${HEALTH_BODY}"
echo "$HEALTH_BODY" | grep -Eq '"status"[[:space:]]*:[[:space:]]*"ok"' || fail "Health response missing status=ok: ${HEALTH_BODY}"
echo "$HEALTH_BODY" | grep -q '"backing_store"' || fail "Health response missing backing_store: ${HEALTH_BODY}"
echo "$HEALTH_BODY" | grep -q "\"backend\":\"sqlite:$WORKDIR/axon.db\"" || fail "Health response backing_store.backend does not match configured sqlite path: ${HEALTH_BODY}"

info "Test 2: self-signed certificate was bootstrapped and the handshake verifies"
[[ -f "$CERT" && -f "$KEY" ]] && pass "Self-signed cert/key bootstrapped at $WORKDIR/tls/" || fail "Expected TLS cert/key at $WORKDIR/tls/"
grep -F "HTTPS gateway listening on" "$SERVE_LOG" >/dev/null && pass "Server log confirms HTTPS listener (not plain HTTP)" || fail "Server log missing HTTPS listener confirmation"

info "Test 3: plain HTTP against the HTTPS-only listener is refused, not silently served"
if curl -sf --max-time 2 -o /dev/null "http://127.0.0.1:${HTTP_PORT}/health" 2>/dev/null; then
    fail "Expected plain HTTP request against the TLS listener to fail, but it succeeded"
else
    pass "Plain HTTP request against the TLS listener correctly failed"
fi

# ── Test 4: configured auth mode ─────────────────────────────────────────────
info "Test 4: resolved auth mode is the explicit guest(admin) mode, not silent no-auth"
grep -F "running in guest mode: unauthenticated requests get role=Admin (actor=guest)" "$SERVE_LOG" >/dev/null \
    && pass "Server log confirms explicit guest(admin) auth mode" \
    || fail "Server log missing explicit guest-mode auth confirmation: $(cat "$SERVE_LOG")"
grep -F -- "--no-auth" "$SERVE_LOG" >/dev/null \
    && fail "Server log unexpectedly reports --no-auth mode (should be explicit guest mode)" \
    || pass "No accidental --no-auth fallback in server log"

# ── Tests 5 & 6: tenant routing ───────────────────────────────────────────────
create_collection() {
    curl -sS --cacert "$CERT" -o /dev/null -w '%{http_code}' -X POST \
        "${BASE_URL}/tenants/$1/databases/$2/collections/items" \
        -H 'Content-Type: application/json' \
        -d '{"schema":{}}'
}
write_entity() {
    curl -sS --cacert "$CERT" -o /dev/null -w '%{http_code}' -X POST \
        "${BASE_URL}/tenants/$1/databases/$2/entities/items/e-route-test" \
        -H 'Content-Type: application/json' \
        -d "{\"data\":{\"marker\":\"$3\"}}"
}
read_entity() {
    curl -sS --cacert "$CERT" "${BASE_URL}/tenants/$1/databases/$2/entities/items/e-route-test"
}

info "Test 5: data-plane writes to distinct (tenant, database) pairs are isolated"
STATUS=$(create_collection acme north)
[[ "$STATUS" == "201" || "$STATUS" == "200" ]] || fail "collection create in acme/north -> ${STATUS}"
STATUS=$(create_collection acme south)
[[ "$STATUS" == "201" || "$STATUS" == "200" ]] || fail "collection create in acme/south -> ${STATUS}"

STATUS=$(write_entity acme north north-marker)
[[ "$STATUS" == "201" ]] || fail "entity write to acme/north -> ${STATUS}"
STATUS=$(write_entity acme south south-marker)
[[ "$STATUS" == "201" ]] || fail "entity write to acme/south -> ${STATUS}"

NORTH_BODY=$(read_entity acme north)
SOUTH_BODY=$(read_entity acme south)
echo "$NORTH_BODY" | grep -q "north-marker" || fail "acme/north entity missing its own marker: ${NORTH_BODY}"
echo "$SOUTH_BODY" | grep -q "south-marker" || fail "acme/south entity missing its own marker: ${SOUTH_BODY}"
if echo "$NORTH_BODY" | grep -q "south-marker"; then
    fail "acme/north entity leaked the acme/south marker — cross-tenant routing bug: ${NORTH_BODY}"
fi
pass "Writes to acme/north and acme/south route to isolated per-tenant databases"

info "Test 6: malformed database segment is rejected with 404, not routed to default"
BODY_AND_STATUS=$(curl -sS --cacert "$CERT" -w '\n%{http_code}' "${BASE_URL}/tenants/acme/databases/1bad/entities/items/e-route-test")
STATUS=$(echo "$BODY_AND_STATUS" | tail -n1)
BODY=$(echo "$BODY_AND_STATUS" | sed '$d')
[[ "$STATUS" == "404" ]] && pass "Malformed database segment -> 404 (not routed to default/master)" || fail "Expected 404, got ${STATUS}: ${BODY}"
echo "$BODY" | grep -q "invalid tenant or database" || fail "Expected explicit invalid-tenant-or-database error, got: ${BODY}"

echo ""
pass "All deployment health/tenant-routing/auth/TLS proof checks passed."
