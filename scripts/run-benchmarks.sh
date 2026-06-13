#!/usr/bin/env bash
# Run the Axon L5 criterion benchmarks (BM-001..BM-010).
#
# Usage:
#   scripts/run-benchmarks.sh              # run all benchmarks
#   scripts/run-benchmarks.sh BM-001       # run a single benchmark by name filter
#
# Benchmarks are NOT run in per-commit CI (too expensive).
# They are run nightly via .github/workflows/nightly.yml, and locally
# before any change that might affect latency targets.
#
# Ratchet status: nightly/manual — see docs/helix/03-test/ci-ratchets.md.
#
# Exit code: 0 = benchmarks ran, non-zero = compilation or runtime error.

set -euo pipefail

FILTER="${1:-}"

echo "Running axon-api benchmarks (BM-001..BM-010)..."
if [[ -n "${FILTER}" ]]; then
    cargo bench -p axon-api -- "${FILTER}"
else
    cargo bench -p axon-api
fi

echo "Running axon-cypher benchmarks..."
if [[ -n "${FILTER}" ]]; then
    cargo bench -p axon-cypher -- "${FILTER}"
else
    cargo bench -p axon-cypher
fi
