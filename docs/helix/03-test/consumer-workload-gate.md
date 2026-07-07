---
ddx:
  id: CWG-001
  depends_on:
    - TP-001
  review:
    reviewed_at: "2026-07-03T00:00:00Z"
---

# Consumer Workload Gate

## Purpose

The consumer workload gate proves Axon against downstream projects that use it
as their application data plane. The gate runs real consumer commands where a
source checkout or exported workload exists, records durable evidence, and
refuses to treat skipped tests, fake transports, or missing repositories as a
green result.

**Decision of record (2026-07-06, decision owner: Erik LaBianca,
operator/product owner)**: no whole-consumer workload (Nexiq, DDx, Cayce) is
deferred out of pilot release scope. All three remain required for release
qualification per the status matrix below; see
`docs/helix/06-iterate/DECISION-2026-07-06-release-and-readiness-dispositions.md`
§4 for the full disposition.

This document is the contract for `scripts/run-consumer-workloads.sh` and the
CI/nightly jobs that call it.

## Status Matrix

The runner emits one status and one classification per consumer/backend run.
The gate mode decides whether that result exits successfully.

| Status | Classification | PR | nightly | release | Meaning |
| --- | --- | --- | --- | --- | --- |
| `passed` | `none` | pass | pass | pass | Required commands ran against the configured Axon endpoint and all required evidence was captured. |
| `failed` | `axon_defect` | fail | fail | fail | Axon process, API, storage, policy, or schema behavior broke a consumer command. |
| `failed` | `consumer_defect` | fail | fail | fail | The consumer command failed after Axon was healthy and the failure is attributable to consumer code or setup. |
| `failed` | `contract_gap` | fail | fail | fail | The requested real workload is not implementable with the current Axon/consumer contract, uses a fake transport, or lacks required endpoint wiring. |
| `failed` | `infra` | fail | fail | fail | Tooling, dependency installation, ports, Docker, or host resources prevented a valid run. |
| `missing` | `missing_workload` | pass | pass-with-warning | fail | The consumer source or exported fixture is absent. This is acceptable for ordinary PR/nightly runs but blocks release qualification. |
| `blocked` | `contract_gap` | pass-with-warning | fail | fail | A known missing integration path prevents a real workload. This is allowed only when the mode explicitly permits known blockers. |
| `failed` | `consumer_dirty` | pass | pass | fail | The resolved consumer checkout has uncommitted changes, so the run is not reproducible from a recorded SHA. Only release qualification gates on this; PR/nightly runs still record `consumer_dirty: true` in `summary.json`. |
| `unknown` | `unknown` | fail | fail | fail | The runner cannot classify the result from mechanical evidence. Unknown never passes. |

The default mode is `pr`. Operators must request `nightly` or `release`
explicitly with a runner flag or environment variable.

The runner also accepts workload-selector modes for direct consumer execution:
`contract` runs the lightweight consumer contract workload, and `e2e` runs the
consumer's real-Axon end-to-end workload. These modes are strict execution
modes, not optional discovery gates: missing consumer source is
`missing_workload` and exits nonzero.

## Source Acquisition

Consumer source resolution is deterministic:

1. Use an explicit consumer path flag or environment variable when provided.
2. Otherwise look for the documented sibling checkout, such as `../nexiq`,
   `../ddx`, or `../cayce`, relative to the Axon repository root.
3. If a checkout URL and revision are configured for CI, clone/fetch into the
   run directory and check out that exact revision.
4. Capture the resolved path, `git rev-parse HEAD`, and dirty-worktree state
   for every consumer checkout in `consumer_path`, `consumer_sha`, and
   `consumer_dirty` on `summary.json`, regardless of mode. A checkout that is
   not a git working tree records `consumer_sha: null` and
   `consumer_dirty: false`.
5. If no checkout or exported workload exists, emit `missing` /
   `missing_workload`; do not synthesize fixtures.
6. Release qualification fails (`failed` / `consumer_dirty`) when the
   resolved consumer checkout has uncommitted changes (`git status
   --porcelain` is non-empty), because a dirty checkout cannot be reproduced
   from the recorded SHA. This check runs before any workload command
   executes. PR and nightly modes do not gate on it but still record the
   dirty state.

GitHub Actions must not hardcode `/home/erik` paths. Local-only sibling paths
are allowed for developer runs and must be shown in `summary.json`.

## Axon Launch Contract

