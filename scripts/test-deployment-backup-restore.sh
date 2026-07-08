#!/usr/bin/env bash
# test-deployment-backup-restore.sh — reproducible proof that the backup and
# restore procedure documented in the runbook ("Back up control-plane +
# tenant data") and deployment checklist ("Back up existing control-plane DB
# and tenants/ directory before upgrading") actually recovers data, schema,
# and health for both supported storage backends:
#
#   SQLite:
#     1. Start axon serve --storage sqlite, write a tenant entity.
#     2. Stop the service and back up the control-plane DB + tenants/ dir
#        exactly as the runbook prescribes.
#     3. Destroy the live control-plane DB + tenants/ dir (simulated disk
#        loss).
#     4. Restore the backup, restart the service, and prove the entity,
#        schema, and health endpoint all recover.
#
#   PostgreSQL:
#     1. Start a real PostgreSQL container, then axon serve
#        --storage postgres, and write a tenant entity (which provisions a
#        physical `axon_<db>` PostgreSQL database).
#     2. Stop axon and take a real `pg_dump` backup of that database plus a
#        copy of the (always-SQLite) control-plane DB.
#     3. Drop the PostgreSQL database entirely (simulated data loss).
#     4. Restore via `psql` from the dump, restart axon, and prove the
#        entity, schema, and health endpoint all recover.
#
# Usage:
#   ./scripts/test-deployment-backup-restore.sh
#
# Environment overrides:
#   AXON_HTTP_PORT_SQLITE   — HTTP port for the SQLite phase (default: 14180)
#   AXON_HTTP_PORT_POSTGRES — HTTP port for the PostgreSQL phase (default: 14181)
#   AXON_POSTGRES_PORT      — host port for the PostgreSQL container (default: 15532)

set -euo pipefail

if ! command -v docker >/dev/null 2>&1; then
    echo "error: docker is required for the PostgreSQL backup/restore proof" >&2
    exit 1
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HTTP_PORT_SQLITE="${AXON_HTTP_PORT_SQLITE:-14180}"
HTTP_PORT_POSTGRES="${AXON_HTTP_PORT_POSTGRES:-14181}"
PG_PORT="${AXON_POSTGRES_PORT:-15532}"

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

wait_for_health() {
    local base_url="$1" log="$2"
    for _ in $(seq 1 30); do
        if curl -sS --max-time 1 "${base_url}/health" >/dev/null 2>&1; then
            return 0
        fi
        sleep 0.3
    done
    cat "$log" >&2
    fail "Server at ${base_url} did not become ready in time"
}

write_and_verify_entity() {
    local base_url="$1" marker="$2"
    local status
    status=$(curl -sS -o /dev/null -w '%{http_code}' -X POST \
        "${base_url}/tenants/acme/databases/proof/collections/items" \
        -H 'Content-Type: application/json' -d '{"schema":{}}')
    [[ "$status" == "201" || "$status" == "200" ]] || fail "collection create -> ${status}"

    status=$(curl -sS -o /dev/null -w '%{http_code}' -X POST \
        "${base_url}/tenants/acme/databases/proof/entities/items/e-backup-test" \
        -H 'Content-Type: application/json' -d "{\"data\":{\"marker\":\"${marker}\"}}")
    [[ "$status" == "201" ]] || fail "entity write -> ${status}"
}

read_entity_body() {
    local base_url="$1"
    curl -sS "${base_url}/tenants/acme/databases/proof/entities/items/e-backup-test"
}

stop_server() {
    local pid="$1"
    kill "$pid" >/dev/null 2>&1 || true
    wait "$pid" >/dev/null 2>&1 || true
}

# ============================================================================
# Phase 1: SQLite backend
# ============================================================================
info "=== Phase 1: SQLite backup and restore ==="

SQLITE_WORKDIR="$(mktemp -d)"
CONTROL_PLANE_PATH="$SQLITE_WORKDIR/axon-control-plane.db"
SQLITE_DEFAULT_PATH="$SQLITE_WORKDIR/axon.db"
SQLITE_LOG="$SQLITE_WORKDIR/serve.log"
BASE_URL_SQLITE="http://127.0.0.1:${HTTP_PORT_SQLITE}"
SQLITE_PID=""

sqlite_cleanup() {
    [[ -n "$SQLITE_PID" ]] && stop_server "$SQLITE_PID"
    rm -rf "$SQLITE_WORKDIR"
}
trap sqlite_cleanup EXIT

info "Starting axon-serve (sqlite) on :${HTTP_PORT_SQLITE}…"
RUST_LOG=info "$AXON_BIN" serve \
    --no-auth \
    --storage sqlite \
    --sqlite-path "$SQLITE_DEFAULT_PATH" \
    --control-plane-path "$CONTROL_PLANE_PATH" \
    --http-port "$HTTP_PORT_SQLITE" \
    >"$SQLITE_LOG" 2>&1 &
