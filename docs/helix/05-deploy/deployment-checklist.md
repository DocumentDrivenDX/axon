---
ddx:
  id: DEPLOY-CHECKLIST-001
  depends_on:
    - FEAT-025
    - FEAT-028
    - CONTRACT-008
---

# Deployment Checklist

## Release Scope

- Service or component: `axon` unified binary (HTTP gateway + embedded
  control plane), per FEAT-028 and FEAT-025. The single `axon` binary is the
  only server artifact; no separate server binary ships.
- Version or commit: Axon 0.4.x pilot release target (confirmed 2026-07-06;
  the earlier 0.7.1 documentation target is revoked — see
  `docs/helix/06-iterate/DECISION-2026-07-06-release-and-readiness-dispositions.md`
  §1). `Cargo.toml` declares workspace version `0.4.0` and local/`origin` Git
  tags reach `v0.4.0`. Confirm the resolved version with `axon doctor` before
  treating any version beyond `v0.4.0` as published.
- Deployment window: [date and time — fill in per release]
- Release owner: [name — fill in per release]
- Rollback owner: [name — fill in per release]
- Supporting artifacts: runbook `RUNBOOK-001`
  (`docs/helix/05-deploy/runbook.md`), CLI/config contract `CONTRACT-008`
  (`docs/helix/02-design/contracts/CONTRACT-008-cli-and-config.md`),
  architecture (`docs/helix/02-design/architecture.md`), release notes
  (`docs/helix/05-deploy/release-notes-0.7.1.md` — superseded; historical
  record of the revoked 0.7.1 target).
- Evidence rule: each checklist row below must record `Owner`, `Window`,
  `Rollback`, `Artifact / command-log path`, `Backend`, and `Result`. Do not
  mark a row complete unless the artifact or log path is durable and
  retrievable.

## Pre-Deploy Checks

| Check | Owner | Window | Rollback | Artifact / command-log path | Backend | Result |
|------|-------|--------|----------|-----------------------------|---------|--------|
| Workspace type-checks, tests pass, clippy clean | Release owner | Pre-deploy gate before any cutover | Keep the current release live; do not proceed until the gate is green | `cargo check && cargo test && cargo clippy -- -D warnings`; `.ddx/executions/<release-bundle>/predeploy/cargo-check.log` | local workspace / CI runner | [pending] |
| `serve` feature builds (server capability is feature-gated, default on) | Release owner | Pre-deploy gate before packaging | Keep the prior binary in place; do not cut over on a failed build | `cargo build --release -p axon-cli`; `.ddx/executions/<release-bundle>/predeploy/cargo-build.log` | build runner / `axon-cli` | [pending] |
| Resolved config inspected; data dir, storage backend, ports as intended | Release owner | Before the first start on the target host | Hold the release and correct config before starting service | `axon doctor` and `axon config show`; `.ddx/executions/<release-bundle>/predeploy/axon-config.log` | resolved config / target host | [pending] |
| First-run config auto-creation will not overwrite an existing file (BIN-05) | Release owner | Before first start on a new host | Preserve the existing `config.toml`; do not bootstrap over it | `axon config path`; `.ddx/executions/<release-bundle>/predeploy/axon-config-path.log` | config file path | [pending] |
| Resolved auth mode is **not** `no-auth` unless explicitly intended; service installs default to authenticated (`tailscale`) per BIN-10 | Release owner + security owner | Before exposing the service to users | Stop the rollout, remove unintended `--no-auth`, and rerun authenticated | `axon doctor`; `.ddx/executions/<release-bundle>/predeploy/axon-auth.log` | auth resolver / `tailscale` | [pending] |
| If TLS terminates at Axon, cert/key resolve (`--tls-cert`/`--tls-key`) or self-signed bootstrap covers the reachable hostname/SAN | Release owner | Before enabling TLS traffic | Do not publish the endpoint until cert/key or SAN coverage is correct | `--tls-self-signed-san <names>`; inspect `$XDG_DATA_HOME/axon/tls/`; `.ddx/executions/<release-bundle>/predeploy/axon-tls.log` | TLS material on target host | [pending] |
| Storage backend chosen (`sqlite` default; `postgres` for shared). For Postgres, DSN reachable | Release owner | Before data-plane cutover | Keep traffic on the current backend; fix DSN before retrying | `AXON_POSTGRES_DSN` set; `--storage=postgres`; `.ddx/executions/<release-bundle>/predeploy/axon-storage.log` | `sqlite` or `postgres` | [pending] |
| Control-plane DB path resolves and is writable (`axon.db` control-plane DB + `tenants/` per-tenant SQLite) | Rollback owner | Before the control plane starts on the target host | Restore file permissions or mount points before starting service | `--control-plane-path` / `AXON_CONTROL_PLANE_PATH`; `axon doctor`; `.ddx/executions/<release-bundle>/predeploy/axon-control-plane.log` | control-plane DB + `tenants/` | [pending] |
| Back up existing control-plane DB and `tenants/` directory before upgrading | Rollback owner | Immediately before the upgrade | Do not continue until the backup exists and is restorable | copy `{data_dir}/axon-control-plane.db` and `{data_dir}/tenants/`; `.ddx/executions/<release-bundle>/predeploy/axon-backup.log` | filesystem backup set | [pending] |
| Port 4170 (HTTP) free; gRPC (4171) only if `--grpc-port` set | Release owner | Just before start or restart | Free the port or keep the existing service running; never start a second instance | `axon doctor`; `.ddx/executions/<release-bundle>/predeploy/axon-ports.log` | network sockets on target host | [pending] |
| Service unit target supported: systemd 240+ (Linux) or launchd 10.13+ (macOS) | Release owner | Before installing the service unit | Keep the service manual if the host target is unsupported | platform check; `.ddx/executions/<release-bundle>/predeploy/axon-unit-target.log` | systemd or launchd | [pending] |

