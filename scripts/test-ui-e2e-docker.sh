#!/usr/bin/env bash
set -euo pipefail

if ! command -v docker >/dev/null 2>&1; then
    echo "error: docker is required for UI Playwright E2E tests" >&2
    exit 1
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_IMAGE="${AXON_E2E_APP_IMAGE:-axon-ui-e2e:local}"
PLAYWRIGHT_IMAGE="${AXON_E2E_PLAYWRIGHT_IMAGE:-axon-ui-e2e-playwright:local}"
NODE_MODULES_VOLUME="${AXON_E2E_NODE_MODULES_VOLUME:-axon-ui-e2e-node-modules}"
RUN_ID="axon-ui-e2e-$$"
NETWORK="${RUN_ID}-net"
APP_CONTAINER="${RUN_ID}-app"
START_LOCAL_APP=0

cleanup() {
    if [[ "${START_LOCAL_APP}" == "1" ]]; then
        docker rm -f "${APP_CONTAINER}" >/dev/null 2>&1 || true
        docker network rm "${NETWORK}" >/dev/null 2>&1 || true
    fi
}
trap cleanup EXIT

if [[ -z "${AXON_E2E_BASE_URL:-}" ]]; then
    START_LOCAL_APP=1
    AXON_E2E_BASE_URL="http://axon:4170"

    docker build -t "${APP_IMAGE}" "${ROOT}"
    docker network create "${NETWORK}" >/dev/null
    docker run -d \
        --name "${APP_CONTAINER}" \
        --network "${NETWORK}" \
        --network-alias axon \
        "${APP_IMAGE}" >/dev/null

    for _ in $(seq 1 60); do
        if ! docker ps --format '{{.Names}}' | grep -Fxq "${APP_CONTAINER}"; then
            docker logs "${APP_CONTAINER}" >&2 || true
            echo "error: Axon E2E app container exited before becoming healthy" >&2
            exit 1
        fi
        if docker exec "${APP_CONTAINER}" curl -fsS http://localhost:4170/health >/dev/null; then
            break
        fi
        sleep 1
    done

    if ! docker exec "${APP_CONTAINER}" curl -fsS http://localhost:4170/health >/dev/null; then
        docker logs "${APP_CONTAINER}" >&2 || true
        echo "error: Axon E2E app container did not become healthy" >&2
        exit 1
    fi
    DOCKER_NETWORK_ARGS=(--network "${NETWORK}")
else
    DOCKER_NETWORK_ARGS=(--add-host=host.docker.internal:host-gateway)
fi

if [[ -z "${AXON_E2E_PLAYWRIGHT_IMAGE:-}" ]]; then
    docker build --target ui-e2e-runner -t "${PLAYWRIGHT_IMAGE}" "${ROOT}"
fi

docker run --rm \
    "${DOCKER_NETWORK_ARGS[@]}" \
    -e AXON_E2E_BASE_URL="${AXON_E2E_BASE_URL}" \
    -v "${ROOT}/ui:/work" \
    -v "${NODE_MODULES_VOLUME}:/work/node_modules" \
    -w /work \
    "${PLAYWRIGHT_IMAGE}" \
    bash -lc 'bun install --frozen-lockfile && bun x playwright test --config playwright.config.ts --reporter=list "$@"' \
    bash "$@"
