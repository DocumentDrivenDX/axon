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
   for every consumer checkout.
5. If no checkout or exported workload exists, emit `missing` /
   `missing_workload`; do not synthesize fixtures.

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
stderr log, and skipped/executed test counts when available.

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

### Cayce

No local Cayce checkout or exported workload is currently required for PR
runs. In release mode, absent Cayce source emits `missing_workload` and fails
the release gate until a real source checkout or exported marketing-control
workload is configured.

## CI Use

PR jobs should run the runner self-test and cheap dry-runs. They may run Nexiq
contract mode only when source acquisition is configured and the command cannot
silently skip.

Nightly jobs may run full Nexiq SQLite and Postgres workloads and upload the
run directory as an artifact.

Release qualification must fail on `missing_workload`, `contract_gap`,
`unknown`, and any `failed` status.