## Rollout Plan

| Stage | Owner | Window | Rollback | Artifact / command-log path | Backend | Result |
|-------|-------|--------|----------|-----------------------------|---------|--------|
| Staging install and verify | Release owner | Staging window, before production cutover | Revert the staging install and keep production unchanged | Install/upgrade log path `.ddx/executions/<release-bundle>/rollout/staging-install.log`; `axon serve` or `axon server install` + `axon server start`; `axon doctor` output `.ddx/executions/<release-bundle>/rollout/staging-doctor.log` | staging control-plane DB | [pending] |
| Initial production cutover | Release owner + rollback owner | Production cutover window | Stop service, restore previous binary, restore backups, and restart | Cutover log path `.ddx/executions/<release-bundle>/rollout/production-cutover.log`; backup path `.ddx/executions/<release-bundle>/predeploy/axon-backup.log`; `axon server start` output `.ddx/executions/<release-bundle>/rollout/production-start.log` | production control-plane DB + `tenants/` | [pending] |
| Verification soak | Release owner | First 15 minutes after cutover | Stop rollout and revert traffic gating if error rate or routing drifts | Soak log path `.ddx/executions/<release-bundle>/rollout/soak.log`; error-rate dashboard snapshot `.ddx/executions/<release-bundle>/rollout/error-rate.json`; `/control/tenants` output `.ddx/executions/<release-bundle>/rollout/control-tenants.log` | live traffic / control-plane inventory | [pending] |
| Full rollout | Release owner | After soak passes | Hold remaining deployments and leave gating in place until every target is healthy | Promotion log path `.ddx/executions/<release-bundle>/rollout/promotion.log`; control-plane inventory snapshot `.ddx/executions/<release-bundle>/rollout/inventory.log` | all target deployments | [pending] |

## Verification Checks

