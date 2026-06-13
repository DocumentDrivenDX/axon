#!/usr/bin/env bash
# Run the axon-sim bounded seed sweep.
#
# Usage:
#   scripts/run-sim-seeds.sh              # 10 seeds (CI default)
#   scripts/run-sim-seeds.sh 100          # 100 seeds (extended local run)
#   AXON_SIM_SEEDS=1000 scripts/run-sim-seeds.sh   # 1000 seeds (nightly)
#
# Any invariant violation is reported with the failing seed.  Add that seed to
# scripts/regression-seeds.txt to replay it on every future CI build.
#
# Exit code: 0 = all seeds passed, non-zero = violation found.

set -euo pipefail

SEEDS="${1:-${AXON_SIM_SEEDS:-10}}"

echo "Running axon-sim seed sweep: AXON_SIM_SEEDS=${SEEDS}"
AXON_SIM_SEEDS="${SEEDS}" cargo test -p axon-sim -- seed_sweep --nocapture
