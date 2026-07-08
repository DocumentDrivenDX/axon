#!/usr/bin/env bash
# Run the Axon L5 criterion benchmarks (BM-001..BM-012).
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
# Evidence contract: this runner writes
# target/benchmarks/<run-id>/benchmark-metadata.json with the commit,
# environment, backend inventory, dataset sizes, and p99 artifact paths.
#
# Exit code: 0 = benchmarks ran, non-zero = compilation or runtime error.

set -euo pipefail

FILTER="${1:-}"
ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

RUN_ID="benchmarks-$(date -u +%Y%m%dT%H%M%SZ)-$$"
RUN_DIR_REL="target/benchmarks/${RUN_ID}"
RUN_DIR="${ROOT}/${RUN_DIR_REL}"
LOG_DIR="${RUN_DIR}/logs"
MANIFEST_PATH="${RUN_DIR}/benchmark-metadata.json"

mkdir -p "$LOG_DIR"

write_manifest() {
    python3 - "$ROOT" "$RUN_ID" "$RUN_DIR_REL" "$MANIFEST_PATH" "$FILTER" <<'PY'
import json
import os
import platform
import subprocess
import sys
from pathlib import Path

root, run_id, run_dir, manifest_path, filter_value = sys.argv[1:6]


def capture(cmd):
    try:
        return subprocess.check_output(cmd, text=True).strip()
    except Exception:
        return None


def dataset(benchmark, dataset_size):
    return {"benchmark": benchmark, "dataset_size": dataset_size}


manifest = {
    "commit": capture(["git", "-C", root, "rev-parse", "HEAD"]) or "unknown",
    "run_id": run_id,
    "filter": filter_value or None,
    "environment": {
        "hostname": platform.node() or "unknown",
        "platform": platform.platform(),
        "architecture": platform.machine() or "unknown",
        "ci": os.environ.get("CI", "false") == "true",
        "github_actions": os.environ.get("GITHUB_ACTIONS", "false") == "true",
        "runner_name": os.environ.get("RUNNER_NAME") or None,
        "runner_os": os.environ.get("RUNNER_OS") or None,
        "rustc": capture(["rustc", "--version", "--verbose"]),
        "cargo": capture(["cargo", "--version"]),
        "git_branch": capture(["git", "-C", root, "rev-parse", "--abbrev-ref", "HEAD"]),
        "git_status_dirty": bool(capture(["git", "-C", root, "status", "--porcelain"])),
    },
    "benchmark_inventory": [
        {
            "suite": "axon-api",
            "package": "axon-api",
            "backend": "memory",
            "dataset_sizes": [
                dataset("BM-001", "10,000 entities / random point lookups"),
                dataset("BM-002", "10,000 entities / creates with schema validation + audit"),
                dataset("BM-003", "1,000 accounts / 1,000 transactions"),
                dataset("BM-004", "1,000 invoices"),
                dataset("BM-005", "single entity write / audit delta"),
                dataset("BM-006", "155 nodes"),
                dataset("BM-007", "10,000 invoices"),
                dataset("BM-008", "100 concurrent writers"),
                dataset("BM-009", "single typical entity (20 fields, 2 levels nesting)"),
                dataset("BM-010", "1 entity / 100 mutations"),
                dataset("BM-011", "10,000 targets + 128 links"),
                dataset("BM-012", "100 nodes / 99 links"),
            ],
        },
        {
            "suite": "axon-cypher",
            "package": "axon-cypher",
            "backend": "fixture",
            "dataset_sizes": [
                dataset("DDx ready/blocked queue", "1,000 beads"),
                dataset("DDx ready/blocked queue", "10,000 beads"),
            ],
        },
    ],
    "artifacts": {
        "run_dir": run_dir,
        "criterion_root": "target/criterion",
        "logs": [
            f"{run_dir}/logs/axon-api.log",
            f"{run_dir}/logs/axon-cypher.log",
        ],
        "p99_artifact_paths": [
            "target/criterion/**/new/sample.json",
            "target/criterion/**/new/estimates.json",
            "target/criterion/**/report/index.html",
        ],
    },
}

Path(manifest_path).write_text(
    json.dumps(manifest, indent=2, sort_keys=True) + "\n",
    encoding="utf-8",
)
PY
}

run_benchmark_suite() {
    local package="$1"
    local log_path="$2"

    echo "Running ${package} benchmarks..."
    if [[ -n "${FILTER}" ]]; then
        cargo bench -p "$package" -- "${FILTER}" 2>&1 | tee "$log_path"
    else
        cargo bench -p "$package" 2>&1 | tee "$log_path"
    fi
}

write_manifest
echo "Benchmark metadata: $MANIFEST_PATH"
echo "Criterion artifacts: ${ROOT}/target/criterion"

run_benchmark_suite axon-api "$LOG_DIR/axon-api.log"
run_benchmark_suite axon-cypher "$LOG_DIR/axon-cypher.log"

if [[ ! -d "${ROOT}/target/criterion" ]]; then
    echo "criterion output missing at ${ROOT}/target/criterion" >&2
    exit 1
fi

echo "Benchmark run directory: $RUN_DIR"
echo "Benchmark metadata: $MANIFEST_PATH"
