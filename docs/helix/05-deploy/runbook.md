---
ddx:
  id: RUNBOOK-001
  depends_on:
    - FEAT-025
    - FEAT-028
    - CONTRACT-008
    - DEPLOY-CHECKLIST-001
---

# Runbook - Axon Unified Binary and Control Plane

## Service Summary

- Service or component: the `axon` unified binary running `axon serve` — the
  HTTP gateway (port 4170), optional gRPC (4171), the embedded per-deployment
  control plane (tenant provisioning and routing), and the optional admin UI.
  This is the only server artifact (FEAT-028 BIN-01); no separate server
  binary ships.
- Primary function: serve the governed data plane (entities, collections,
  links, audit, schema) and route each request to the correct per-tenant
  database via the embedded control plane (FEAT-025 in-deployment surface).
- Business impact if degraded: applications cannot read or write governed
  data; a routing fault can expose the wrong tenant's data — treat
  cross-tenant or default-DB routing as a data-sovereignty incident, not a
  routine outage.
- Ownership team: Core Team. **Assumption**: on-call rotation and channels are
  not yet defined in the repo — fill in the real rotation when established.
- On-call rotation: [link or contact — fill in]
- Environments covered: staging and production single-deployment installs
  (Linux systemd / macOS launchd). Distributed placement, DB migration, and
  cross-node routing are out of scope (FEAT-025 parking lot).

## Operator Entry Points

| Situation | First dashboard, log, or query | First command or check | Owner |
|-----------|--------------------------------|------------------------|-------|
| Server down or unreachable | service logs (`journalctl -u axon` / launchd logs) | `axon doctor`; `axon server status` | on-call |
| Health endpoint failing | server logs | `curl -fsS http://localhost:4170/health` | on-call |
| Wrong-tenant / default-DB routing | server logs (routing-rejection entries) | `curl -fsS http://localhost:4170/control/tenants` | on-call + security owner |
| Unexpected `no-auth` exposure | resolved auth mode line | `axon doctor` | on-call + security owner |
| TLS handshake failures | server logs; client TLS errors | inspect `--tls-cert`/`--tls-key` or `$XDG_DATA_HOME/axon/tls/` | on-call |
| Control-plane DB will not open | startup logs ("control-plane database opened at …" absent) | check `--control-plane-path` / `AXON_CONTROL_PLANE_PATH` | on-call |

## Dependencies and Failure Boundaries

| Dependency or boundary | Why it matters | Failure signal | Fallback or escalation |
|------------------------|----------------|----------------|------------------------|
| Control-plane DB (`axon-control-plane.db`) | maps tenants to per-tenant databases; provisioning and routing depend on it | startup fails to open/migrate; routing errors | stop service; restore backed-up control-plane DB + `tenants/`; escalate to Core Team |
| Per-tenant SQLite stores (`{data_dir}/tenants/{db_name}.db`) | hold each tenant's governed data | missing/locked tenant DB; readiness failure | stop affected traffic; restore the tenant DB from backup; do not point traffic at the wrong DB |
| PostgreSQL (when `--storage=postgres`) | shared backing store for data plane | connection saturation, DSN errors | verify `AXON_POSTGRES_DSN`; escalate to platform owner; do not fail open to default DB |
| Tailscale daemon (`--tailscale-socket`, default auth mode) | identity/whois for `tailscale` auth mode | whois lookups fail; auth verification fails fast at startup | confirm `tailscaled` running; do **not** switch to `--no-auth` to "fix" auth |
| TLS material (cert/key or self-signed pair) | HTTPS termination at the server | handshake failures; SAN mismatch | supply correct `--tls-self-signed-san` or a CA-signed `--tls-cert`/`--tls-key` |
| Tenant/default-DB boundary | prevents cross-tenant exposure | request routed to master/default DB or wrong tenant | stop service; preserve logs; treat as data-sovereignty incident |

## Alert Triage

| Alert or symptom | Likely causes | Immediate checks | Stop and escalate when |
|------------------|---------------|------------------|------------------------|
| Server unreachable | bad deploy, port conflict, crash on startup, control-plane DB unopenable | `axon doctor`; `axon server status`; service logs | service will not start after one restart |
| Health endpoint non-2xx | storage failure, control-plane DB issue, dependency down | `curl .../health`; storage/control-plane log lines | non-2xx persists beyond 5 minutes |
| Wrong-tenant or default-DB routing | routing regression, malformed data-plane path, missing/empty `db_name` | `/control/tenants` for `db_name`; server routing-rejection logs | any request reaches the wrong tenant or the default/master DB |
| Resolved auth mode is `no-auth` | unintended `--no-auth` opt-in or `AXON_NO_AUTH` set | `axon doctor`; service unit / env | server is network-reachable while unauthenticated |
| TLS handshake / SAN errors | cert/key mismatch, SAN does not cover hostname | inspect cert paths and SAN list | clients cannot connect over the required name |

## Common Incident Procedures

### Server Will Not Start or Is Unreachable

- Trigger: `axon doctor` reports "not reachable", or `axon server status`
  shows the service is not active.
- Immediate actions:
  1. Run `axon doctor` to read resolved config (config path, data dir,
     storage backend, ports, auth mode, reachability).
  2. Check service logs (`journalctl -u axon` on Linux; launchd logs on
     macOS) for the startup error.
  3. If a port conflict is reported, free port 4170 (or the configured
     `server.http_port`) — do not start a second instance.
  4. If the control-plane DB failed to open, verify
     `--control-plane-path` / `AXON_CONTROL_PLANE_PATH` and file permissions.
  5. Restart once: `axon server restart`.
