#!/usr/bin/env bash
# Slow-test wall-clock ratchet.
#
# Runs a test command, tees its output to a log file, and fails if wall-clock
# time exceeds a threshold. Guards against regression back to the per-test
# Docker container-startup pattern documented in
# docs/helix/03-test/ci-ratchets.md (PostgreSQL L4 conformance suite: 148.23s
# total before the fix landed, 18.74s after).
#
# Usage:
#   scripts/check-slow-tests.sh <threshold-seconds> <log-path> -- <command...>
#
# Example (matches the storage-conformance job in .github/workflows/ci.yml):
#   scripts/check-slow-tests.sh 60 /tmp/storage-conformance.log -- cargo test -p axon-storage
#
# Exit code: 0 = command succeeded within the threshold.
#            The command's own exit code if it failed.
#            2 = command succeeded but exceeded the threshold (ratchet trip).

set -uo pipefail

if [[ $# -lt 4 || "$3" != "--" ]]; then
    echo "Usage: $0 <threshold-seconds> <log-path> -- <command...>" >&2
    exit 64
fi

THRESHOLD_SECONDS="$1"
LOG_PATH="$2"
shift 3

mkdir -p "$(dirname "$LOG_PATH")"

START=$(date +%s)
"$@" 2>&1 | tee "$LOG_PATH"
STATUS=${PIPESTATUS[0]}
END=$(date +%s)
ELAPSED=$((END - START))

echo "Elapsed: ${ELAPSED}s (threshold: ${THRESHOLD_SECONDS}s)"

if [[ "$STATUS" -ne 0 ]]; then
    exit "$STATUS"
fi

if (( ELAPSED > THRESHOLD_SECONDS )); then
    echo "Slow-test ratchet tripped: ${ELAPSED}s exceeds ${THRESHOLD_SECONDS}s threshold." >&2
    echo "See docs/helix/03-test/ci-ratchets.md for the prior root cause (per-test container startup) before raising this threshold." >&2
    exit 2
fi
