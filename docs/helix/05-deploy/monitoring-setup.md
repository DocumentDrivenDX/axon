---
ddx:
  id: MONITORING-SETUP-001
  depends_on:
    - FEAT-025
    - FEAT-028
    - CONTRACT-008
    - DEPLOY-CHECKLIST-001
    - RUNBOOK-001
---

# Monitoring Setup

Readiness artifact: what Axon exposes today for health, latency, error rate,
storage, connections, and tenant visibility, what alert thresholds a
deployment should watch, and how monitoring evidence is captured during
release qualification. This documents current signals honestly, including
gaps — it is not a production observability stack (out of scope; see
`docs/helix/05-deploy/runbook.md` and
`docs/helix/05-deploy/deployment-checklist.md` for the operational
procedures these signals feed).

## Signal Inventory

| Signal | Exposed today | Source | Notes |
|--------|----------------|--------|-------|
| Health | Yes | `GET /health` (`crates/axon-server/src/gateway.rs`) | Returns `status`, `version`, `uptime_seconds`, `backing_store.{backend,status}`, `databases`, `default_namespace`, `default_namespace_status`. |
| Reachability | Yes | `axon doctor` (`crates/axon-cli/src/doctor.rs`) | Resolves config, storage backend, ports, and auth mode; probes `{server_url}/health`; prints the effective CLI mode, `reachable` / HTTP status / `not reachable`, and next-step guidance when the configured server is unreachable. |
| p99 / request latency | No | — | No latency histogram or timing middleware is implemented. `/metrics` is reserved as a non-data-plane path prefix (`crates/axon-server/src/path_router.rs`) but no metrics handler is registered on the router — there is nothing to scrape yet. Track under the improvement backlog. |
| Error rate | Partial | server logs; write-rate-limit rejections (`crates/axon-server/src/rate_limit.rs`) return a typed `rate_limit_exceeded` error body | No aggregated error-rate counter or endpoint exists. During release qualification, error rate is derived by tailing/grepping server logs over the soak window (see Evidence Capture below), not read from a dashboard. |
| Storage | Partial | `GET /health` `databases` list; `axon database list` | Reports which databases exist and default-namespace status, not size/disk-usage/growth. No storage-capacity signal is exposed. |
| Connections | No | — | No connection-pool or concurrent-connection count is exposed by the server or `axon doctor`. Postgres pool sizing is caller-configured (`AXON_POSTGRES_DSN`) but pool utilization is not surfaced. |
| Tenant visibility | Yes (REST only) | `GET /control/tenants` (`crates/axon-server/src/control_plane_routes.rs`) | Lists tenants with `db_name` for routing verification. **Not exposed on GraphQL** — `dbName` is intentionally absent from `ControlTenant` per ADR-018; REST is authoritative for `db_name`. |
| Tenant retention policy | Yes | `GET /control/tenants/{id}/retention` | Confirms the retention policy in effect per tenant; not itself an alerting signal. |
| Auth mode | Yes | `axon doctor` | Prints the resolved auth mode; used to catch unintended `no-auth` exposure (see Alert Thresholds). |

## Alert Thresholds

These are the thresholds already codified in the runbook and deployment
checklist rollback triggers (`docs/helix/05-deploy/runbook.md` §"Rollback
Entry Conditions", `docs/helix/05-deploy/deployment-checklist.md` §"Rollback
Triggers"). Restated here as the monitoring-facing view:

| Condition | Threshold | Action |
|-----------|-----------|--------|
| Health endpoint failing | `GET /health` non-2xx for more than 5 minutes after start | Stop service, restore previous binary, restart — see runbook "Server Will Not Start or Is Unreachable". |
| Error rate during soak | Elevated 4xx/5xx observed in server logs during the first 15 minutes after cutover | Stop rollout and revert traffic gating — see deployment checklist "Verification Checks". |
| Wrong-tenant / default-DB routing | Any data-plane request routed to master/default DB or to the wrong tenant | Stop service immediately; treat as a data-sovereignty incident — see runbook "Tenant Routing or Data-Sovereignty Incident". |
| Unintended `no-auth` exposure | `axon doctor` reports auth mode `no-auth` without explicit opt-in | Stop service, remove `--no-auth`, restart authenticated. |
| Control-plane DB failure | Control-plane DB fails to open or migrate on start | Stop service, restore backed-up control-plane DB + `tenants/`. |
| Port conflict | `axon serve` reports a conflicting port and exits non-zero | Resolve the conflict or revert; never run two instances. |

Latency (p99) and storage-capacity thresholds are **not yet defined** because
the underlying signals are not exposed (see Signal Inventory). Do not treat
this artifact as claiming a latency or capacity alert exists — add rows here
only once the corresponding signal ships.

## Evidence Capture During Release Qualification

Monitoring evidence for a release follows the same evidence rule as the
deployment checklist (`docs/helix/05-deploy/deployment-checklist.md` §"Evidence
rule"): each check below records an artifact or command-log path under
`.ddx/executions/<release-bundle>/`.

| Check | Command | Evidence path |
|-------|---------|----------------|
| Health at cutover | `curl -fsS http://localhost:4170/health` (HTTPS if TLS) | `.ddx/executions/<release-bundle>/verify/http-health.log` |
| Reachability + resolved config | `axon doctor` | `.ddx/executions/<release-bundle>/predeploy/axon-config.log` |
| Tenant inventory (`db_name` correctness) | `curl -fsS http://localhost:4170/control/tenants` | `.ddx/executions/<release-bundle>/verify/control-tenants.log` |
| Error-rate soak window | tail/grep server logs (`journalctl -u axon` / launchd logs) for the first 15 minutes after cutover | `.ddx/executions/<release-bundle>/rollout/error-rate.json` (deployment checklist "Rollout Plan" already names this path) |
| Database enumeration | `axon database list` | `.ddx/executions/<release-bundle>/verify/database-list.log` |
| Auth mode | `axon doctor` | `.ddx/executions/<release-bundle>/verify/auth-mode.log` |

A release is not qualified on monitoring grounds unless every row above has a
retrievable log path, per the deployment checklist evidence rule — a
`[pending]` result with no artifact does not satisfy readiness.

## Gaps and Follow-Up

- No `/metrics` endpoint (Prometheus or otherwise) is implemented; `/metrics`
  is currently only a reserved path prefix. Track adding a metrics exporter
  (latency histogram, request counters, error-rate counter) in the
  improvement backlog (`docs/helix/06-iterate/improvement-backlog.md`).
- No storage-capacity or connection-pool-utilization signal exists.
- On-call rotation and dashboards are not yet defined (same gap the runbook
  already flags under "Ownership team" and "Escalation and Communications").

## References

- Runbook: `docs/helix/05-deploy/runbook.md`
- Deployment checklist: `docs/helix/05-deploy/deployment-checklist.md`
- CLI and config contract: `docs/helix/02-design/contracts/CONTRACT-008-cli-and-config.md`
- Control plane feature: `docs/helix/01-frame/features/FEAT-025-control-plane.md`
- Unified binary feature: `docs/helix/01-frame/features/FEAT-028-unified-binary.md`
- `dbName` / ADR-018 boundary: `docs/helix/02-design/adr/ADR-018-tenant-user-credential-model.md`