- Validation:
  - `axon doctor` reports the server reachable.
  - `curl -fsS http://localhost:4170/health` returns 2xx.
- Escalate to: Core Team if the control-plane DB is corrupt or the service
  will not start after one restart (proceed to Rollback).

### Tenant Routing or Data-Sovereignty Incident

- Trigger: a data-plane request is routed to the master/default DB or to the
  wrong tenant, or routing-rejection log entries spike.
- Immediate actions:
  1. **Preserve evidence first**: capture server logs, the request path, the
     deploy version (`axon doctor`), and the control-plane DB state before
     any restart or restore.
  2. Stop the service (`axon server stop`) to contain exposure.
  3. Verify expected tenant-to-`db_name` mapping via
     `curl -fsS http://localhost:4170/control/tenants` (REST is authoritative
     for `db_name`; it is intentionally absent from GraphQL per ADR-018) —
     run against a known-good instance or backup if the live one is stopped.
  4. Do not "repair" by relaxing auth or pointing traffic at the default DB.
  5. Roll back to the previous known-good binary (see Rollback) once evidence
     is preserved.
- Validation:
  - spot-checked tenant requests resolve to the correct per-tenant DB.
  - no further default-DB / cross-tenant routing entries after restart.
- Escalate to: security owner and Core Team; this is a data-sovereignty event.

### Unintended Unauthenticated Exposure (`no-auth`)

- Trigger: `axon doctor` reports auth mode `no-auth` without an explicit,
  intended opt-in.
- Immediate actions:
  1. Stop the service immediately (`axon server stop`).
  2. Remove the `--no-auth` flag / `AXON_NO_AUTH` env and reinstall or serve
     authenticated (default `tailscale`, or `--guest-role` if intended).
  3. Preserve access logs covering the unauthenticated window.
  4. Restart: `axon server start`.
- Validation:
  - `axon doctor` shows the intended authenticated mode.
- Escalate to: security owner with the exposure window from the logs.

## Rollback and Recovery

### Rollback Entry Conditions

- Health endpoint non-2xx for more than 5 minutes after a fresh start.
- Any confirmed wrong-tenant or default-DB routing introduced by the deploy.
- Control-plane DB fails to open or migrate after the upgrade.
- Unintended `no-auth` exposure that the binary cannot be reconfigured out of.

### Rollback Procedure

1. Announce the rollback to the release/incident channel.
2. Record the current deploy version (`axon doctor`), the trigger, and any
   affected tenants.
3. Stop the service: `axon server stop`.
4. Restore the backed-up control-plane DB and tenant stores if the upgrade
   migrated them (`{data_dir}/axon-control-plane.db` and `{data_dir}/tenants/`).
5. Replace the `axon` binary with the previous known-good version.
6. Start the service: `axon server start`.

### Recovery Validation

- `axon doctor` reports the server reachable and the expected auth mode.
- `curl -fsS http://localhost:4170/health` returns 2xx (HTTPS if TLS enabled).
- `curl -fsS http://localhost:4170/control/tenants` lists the expected tenants
  with correct `db_name` values.
- Spot-checked data-plane requests resolve to the correct per-tenant DB; no
  routing-rejection or default-DB entries in the logs.
- Error rate stays within threshold for 15 minutes.

## Routine Operations

| Operation | Trigger or cadence | Command or workflow | Verification |
|-----------|--------------------|---------------------|--------------|
| Inspect installation state | before/after any deploy or on incident | `axon doctor` | resolved config, reachability, and auth mode printed |
| Provision a tenant | onboarding a new tenant | `curl -X POST http://localhost:4170/control/tenants ...` (CONTRACT / control-plane routes) | tenant appears in `/control/tenants` with a non-empty `db_name` |
| Manage user roles | access change | `axon user grant <login> <role>` / `axon user revoke <login>` | `axon user list` reflects the change |
| Rotate / regenerate self-signed TLS | hostname/SAN change | delete the existing pair in `$XDG_DATA_HOME/axon/tls/`, restart with `--tls-self-signed-san <names>` | new cert covers the required SANs; handshake succeeds |
| Back up control-plane + tenant data | before every upgrade, plus regular cadence | copy `{data_dir}/axon-control-plane.db` and `{data_dir}/tenants/` | backup exists and is restorable |

## Escalation and Communications

1. Primary on-call: [application on-call — fill in].
2. Secondary escalation: Core Team.
3. Incident coordinator or manager: [product/release owner — fill in].
4. Security escalation: security owner for any cross-tenant routing,
   default-DB exposure, or unintended `no-auth` incident.
5. External dependency support: [hosting / Postgres / Tailscale support — fill
   in per deployment].

## References

- Deployment checklist: `docs/helix/05-deploy/deployment-checklist.md`
- CLI and config contract: `docs/helix/02-design/contracts/CONTRACT-008-cli-and-config.md`
- Architecture: `docs/helix/02-design/architecture.md`
- Control plane feature: `docs/helix/01-frame/features/FEAT-025-control-plane.md`
- Unified binary feature: `docs/helix/01-frame/features/FEAT-028-unified-binary.md`
- Release notes: `docs/helix/05-deploy/release-notes-0.4.x.md`
- Historical release note: `docs/helix/05-deploy/release-notes-0.7.1.md`
- Monitoring setup: not yet authored (`docs/helix/05-deploy/monitoring-setup.md`
  — placeholder; signals here use `axon doctor`, `/health`, and server logs).
- Security architecture: not yet authored
  (`docs/helix/02-design/security-architecture.md` — placeholder).