Each run uses an isolated directory under `target/consumer-workloads/<run-id>`.
The runner records the Axon repository SHA before starting any service.

Backends:

- `sqlite`: start Axon with `AXON_STORAGE=sqlite`,
  `AXON_SQLITE_PATH=<run-dir>/axon.db`, and an allocated HTTP port.
- `postgres`: either start the repository Docker Compose Postgres profile or
  use an explicit `AXON_POSTGRES_DSN`. The DSN, port, and cleanup behavior must
  be captured. Persistent shared volumes are not allowed in CI.

The runner waits for `/health` before starting consumer commands. A failure to
start or become healthy is classified as `infra` unless Axon logs identify an
Axon defect.

Child processes are killed on exit through shell traps or equivalent cleanup.
The runner must not leave Axon, Postgres, preview servers, or Playwright
processes running after failure.

## Endpoint Injection

Consumers must use the Axon endpoint started by the runner. The runner injects
the endpoint, tenant, database, and schema hash when available.

Required baseline variables:

- `AXON_ENDPOINT`
- `AXON_TENANT`
- `AXON_DATABASE`

Nexiq-specific aliases:

- `NEXIQ_AXON_ENDPOINT`
- `NEXIQ_AXON_TENANT`
- `NEXIQ_AXON_DATABASE`
- `NEXIQ_AXON_SCHEMA_HASH` when the runner can discover it

If a consumer starts its own unrelated Axon instance or ignores the injected
endpoint, the result is `failed` / `contract_gap`.

## Skip Detection

Skip Detection is mechanical. A command can pass only when at least one of the
following proves real execution:

- the command exits 0 and its runner reports nonzero executed test count with
  zero required-test skips;
- in release mode, those counts come from a native machine-readable payload
  rather than heuristic stdout markers; the current accepted shape is a JSON
  object with `executed_tests` and `skipped_tests` fields;
- the consumer log includes a required marker configured for that workload;
- Axon request logs show traffic from the consumer during the command; or
- a workload-specific postcondition query confirms the expected writes in Axon.

The following never count as pass:

- `RUN_INTEGRATION` or equivalent integration flag missing for a gated command;
- Playwright, Bun, Go, or Rust output showing all relevant tests skipped;
- DDx fake GraphQL transports or in-process emulations used as the only proof
  for a real Axon backend;
- missing consumer source; or
- missing `summary.json`.

### Release-Mode Traffic Proof

Passing native test counts is necessary but not sufficient in release mode. A
command can still be lying about exercising Axon at all — it can read
`AXON_ENDPOINT` and never call it. Release qualification additionally requires
one of, for every consumer, not only DDx:

1. captured Axon request traffic, evidenced by a `real_axon_wire_calls=1` or
   `axon_request_log=1` marker in the command's stdout/stderr;
2. a workload-specific postcondition query confirming the expected writes
   landed in Axon, evidenced by an `axon_postcondition_query=1` marker; or
3. an explicit Phase-0 exception, set via
   `AXON_RELEASE_TRAFFIC_PROOF_EXCEPTION=<consumer>` together with a non-empty
   `AXON_RELEASE_TRAFFIC_PROOF_EXCEPTION_REASON`. No consumer currently has
   this exception configured in CI; any future exception must be recorded in
   the consumer disposition artifact and mirrored here before it can affect
   release verdicts.

A command that ignores `AXON_ENDPOINT` (or otherwise fails to leave any of the
above evidence) is classified `contract_gap` and cannot pass release
qualification, even if it reports nonzero native `executed_tests`.

## Failure Classification

Classification is rule-based and conservative:

- `axon_defect`: Axon is healthy enough to receive traffic, then returns
  incorrect data, violates schema/policy/audit expectations, crashes, or logs an
  Axon panic/error that explains the failed consumer command.
- `consumer_defect`: Axon is healthy and the consumer fails due its own build,
  dependency, assertion, or unsupported local setup.
- `contract_gap`: the consumer needs a surface Axon does not expose, the
  adapter cannot force the real endpoint, or only a fake transport exists.
- `missing_workload`: source checkout or exported workload is absent.
- `infra`: host tooling, ports, Docker, dependency install, browser runtime, or
  network setup prevents a meaningful run.
- `consumer_dirty`: the resolved consumer checkout has uncommitted changes;
  release qualification cannot treat the run as reproducible from
  `consumer_sha` alone.
- `unknown`: evidence is insufficient or contradictory. Unknown always fails.

When multiple rules match, choose the most actionable non-unknown
classification and include the raw command/log paths in evidence.