SQLITE_PID=$!
wait_for_health "$BASE_URL_SQLITE" "$SQLITE_LOG"

info "Writing tenant data (acme/proof) before backup…"
write_and_verify_entity "$BASE_URL_SQLITE" "before-sqlite-backup"
BODY=$(read_entity_body "$BASE_URL_SQLITE")
echo "$BODY" | grep -q "before-sqlite-backup" || fail "entity not readable before backup: ${BODY}"
[[ -f "$SQLITE_WORKDIR/tenants/acme:proof.db" ]] || fail "expected per-tenant store at tenants/acme:proof.db"
pass "Tenant entity written to per-tenant SQLite store"

info "Stopping server before taking backup (consistent snapshot)…"
stop_server "$SQLITE_PID"
SQLITE_PID=""

info "Backing up control-plane DB and tenants/ directory (runbook procedure)…"
BACKUP_DIR="$SQLITE_WORKDIR/backup"
mkdir -p "$BACKUP_DIR"
cp -p "$CONTROL_PLANE_PATH" "$BACKUP_DIR/axon-control-plane.db"
cp -pr "$SQLITE_WORKDIR/tenants" "$BACKUP_DIR/tenants"
pass "Backup captured at ${BACKUP_DIR}"

info "Simulating data loss: deleting the live control-plane DB and tenants/ dir…"
rm -f "$CONTROL_PLANE_PATH"
rm -rf "$SQLITE_WORKDIR/tenants"
[[ ! -e "$CONTROL_PLANE_PATH" && ! -e "$SQLITE_WORKDIR/tenants" ]] || fail "simulated data loss did not remove live files"

info "Restoring from backup…"
cp -p "$BACKUP_DIR/axon-control-plane.db" "$CONTROL_PLANE_PATH"
cp -pr "$BACKUP_DIR/tenants" "$SQLITE_WORKDIR/tenants"

info "Restarting axon-serve (sqlite) after restore…"
RUST_LOG=info "$AXON_BIN" serve \
    --no-auth \
    --storage sqlite \
    --sqlite-path "$SQLITE_DEFAULT_PATH" \
    --control-plane-path "$CONTROL_PLANE_PATH" \
    --http-port "$HTTP_PORT_SQLITE" \
    >"$SQLITE_LOG" 2>&1 &
SQLITE_PID=$!
wait_for_health "$BASE_URL_SQLITE" "$SQLITE_LOG"

HEALTH_BODY=$(curl -sS "${BASE_URL_SQLITE}/health")
echo "$HEALTH_BODY" | grep -Eq '"status"[[:space:]]*:[[:space:]]*"ok"' || fail "restored server health not ok: ${HEALTH_BODY}"
pass "Restored server reports healthy"

BODY=$(read_entity_body "$BASE_URL_SQLITE")
echo "$BODY" | grep -q "before-sqlite-backup" || fail "entity missing after restore: ${BODY}"
pass "Tenant entity, schema, and data recovered after SQLite restore"

stop_server "$SQLITE_PID"
SQLITE_PID=""
rm -rf "$SQLITE_WORKDIR"
trap - EXIT

# ============================================================================
# Phase 2: PostgreSQL backend
# ============================================================================
info "=== Phase 2: PostgreSQL backup and restore ==="

PG_WORKDIR="$(mktemp -d)"
PG_CONTAINER="axon-backup-restore-pg-$$"
PG_CONTROL_PLANE_PATH="$PG_WORKDIR/axon-control-plane.db"
PG_LOG="$PG_WORKDIR/serve.log"
PG_DSN="postgresql://axon:axon@127.0.0.1:${PG_PORT}/postgres"
BASE_URL_POSTGRES="http://127.0.0.1:${HTTP_PORT_POSTGRES}"
AXON_PG_PID=""

pg_cleanup() {
    [[ -n "$AXON_PG_PID" ]] && stop_server "$AXON_PG_PID"
    docker rm -f "$PG_CONTAINER" >/dev/null 2>&1 || true
    rm -rf "$PG_WORKDIR"
}
trap pg_cleanup EXIT

info "Starting PostgreSQL container on 127.0.0.1:${PG_PORT}…"
docker run -d \
    --name "$PG_CONTAINER" \
    -p "127.0.0.1:${PG_PORT}:5432" \
    -e POSTGRES_USER=axon \
    -e POSTGRES_PASSWORD=axon \
    -e POSTGRES_DB=postgres \
    postgres:16-alpine >/dev/null

READY=false
for _ in $(seq 1 60); do
    if docker exec "$PG_CONTAINER" pg_isready -U axon -d postgres >/dev/null 2>&1; then
        READY=true
        break
    fi
    sleep 1
