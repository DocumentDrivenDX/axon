#!/usr/bin/env bash
# cutover-smoke.sh — end-to-end smoke test for the ADR-018 JWT cutover.
#
# Starts axon-server in --no-auth mode (no Tailscale required) with a
# known JWT secret, mints a token, and verifies:
#   1. No auth header → 201 (legacy path still works)
#   2. Valid JWT with write grant → 201
#   3. Valid JWT with read-only grant on POST → 403 op_not_granted
#   4. gRPC composite x-axon-tenant-database header is accepted
#
# Requirements: cargo, curl, jq, grpcurl (optional, for gRPC check).
#
# Usage:
#   ./scripts/cutover-smoke.sh
#
# Environment overrides:
#   AXON_HTTP_PORT   — HTTP port to use (default: 14170)
#   AXON_GRPC_PORT   — gRPC port to use (default: 14171)
#   AXON_JWT_KEY     — HMAC secret for JWT issuance (default: smoke-test-secret)

set -euo pipefail

AXON_HTTP_PORT=${AXON_HTTP_PORT:-14170}
AXON_GRPC_PORT=${AXON_GRPC_PORT:-14171}
AXON_JWT_KEY=${AXON_JWT_KEY:-smoke-test-secret-32bytes-xxxxx}
BASE_URL="http://127.0.0.1:${AXON_HTTP_PORT}"
TENANT="acme"
DATABASE="smoke"
COLLECTION="items"

# ── Colour helpers ────────────────────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'
pass() { echo -e "${GREEN}[PASS]${NC} $*"; }
fail() { echo -e "${RED}[FAIL]${NC} $*"; exit 1; }
info() { echo -e "${YELLOW}[INFO]${NC} $*"; }

# ── Build ─────────────────────────────────────────────────────────────────────
info "Building axon-server…"
cargo build -p axon-cli --quiet

AXON_BIN="./target/debug/axon"
if [[ ! -x "$AXON_BIN" ]]; then
    fail "Binary not found at $AXON_BIN after build"
fi

# ── Start server ──────────────────────────────────────────────────────────────
info "Starting axon-server on HTTP :${AXON_HTTP_PORT} gRPC :${AXON_GRPC_PORT}…"
AXON_JWT_KEY="${AXON_JWT_KEY}" \
AXON_NO_AUTH=true \
AXON_HTTP_PORT="${AXON_HTTP_PORT}" \
AXON_GRPC_PORT="${AXON_GRPC_PORT}" \
AXON_SQLITE_PATH=":memory:" \
"${AXON_BIN}" serve --storage memory &
SERVER_PID=$!

cleanup() {
    kill "${SERVER_PID}" 2>/dev/null || true
    wait "${SERVER_PID}" 2>/dev/null || true
}
trap cleanup EXIT

# Wait for the server to be ready.
for i in $(seq 1 20); do
    if curl -sf "${BASE_URL}/health" > /dev/null 2>&1; then
        info "Server ready after ${i} attempts"
        break
    fi
    sleep 0.3
    if [[ $i -eq 20 ]]; then
        fail "Server did not become ready in time"
    fi
done

# ── Mint a JWT ────────────────────────────────────────────────────────────────
# We need to produce a HS256 JWT. If python3 is available, use it; otherwise
# fall back to the /control/tenants/{id}/credentials endpoint (which requires
# a pre-provisioned tenant).
#
# Here we use a small python3 snippet to keep the script self-contained.
if ! command -v python3 &>/dev/null; then
    info "python3 not found — skipping JWT tests (only running no-auth test)"
    JWT_AVAILABLE=false