## Evidence Schema

Every run writes `summary.json` under the run directory. The schema is stable
enough for CI parsing and can grow only by adding fields.

Required top-level fields:

```json
{
  "run_id": "consumer-workloads-20260703T000000Z",
  "mode": "pr",
  "consumer": "nexiq",
  "backend": "sqlite",
  "status": "passed",
  "classification": "none",
  "axon_sha": "git sha",
  "consumer_path": "resolved consumer checkout path or null",
  "consumer_sha": "git sha or null",
  "consumer_dirty": false,
  "endpoint": "http://127.0.0.1:4170",
  "tenant": "nexiq-dev",
  "database": "default",
  "schema_hash": null,
  "started_at": "RFC3339",
  "finished_at": "RFC3339",
  "exit_code": 0,
  "commands": [],
  "artifacts": [],
  "failure": null
}
```

Each command entry records name, working directory, environment keys injected
by the runner, command argv or shell string, exit code, duration, stdout log,
stderr log, skipped/executed test counts when available, and a
`test_counts_source` field (`native`, `heuristic`, or `none`) so release
qualification can distinguish structured evidence from advisory stdout
markers.

Artifacts include Axon logs, consumer logs, JUnit output when available,
Playwright artifacts, and any postcondition query output.

## Consumer Contracts

### Nexiq

Nexiq is the first real workload. The runner uses `../nexiq` by default and
injects the Nexiq endpoint variables. Contract mode runs:

```bash
RUN_INTEGRATION=1 bun test tests/contract/axon-contract.spec.ts
```

E2E mode runs:

```bash
bun run scripts/run-e2e-real-axon.ts
```

Dry-run mode must print the exact commands and injected endpoint variables.

### DDx

DDx has a local checkout at `../ddx`, but its current Axon backend is not a
passing real workload until it performs real wire calls against a runner-owned
Axon endpoint. Fake GraphQL transports and local JSONL emulation classify as
`contract_gap`, not pass.

Until that contract exists, DDx dry-runs print the intended future proof steps
and write `blocked` / `contract_gap` to `summary.json`. A passing DDx workload
requires an explicit real command contract, injected runner endpoint variables,
nonzero execution evidence, and proof of real Axon wire calls such as captured
request-log evidence. The runner must reject fake transports, in-process
emulation, JSONL-shaped writes, and skipped backend tests as `contract_gap`.

### Cayce

No local Cayce checkout or exported workload is currently required for PR
runs. If the Cayce source checkout or exported workload path is absent, the
runner emits `missing` / `missing_workload` and never synthesizes marketing
fixtures. In release mode and strict workload-selector modes, absent Cayce
source exits nonzero until a real source checkout or exported workload is
configured.

## CI Use

PR jobs should run the runner self-test and cheap dry-runs. They may run Nexiq
contract mode only when source acquisition is configured and the command cannot
silently skip.

The GitHub PR workflow runs:

- `scripts/run-consumer-workloads.sh --self-test`
- Nexiq contract mode when a local `NEXIQ_WORKLOAD_PATH` checkout is present,
  otherwise a Nexiq contract dry-run
- DDx PR dry-run, which must report `blocked` / `contract_gap` until a real
  Axon wire-call contract is configured
- Cayce PR missing-workload probe, which must report `missing` /
  `missing_workload` when no source/export is configured

The PR workflow uploads `target/consumer-workloads/pr-*` as the
`consumer-workload-pr` artifact. It also prints the DDx and Cayce
status/classification pairs so those pass-with-warning states are visible in
GitHub Actions logs instead of being silently green.

Nightly and `workflow_dispatch` jobs run a Nexiq workload matrix for `sqlite`
and `postgres` backends and upload each run directory as an artifact. Scheduled
runs can check out Nexiq by setting repository variable
`AXON_NEXIQ_REPOSITORY`; manual runs can pass `nexiq_repository` and
`nexiq_ref` inputs. If no checkout is configured, the runner emits `missing` /
`missing_workload` per the status matrix and still uploads the summary/log
directory.

Release qualification must fail on `missing_workload`, `contract_gap`,
`consumer_dirty`, `unknown`, any `failed` status, and any run whose test
counts are only supported by heuristic stdout markers instead of a native
machine-readable payload. No consumer currently has a Phase-0 exception that
narrows this rule; any future exception must be recorded explicitly in the
consumer disposition artifact and mirrored here before it can affect release
verdicts.
