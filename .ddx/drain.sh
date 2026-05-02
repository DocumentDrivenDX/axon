#!/usr/bin/env bash
# Sequential bead-queue drainer using `ddx agent execute-bead --harness claude`.
# Equivalent to `ddx work` semantics but bypasses the providers config layer
# that ddx work / execute-loop require.

set -u
LOG=/home/erik/Projects/axon/.ddx/drain.log
echo "=== drain.sh starting at $(date -Is) ===" | tee -a "$LOG"

NO_PROGRESS=0
MAX_NO_PROGRESS=3

while true; do
    # Grab the highest-priority ready bead id (or empty).
    bead_id=$(ddx bead ready --json 2>/dev/null \
        | python3 -c 'import json,sys
try:
    data=json.load(sys.stdin)
    if data: print(data[0]["id"])
except: pass' 2>/dev/null)

    if [ -z "$bead_id" ]; then
        echo "[$(date -Is)] queue drained — no ready beads" | tee -a "$LOG"
        exit 0
    fi

    echo "" | tee -a "$LOG"
    echo "[$(date -Is)] === processing $bead_id ===" | tee -a "$LOG"

    # Capture pre-state for progress detection.
    pre_head=$(git -C /home/erik/Projects/axon rev-parse HEAD 2>/dev/null || echo "")

    # Run the bead synchronously. --no-merge would preserve under refs/ but we
    # want the merge so dependent beads can pick up the result.
    ddx agent execute-bead "$bead_id" --harness claude 2>&1 | tee -a "$LOG"
    rc=$?

    post_head=$(git -C /home/erik/Projects/axon rev-parse HEAD 2>/dev/null || echo "")

    echo "[$(date -Is)] === finished $bead_id (rc=$rc) ===" | tee -a "$LOG"

    if [ "$pre_head" != "$post_head" ]; then
        NO_PROGRESS=0
    else
        NO_PROGRESS=$((NO_PROGRESS + 1))
        echo "[$(date -Is)] no commit produced (consecutive=$NO_PROGRESS)" | tee -a "$LOG"
    fi

    if [ "$NO_PROGRESS" -ge "$MAX_NO_PROGRESS" ]; then
        echo "[$(date -Is)] STOP: $MAX_NO_PROGRESS consecutive attempts produced no commit" | tee -a "$LOG"
        exit 1
    fi
done