done
[[ "$READY" == "true" ]] || { docker logs "$PG_CONTAINER" >&2 || true; fail "PostgreSQL container did not become healthy"; }
pass "PostgreSQL container ready"

info "Starting axon-serve (postgres) on :${HTTP_PORT_POSTGRES}…"
RUST_LOG=info "$AXON_BIN" serve \
    --no-auth \
    --storage postgres \
    --postgres-dsn "$PG_DSN" \
    --control-plane-path "$PG_CONTROL_PLANE_PATH" \
    --http-port "$HTTP_PORT_POSTGRES" \
    >"$PG_LOG" 2>&1 &
AXON_PG_PID=$!
wait_for_health "$BASE_URL_POSTGRES" "$PG_LOG"

info "Writing tenant data (acme/proof) before backup…"
write_and_verify_entity "$BASE_URL_POSTGRES" "before-postgres-backup"
BODY=$(read_entity_body "$BASE_URL_POSTGRES")
echo "$BODY" | grep -q "before-postgres-backup" || fail "entity not readable before backup: ${BODY}"

# The physical database name for a (tenant, database) pair is a sanitized,
# hash-suffixed identifier (see tenant_router::postgres_database_key) — not a
# literal "axon_proof". Discover it rather than assuming a name.
TENANT_DB=$(docker exec "$PG_CONTAINER" psql -U axon -d postgres -tAc \
    "SELECT datname FROM pg_database WHERE datname LIKE 'axon\_%' AND datname <> 'axon_master'" | tr -d '[:space:]')
[[ -n "$TENANT_DB" ]] || fail "expected a provisioned PostgreSQL database for acme/proof"
pass "Tenant entity written to provisioned PostgreSQL database ${TENANT_DB}"

info "Stopping server before taking backup (consistent snapshot)…"
stop_server "$AXON_PG_PID"
AXON_PG_PID=""

info "Backing up control-plane DB (SQLite) and the ${TENANT_DB} PostgreSQL database (pg_dump)…"
PG_BACKUP_DIR="$PG_WORKDIR/backup"
mkdir -p "$PG_BACKUP_DIR"
cp -p "$PG_CONTROL_PLANE_PATH" "$PG_BACKUP_DIR/axon-control-plane.db"
docker exec "$PG_CONTAINER" pg_dump -U axon -d "$TENANT_DB" --clean --if-exists \
    >"$PG_BACKUP_DIR/tenant.sql"
[[ -s "$PG_BACKUP_DIR/tenant.sql" ]] || fail "pg_dump produced an empty backup"
pass "Backup captured at ${PG_BACKUP_DIR}"

info "Simulating data loss: dropping the live ${TENANT_DB} database and control-plane DB…"
docker exec "$PG_CONTAINER" dropdb -U axon "$TENANT_DB"
rm -f "$PG_CONTROL_PLANE_PATH"
docker exec "$PG_CONTAINER" psql -U axon -d postgres -tAc \
    "SELECT 1 FROM pg_database WHERE datname='${TENANT_DB}'" | grep -q 1 \
    && fail "simulated data loss did not drop ${TENANT_DB}" || true

info "Restoring from backup (recreate database, psql replay, restore control-plane DB)…"
cp -p "$PG_BACKUP_DIR/axon-control-plane.db" "$PG_CONTROL_PLANE_PATH"
docker exec "$PG_CONTAINER" createdb -U axon "$TENANT_DB"
docker exec -i "$PG_CONTAINER" psql -U axon -d "$TENANT_DB" <"$PG_BACKUP_DIR/tenant.sql" >/dev/null

info "Restarting axon-serve (postgres) after restore…"
RUST_LOG=info "$AXON_BIN" serve \
    --no-auth \
    --storage postgres \
    --postgres-dsn "$PG_DSN" \
    --control-plane-path "$PG_CONTROL_PLANE_PATH" \
    --http-port "$HTTP_PORT_POSTGRES" \
    >"$PG_LOG" 2>&1 &
AXON_PG_PID=$!
wait_for_health "$BASE_URL_POSTGRES" "$PG_LOG"

HEALTH_BODY=$(curl -sS "${BASE_URL_POSTGRES}/health")
echo "$HEALTH_BODY" | grep -Eq '"status"[[:space:]]*:[[:space:]]*"ok"' || fail "restored server health not ok: ${HEALTH_BODY}"
echo "$HEALTH_BODY" | grep -q '"backend":"postgres"' || fail "restored server not reporting postgres backend: ${HEALTH_BODY}"
pass "Restored server reports healthy on the postgres backend"

BODY=$(read_entity_body "$BASE_URL_POSTGRES")
echo "$BODY" | grep -q "before-postgres-backup" || fail "entity missing after restore: ${BODY}"
pass "Tenant entity, schema, and data recovered after PostgreSQL restore"

echo ""
pass "All backup/restore proof checks passed for SQLite and PostgreSQL."