| Signal or Check | Owner | Window | Rollback | Artifact / command-log path | Backend | Result |
|-----------------|-------|--------|----------|-----------------------------|---------|--------|
| Server reachability: server reachable at configured URL | Release owner | After start, before traffic handoff | Stop the service and restore the previous binary if the host stays unreachable | `axon doctor` (`reachable`); `.ddx/executions/<release-bundle>/verify/server-reachability.log` | service endpoint | [pending] |
| HTTP health: 2xx from health endpoint | Release owner | After start and during soak | Stop service and restore previous binary if health stays non-2xx | `curl -fsS http://localhost:4170/health` (HTTPS if TLS enabled); `.ddx/executions/<release-bundle>/verify/http-health.log` | HTTP gateway | [pending] |
| Resolved auth mode: matches intended mode (`tailscale` default; never silent `no-auth`) | Release owner + security owner | After start and before exposing users | Stop service, remove unintended `--no-auth`, and restart authenticated | `axon doctor`; `.ddx/executions/<release-bundle>/verify/auth-mode.log` | auth resolver | [pending] |
| Control-plane inventory: tenant list returns expected tenants with correct `db_name` | Release owner | After start and after tenant changes | Restore the control-plane DB and rerun inventory if the list is wrong | `curl -fsS http://localhost:4170/control/tenants` (REST is authoritative for `db_name`; not on GraphQL per ADR-018); `.ddx/executions/<release-bundle>/verify/control-tenants.log` | control-plane inventory | [pending] |
| Tenant routing: data-plane requests route to the correct per-tenant DB; malformed paths rejected, not silently routed to master/default | Release owner + rollback owner | During soak and after any routing change | Stop service immediately and restore the previous binary if wrong-tenant routing appears | spot-check a known tenant request; server logs for routing-rejection entries; `.ddx/executions/<release-bundle>/verify/tenant-routing.log` | tenant routing backend | [pending] |
| Database list: embedded/server storage enumerates databases | Release owner | After start, before full rollout | Hold the rollout and investigate storage enumeration before promoting | `axon database list`; `.ddx/executions/<release-bundle>/verify/database-list.log` | embedded/server storage | [pending] |
| TLS (if enabled): HTTPS handshake succeeds with the reachable hostname | Release owner | After TLS enablement and before public exposure | Replace cert/key or SAN config before serving TLS traffic | `curl -fsS https://<reachable-name>:4170/health`; `.ddx/executions/<release-bundle>/verify/tls-handshake.log` | TLS termination | [pending] |

## Rollback Triggers

| Trigger | Threshold or Condition | Immediate Action | Owner | Evidence artifact / log path | Result |
|---------|------------------------|------------------|-------|-----------------------------|--------|
| Health endpoint failing | `GET /health` non-2xx for more than 5 minutes after start | Stop service, restore previous binary, `axon server start`; see runbook "Rollback Procedure" | Rollback owner | `.ddx/executions/<incident-bundle>/rollback/health-failure.log` | [pending] |
| Default-DB / cross-tenant routing | Any data-plane request routed to master/default DB or to the wrong tenant | Stop service immediately, restore previous binary, preserve logs; escalate per runbook "Tenant Routing or Data-Sovereignty Incident" | Rollback owner | `.ddx/executions/<incident-bundle>/rollback/routing-incident.log` | [pending] |
| Unintended `no-auth` exposure | `axon doctor` shows `no-auth` without explicit opt-in | Stop service, reinstall/serve without `--no-auth`, restart authenticated | Release owner | `.ddx/executions/<incident-bundle>/rollback/no-auth-exposure.log` | [pending] |
| Control-plane DB corruption / migration failure | Control-plane DB fails to open or migrate on start | Stop service, restore backed-up `axon-control-plane.db` + `tenants/`, restart previous version | Rollback owner | `.ddx/executions/<incident-bundle>/rollback/control-plane-db.log` | [pending] |
| Port conflict on start | `axon serve` reports conflicting port, exits non-zero | Resolve the conflict or revert to previous service; do not run two instances | Release owner | `.ddx/executions/<incident-bundle>/rollback/port-conflict.log` | [pending] |

## Go or No-Go Decision

- Decision: [Go / Hold / Roll Back]
- Decision time: [timestamp]
- Evidence artifact: [path to the release execution bundle or command-log set]
- Notes: [exceptions, deferred checks, follow-up — e.g. version/tag caveat;
  see DECISION-2026-07-06-release-and-readiness-dispositions.md §1 for the
  current 0.4.x release target disposition]
- Follow-up owner: Release owner
