# DDx Phase-0 Deferral Evidence

- Run summary: `target/consumer-workloads/consumer-workloads-20260707T155913Z-2710277/summary.json`
- Consumer: `ddx`
- Consumer repo: `/home/erik/Projects/ddx`
- Consumer SHA: `235fbe60f1d56d3e90878d4b0a1546da46017dc2`
- Dirty state: `false`
- Runner result: `status=blocked`, `classification=contract_gap`, `exit_code=1`
- Verdict impact: DDx does not count as real Axon traffic evidence; release
  qualification remains non-green on the DDx lane until a real wire-call
  command lands in the DDx repo.
- Owner: Erik LaBianca (operator/product owner)
- Rationale: the DDx repo still uses in-process Axon emulation / zero wire
  calls, so the runner cannot treat it as real workload evidence.
- Target repo / commit expectation: `~/Projects/ddx`, next DDx commit that
  replaces emulation with a runner-owned real wire-call workload and captured
  request-log evidence.
- Upstream tracker: `axon-82b6f7b2`
