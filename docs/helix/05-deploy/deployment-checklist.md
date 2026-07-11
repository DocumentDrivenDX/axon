---
ddx:
  id: DEPLOY-CHECKLIST-001
  depends_on:
    - FEAT-025
    - FEAT-028
    - CONTRACT-008
  review:
    self_hash: e4c04836ff2c8045398310ce0636e3043545f1f9f5baea1a1626211453d9ed17
    deps:
      CONTRACT-008: 9dbb01dffcecc2282b2dc8e819f7e182e1204ba6a8d40c0d0b02b6b9c767e613
      FEAT-025: 5ff1ca8b03318957e25d5a3752ebf8999a45378a7b83aa6c2978739263ac3603
      FEAT-028: eaf50210678ba364441138764677456c6bf02edcc09282dfaa7d1b312f3fea20
    reviewed_at: "2026-07-11T05:09:15Z"
---

# Deployment Checklist

## Release Scope

- Service or component: `axon` unified binary (HTTP gateway + embedded
  control plane), per FEAT-028 and FEAT-025. The single `axon` binary is the
  only server artifact; no separate server binary ships.
- Version or commit: Axon 0.4.x pilot release target (confirmed 2026-07-06;
  the earlier 0.7.1 documentation target is revoked — see
  `docs/helix/06-iterate/DECISION-2026-07-06-release-and-readiness-dispositions.md`
  §1). `Cargo.toml` declares workspace version `0.4.0`, local/`origin` Git tags
  reach `v0.4.0`, and GitHub release `v0.4.0` is published. Confirm the
  resolved binary version with `axon doctor` before treating any version beyond
  `v0.4.0` as published.
- Deployment window: [date and time — fill in per release]
- Release owner: [name — fill in per release]
- Rollback owner: [name — fill in per release]
- Supporting artifacts: runbook `RUNBOOK-001`
  (`docs/helix/05-deploy/runbook.md`), CLI/config contract `CONTRACT-008`
  (`docs/helix/02-design/contracts/CONTRACT-008-cli-and-config.md`),
  architecture (`docs/helix/02-design/architecture.md`), release notes
  (`docs/helix/05-deploy/release-notes-0.4.x.md` — active 0.4.x pilot target;
  `docs/helix/05-deploy/release-notes-0.7.1.md` remains the historical record
  of the revoked 0.7.1 target).

## Pre-Deploy Checks

| Area | Check | Evidence or Command | Status |
|------|-------|---------------------|--------|
| Build | Workspace type-checks, tests pass, clippy clean | `cargo check && cargo test && cargo clippy -- -D warnings` | [ ] |
| Build | `serve` feature builds (server capability is feature-gated, default on) | `cargo build --release -p axon-cli` | [ ] |
| Config | Resolved config inspected; data dir, storage backend, ports as intended | `axon doctor` and `axon config show` | [ ] |
| Config | First-run config auto-creation will not overwrite an existing file (BIN-05) | confirm `config.toml` path with `axon config path` | [ ] |
| Auth | Resolved auth mode is **not** `no-auth` unless explicitly intended; service installs default to authenticated (`tailscale`) per BIN-10 | `axon doctor` (surfaces resolved auth mode) | [ ] |
| Auth | If TLS terminates at Axon, cert/key resolve (`--tls-cert`/`--tls-key`) or self-signed bootstrap covers the reachable hostname/SAN | `--tls-self-signed-san <names>`; inspect `$XDG_DATA_HOME/axon/tls/` | [ ] |
| Data | Storage backend chosen (`sqlite` default; `postgres` for shared). For Postgres, DSN reachable | `AXON_POSTGRES_DSN` set; `--storage=postgres` | [ ] |
| Data | Control-plane DB path resolves and is writable (`axon.db` control-plane DB + `tenants/` per-tenant SQLite) | `--control-plane-path` / `AXON_CONTROL_PLANE_PATH`; `axon doctor` data dir exists | [ ] |
| Data | Back up existing control-plane DB and `tenants/` directory before upgrading | copy `{data_dir}/axon-control-plane.db` and `{data_dir}/tenants/` | [ ] |
| Ops | Port 4170 (HTTP) free; gRPC (4171) only if `--grpc-port` set | `axon doctor` reports running server if port responds | [ ] |
| Ops | Service unit target supported: systemd 240+ (Linux) or launchd 10.13+ (macOS) | platform check | [ ] |

