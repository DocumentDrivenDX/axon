#!/usr/bin/env bash
# Axon demo — full CLI lifecycle walkthrough
#
# Run inside the axon Docker container:
#   docker run --rm --entrypoint bash axon:demo /scripts/demo.sh
#
# Or locally (requires axon in PATH):
#   bash scripts/demo.sh
set -euo pipefail

# ── helpers ───────────────────────────────────────────────────────────────────

BOLD='\033[1m'
CYAN='\033[0;36m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
RESET='\033[0m'

section() { echo; echo -e "${BOLD}${CYAN}━━━ $* ━━━${RESET}"; }

cmd() {
    local display="axon"
    for arg in "$@"; do
        if [[ "$arg" == *" "* || "$arg" == *$'\n'* || "$arg" == *"{"* ]]; then
            display+=" '${arg}'"
        else
            display+=" ${arg}"
        fi
    done
    echo -e "${GREEN}\$ ${display}${RESET}"
    axon "$@"
}

# ─────────────────────────────────────────────────────────────────────────────

section "0 · Start server (in-process background)"

# Launch axon server in the background with in-memory storage.
axon serve --no-auth --storage memory --http-port 4170 \
    --control-plane-path /tmp/axon-control-plane.db &
SERVER_PID=$!
trap 'kill "$SERVER_PID" 2>/dev/null || true' EXIT

# Wait for the server to be ready (up to 10s).
echo -e "${YELLOW}Waiting for server on :4170…${RESET}"
for i in $(seq 1 20); do
    if curl -sf http://localhost:4170/health >/dev/null 2>&1; then
        echo -e "${GREEN}Server ready.${RESET}"
        break
    fi
    sleep 0.5
    if [[ $i -eq 20 ]]; then
        echo "Server did not start in time." >&2; exit 1
    fi
done

# ─────────────────────────────────────────────────────────────────────────────

section "1 · Doctor (server reachable)"
cmd doctor

section "2 · Create collections"
cmd collections create tasks
cmd collections create projects
cmd collections list

section "3 · Define entity schema"
cmd schema set tasks --schema \
    '{"type":"object","properties":{"title":{"type":"string"},"status":{"type":"string","enum":["open","in-progress","done"]},"priority":{"type":"integer"}},"required":["title","status"]}'
cmd schema show tasks

section "4 · Create entities"
cmd entities create tasks \
    --id task-001 \
    --data '{"title":"Set up database","status":"done","priority":1}'
cmd entities create tasks \
    --id task-002 \
    --data '{"title":"Build REST API","status":"in-progress","priority":2}'
cmd entities create tasks \
    --id task-003 \
    --data '{"title":"Write tests","status":"open","priority":3}'
cmd entities create projects \
    --id proj-axon \
    --data '{"name":"Axon","phase":"alpha"}'

section "5 · List and get entities"
cmd entities list tasks
cmd entities get tasks task-001

section "6 · Update entity (version auto-fetched)"
cmd entities update tasks task-002 \
    --data '{"title":"Build REST API","status":"done","priority":2}'

section "7 · Query with filter"
cmd entities query tasks --filter status=open

section "8 · Set links"
cmd links set tasks task-001 projects proj-axon --type belongs-to
cmd links set tasks task-002 projects proj-axon --type belongs-to
cmd links set tasks task-003 projects proj-axon --type belongs-to
cmd links set tasks task-002 tasks task-001 --type depends-on

section "9 · List outbound links from task-002"
cmd links list tasks task-002

section "10 · Graph traversal (depth 2 from task-002)"
cmd graph tasks task-002 --depth 2

section "11 · Audit log"
cmd audit list --collection tasks --limit 5

section "12 · Schema evolution — add assignee field (compatible)"
cmd schema set tasks --schema \
    '{"type":"object","properties":{"title":{"type":"string"},"status":{"type":"string","enum":["open","in-progress","done"]},"priority":{"type":"integer"},"assignee":{"type":"string"}},"required":["title","status"]}'
cmd schema show tasks

section "13 · Schema evolution — remove required field (breaking, needs --force)"
echo -e "${YELLOW}Attempting breaking change without --force (expect error):${RESET}"
cmd schema set tasks --schema \
    '{"type":"object","properties":{"title":{"type":"string"},"priority":{"type":"integer"}},"required":["title"]}' \
    || true
echo
echo -e "${YELLOW}Same change with --force:${RESET}"
cmd schema set tasks --schema \
    '{"type":"object","properties":{"title":{"type":"string"},"priority":{"type":"integer"}},"required":["title"]}' \
    --force

section "14 · Drop collection"
cmd collections drop tasks --confirm
cmd collections list

echo
echo -e "${BOLD}${GREEN}✓ Demo complete.${RESET}"