else
    JWT_AVAILABLE=true
    NOW=$(date +%s)
    EXP=$((NOW + 3600))

    mint_token() {
        local grants_json="$1"
        python3 - "$AXON_JWT_KEY" "$NOW" "$EXP" "${TENANT}" "u-smoke-01" "$grants_json" <<'PYEOF'
import sys, json, hmac, hashlib, base64

secret = sys.argv[1].encode()
now = int(sys.argv[2])
exp = int(sys.argv[3])
aud = sys.argv[4]
sub = sys.argv[5]
grants = json.loads(sys.argv[6])

def b64url(data: bytes) -> str:
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode()

header = b64url(json.dumps({"alg":"HS256","typ":"JWT"}).encode())
payload = b64url(json.dumps({
    "iss": "axon-server",
    "sub": sub,
    "aud": aud,
    "jti": "smoke-jti-01",
    "iat": now,
    "nbf": now,
    "exp": exp,
    "grants": grants,
}).encode())

sig_input = f"{header}.{payload}".encode()
sig = b64url(hmac.new(secret, sig_input, hashlib.sha256).digest())
print(f"{header}.{payload}.{sig}")
PYEOF
    }

    WRITE_GRANTS='{"databases":[{"name":"smoke","ops":["read","write"]}]}'
    READ_GRANTS='{"databases":[{"name":"smoke","ops":["read"]}]}'
    WRITE_TOKEN=$(mint_token "$WRITE_GRANTS")
    READ_TOKEN=$(mint_token "$READ_GRANTS")
fi

# ── Create the collection (needed before entity writes) ───────────────────────
info "Creating collection ${COLLECTION} in ${TENANT}/${DATABASE}…"
curl -sf -X POST \
    "${BASE_URL}/tenants/${TENANT}/databases/${DATABASE}/collections/${COLLECTION}" \
    -H 'Content-Type: application/json' \
    -d '{}' > /dev/null

# ── Test 1: No auth header → 201 ─────────────────────────────────────────────
info "Test 1: No auth header → legacy no-auth → 201"
STATUS=$(curl -so /dev/null -w "%{http_code}" -X POST \
    "${BASE_URL}/tenants/${TENANT}/databases/${DATABASE}/entities/${COLLECTION}/e-smoke-noauth" \
    -H 'Content-Type: application/json' \
    -d '{"data":{"x":1}}')
[[ "$STATUS" == "201" ]] && pass "No-auth entity create → ${STATUS}" || fail "Expected 201, got ${STATUS}"

# ── Test 2: Valid JWT write grant → 201 ───────────────────────────────────────
if [[ "$JWT_AVAILABLE" == "true" ]]; then
    info "Test 2: Valid JWT (write grant) → 201"
    STATUS=$(curl -so /dev/null -w "%{http_code}" -X POST \
        "${BASE_URL}/tenants/${TENANT}/databases/${DATABASE}/entities/${COLLECTION}/e-smoke-jwt" \
        -H 'Content-Type: application/json' \
        -H "Authorization: Bearer ${WRITE_TOKEN}" \
        -d '{"data":{"x":2}}')
    [[ "$STATUS" == "201" ]] && pass "JWT write → ${STATUS}" || fail "Expected 201, got ${STATUS}"
else
    info "Test 2: skipped (python3 not available)"
fi

# ── Test 3: JWT read-only grant → 403 ─────────────────────────────────────────
if [[ "$JWT_AVAILABLE" == "true" ]]; then
    info "Test 3: JWT read-only grant on POST → 403"
    BODY=$(curl -s -X POST \
        "${BASE_URL}/tenants/${TENANT}/databases/${DATABASE}/entities/${COLLECTION}/e-smoke-ro" \
        -H 'Content-Type: application/json' \
        -H "Authorization: Bearer ${READ_TOKEN}" \
        -d '{"data":{"x":3}}')
    CODE=$(echo "$BODY" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('error',{}).get('code',''))" 2>/dev/null || echo "")
    [[ "$CODE" == "op_not_granted" ]] && pass "JWT read-only → op_not_granted" || fail "Expected op_not_granted, got: ${BODY}"
else
    info "Test 3: skipped (python3 not available)"
fi

# ── Test 4: gRPC composite header ─────────────────────────────────────────────
if command -v grpcurl &>/dev/null; then
    info "Test 4: gRPC composite x-axon-tenant-database header"
    GRPC_RESP=$(grpcurl \
        -plaintext \
        -H "x-axon-tenant-database: ${TENANT}:${DATABASE}" \
        -d '{"collection":"items","id":"grpc-smoke-01","data_json":"{\"x\":4}","actor":"smoke"}' \
        "127.0.0.1:${AXON_GRPC_PORT}" axon.v1.AxonService/CreateEntity 2>&1)
    echo "$GRPC_RESP" | grep -q '"id": "grpc-smoke-01"' && \
        pass "gRPC composite header → entity created" || \
        fail "gRPC composite header failed: ${GRPC_RESP}"
else
    info "Test 4: skipped (grpcurl not installed)"
fi

echo ""
pass "All cutover smoke tests passed."