## Rollout Plan

| Stage | Action | Exit Condition |
|-------|--------|----------------|
| Staging | Install/upgrade the binary; run `axon serve` (or `axon server install` + `axon server start`); verify against a staging control-plane DB | `axon doctor` reports reachable server; tenant list returns; auth mode correct |
| Initial production | Stop existing service (`axon server stop`), back up control-plane DB + `tenants/`, replace binary, `axon server start` | Service active; `GET /health` succeeds for 15 minutes; no malformed/default-DB routing errors in logs |
| Verification soak | Hold at current version under live traffic; watch error rate and per-tenant routing | Error rate within threshold for 15 minutes; control-plane `/control/tenants` returns expected fleet |
| Full rollout | Promote remaining deployments / lift any traffic gating | All target deployments report healthy via control-plane inventory |

## Verification Checks

| Signal or Check | Expected Result | Evidence or Command | Status |
|-----------------|-----------------|---------------------|--------|
| Server reachability | Server reachable at configured URL | `axon doctor` ("reachable") | [ ] |
| HTTP health | 2xx from health endpoint | `curl -fsS http://localhost:4170/health` (HTTPS if TLS enabled) | [ ] |
| Resolved auth mode | Matches intended mode (`tailscale` default; never silent `no-auth`) | `axon doctor` | [ ] |
| Control-plane inventory | Tenant list returns expected tenants with correct `db_name` | `curl -fsS http://localhost:4170/control/tenants` (REST is authoritative for `db_name`; not on GraphQL per ADR-018) | [ ] |
| Tenant routing | Data-plane requests route to the correct per-tenant DB; malformed paths rejected, not silently routed to master/default | spot-check a known tenant request; inspect server logs for routing-rejection entries | [ ] |
| Database list | Embedded/server storage enumerates databases | `axon database list` | [ ] |
| TLS (if enabled) | HTTPS handshake succeeds with the reachable hostname | `curl -fsS https://<reachable-name>:4170/health` | [ ] |

## Rollback Triggers

| Trigger | Threshold or Condition | Immediate Action | Owner |
|---------|------------------------|------------------|-------|
| Health endpoint failing | `GET /health` non-2xx for more than 5 minutes after start | Stop service, restore previous binary, `axon server start`; see runbook "Rollback Procedure" | Rollback owner |
| Default-DB / cross-tenant routing | Any data-plane request routed to master/default DB or to the wrong tenant | Stop service immediately, restore previous binary, preserve logs; escalate per runbook "Tenant Routing or Data-Sovereignty Incident" | Rollback owner |
| Unintended `no-auth` exposure | `axon doctor` shows `no-auth` without explicit opt-in | Stop service, reinstall/serve without `--no-auth`, restart authenticated | Release owner |
| Control-plane DB corruption / migration failure | Control-plane DB fails to open or migrate on start | Stop service, restore backed-up `axon-control-plane.db` + `tenants/`, restart previous version | Rollback owner |
| Port conflict on start | `axon serve` reports conflicting port, exits non-zero | Resolve the conflict or revert to previous service; do not run two instances | Release owner |

## Go or No-Go Decision

- Decision: [Go / Hold / Roll Back]
- Decision time: [timestamp]
- Notes: [exceptions, deferred checks, follow-up — e.g. version/tag caveat;
  see DECISION-2026-07-06-release-and-readiness-dispositions.md §1 for the
  current 0.4.x release target disposition]
- Follow-up owner: Release owner
