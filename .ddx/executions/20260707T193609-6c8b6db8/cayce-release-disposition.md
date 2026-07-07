# Cayce Release Disposition

- Run command: `CAYCE_WORKLOAD_PATH=../cayce scripts/run-consumer-workloads.sh --consumer cayce --backend sqlite --mode release`
- Run summary: `target/consumer-workloads/consumer-workloads-20260707T194053Z-3569564/summary.json`
- Consumer: `cayce`
- Source/export location: `../cayce` (not present in this execution workspace)
- Consumer SHA: `null`
- Dirty state: `false`
- Runner result: `status=missing`, `classification=missing_workload`, `exit_code=1`
- Owner: Erik LaBianca (operator/product owner)
- Rationale: the Cayce source checkout or exported workload is absent here, and the runner must not synthesize fake marketing fixtures for release qualification.
- Target repo / commit expectation: `~/Projects/cayce`, future commit that provides a real source checkout or exported workload and emits real Axon traffic/workload evidence.
- Verdict impact: Cayce remains non-green for release qualification until real source/export evidence exists.
