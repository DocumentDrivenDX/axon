---
ddx:
  id: ADR-005
  depends_on:
    - helix.prd
    - ADR-001
    - FEAT-005
---
# ADR-005: Authentication via Tailscale LocalAPI

| Date | Status | Deciders | Related | Confidence |
|------|--------|----------|---------|------------|
| 2026-04-05 | Accepted | Erik LaBianca | FEAT-005, FEAT-012, PRD §P1 Auth | High |

## Context

Axon's PRD lists authentication and authorization as P1 (Phase 3). The server
currently has no auth — all HTTP and gRPC endpoints are open. We need an auth
mechanism that:

- Works for both HTTP (axum) and gRPC (tonic) transport
- Provides real user/node identity (not just API keys)
- Integrates with existing infrastructure (the team uses Tailscale)
- Doesn't require building a custom auth system from scratch
- Supports network-layer ACLs for defense-in-depth

| Aspect | Description |
|--------|-------------|
| Problem | Axon server endpoints are unauthenticated; need identity + authorization for production |
| Current State | No auth middleware. Server binds to 0.0.0.0 on both ports |
| Requirements | Identity on every request (who), per-collection authorization (what), both transports |
| Decision Drivers | Avoid building/operating a custom credential system; reuse existing tailnet identity; must cover gRPC (HTTP/2), which rules out header-injecting proxies |

## Decision

Use **Tailscale** as the authentication and authorization layer via the
**LocalAPI whois** mechanism, with network-layer ACLs for coarse-grained
access control.

### How It Works

```
┌─────────────┐     Tailscale      ┌──────────────────────┐
│   Client     │───── tunnel ──────▶│   Axon Server        │
│ (agent/CLI)  │  100.x.x.x:3000  │  ┌──────────────┐    │
└─────────────┘                     │  │ Auth Middleware│   │
                                    │  │  1. Peer IP    │   │
                                    │  │  2. whois()    │   │
                                    │  │  3. Identity   │   │
                                    │  └──────────────┘    │
                                    │         │            │
                                    │         ▼            │
                                    │  ┌──────────────┐    │
                                    │  │  tailscaled   │   │
                                    │  │  Unix socket   │   │
                                    │  └──────────────┘    │
                                    └──────────────────────┘
```

1. Axon server binds to the Tailscale interface (100.x.x.x) or verifies
   that incoming connections originate from Tailscale IP ranges.
2. On each request, auth middleware extracts the peer/source IP address.
3. Middleware calls `GET /localapi/v0/whois?addr={peer_ip}:{port}` on the
   tailscaled Unix socket (`/run/tailscale/tailscaled.sock`).
4. The whois response provides:
   - **UserProfile**: login email, display name
   - **Node**: hostname, OS, Tailscale ACL tags, capabilities
5. This identity is injected into the request context (axum extensions /
   tonic metadata) and available to all handlers.
6. Authorization checks use ACL tags and/or email for per-collection
   permissions. Role mapping: `tag:axon-admin` → all operations,
   `tag:axon-write` (alias `tag:axon-agent`) → create/update/delete,
   `tag:axon-read` → read/query/traverse/audit. Highest-privilege tag wins.

### Network-Layer ACLs (Defense-in-Depth)

Tailscale ACLs are enforced at the network layer before traffic reaches Axon.
Example ACL policy:

```json
{
  "acls": [
    {"action": "accept", "src": ["tag:axon-admin"], "dst": ["tag:axon-server:*"]},
    {"action": "accept", "src": ["tag:axon-agent"], "dst": ["tag:axon-server:3000,50051"]},
    {"action": "accept", "src": ["autogroup:member"],  "dst": ["tag:axon-server:3000"]}
  ]
}
```

This means agents (tagged `axon-agent`) can reach both HTTP and gRPC, admins
get full access, and regular tailnet members can only use HTTP. Connections
from outside the tailnet never reach the server.

### Client Implementation

**Direct HTTP/1.1 over the tailscaled Unix socket** via `hyper` +
`tokio::net::UnixStream`, with manual serde deserialization. The
`tailscale-localapi` crate (0.5.0) was evaluated but not adopted — it is small
and under-maintained, and only two endpoints are needed (`/localapi/v0/status`
for startup verification, `/localapi/v0/whois?addr=` for per-request identity).
Owning the client keeps the dependency surface minimal and the behavior
explicit.

## Alternatives Considered

### A. Tailscale Serve (reverse proxy with identity headers)

Tailscale Serve injects `Tailscale-User-Login` / `Tailscale-User-Name` headers.

**Rejected because**:
- Only works for HTTP/1.1 — does not support gRPC (HTTP/2)
- Header spoofing risk if server doesn't bind exclusively to localhost
- Adds an extra proxy hop

### B. Embedded tsnet via libtailscale FFI

Each Axon instance gets its own Tailscale identity with independent ACLs.

**Rejected because**:
- Rust crate (`tsnet` 0.1.0) is experimental and stale (March 2023)
- Requires Go toolchain at build time (cgo)
- Significant build complexity for a Rust project
- May revisit if the Rust binding matures

### C. Custom JWT / API Key Auth

Build a custom auth system with JWT tokens or API keys.

**Rejected because**:
- Requires building and maintaining a full auth system (key management,
  rotation, revocation, token validation)
- Doesn't integrate with existing tailnet identity
- Duplicates what Tailscale already provides

### D. mTLS with Tailscale-Provisioned Certificates

Use Tailscale's cert provisioning for mutual TLS authentication.

**Deferred** — could complement LocalAPI whois for environments where
tailscaled is not co-located with the server. More complex to set up.

## Consequences

**Positive**:
- Zero custom auth code for identity — Tailscale provides it
- Network-layer ACLs provide defense-in-depth for free
- Real user identity (email, node name) available on every request
- Works for both HTTP and gRPC transports
- Audit log entries get meaningful actor identities (email instead of API key)
- Agents on the tailnet are authenticated by their node identity

**Negative**:
- Requires tailscaled running on the server host
- Not usable outside a tailnet (public API access requires a different auth
  layer — API keys or OAuth — as a future extension)
- Adds ~1-2ms latency per request for the whois Unix socket call (mitigated
  by a per-peer-IP TTL cache, default 60s)

**Operational addendum** (implemented 2026-04-12 with FEAT-012 V1; condensed
from the original implementation plan and operational notes):
- Identity flows through both transports: axum middleware for HTTP, an
  authorize hook for gRPC, both delegating to a shared `AuthContext`.
- Startup verifies tailscaled connectivity and fails fast if unreachable.
- Escape hatches for non-tailnet environments: `--no-auth` (development;
  `actor="anonymous"`, role Admin) and `--guest-role <role>` (unauthenticated
  access with a fixed role).
- Audit actor names derive from the Tailscale node's computed name with
  hostname/FQDN fallbacks.
- Embedded mode (CLI in-process) has no network boundary and performs no auth.

## Risks

| Risk | Prob | Impact | Mitigation |
|------|------|--------|------------|
| Tailscale LocalAPI has no documented stability guarantee — whois endpoint could change | Low | High | Pin tailscaled version in deployment; only two endpoints consumed |
| tailscaled outage fails auth for all requests | Medium | High | Health check includes tailscaled connectivity; guest mode provides a controlled degraded path |
| Whois latency on every request | Medium | Low | Per-peer-IP cache with TTL reduces p99 auth overhead to <1ms after warmup |

## Validation

| Success Metric | Review Trigger |
|----------------|----------------|
| 401 returned for connections from outside the tailnet; whois maps peer IP to identity on both transports | Any unauthenticated request reaching a handler |
| Audit entries carry the Tailscale user login as actor; ACL tags gate write vs read correctly | Any actor-attribution gap or privilege escalation |
| Whois cache keeps p99 auth overhead <1ms after warmup | Auth overhead regression beyond 2ms |
| LocalAPI whois remains compatible across tailscaled upgrades | A tailscaled release changes the whois surface — reconsider mTLS (Alternative D) or vendoring |

## Supersession

- **Supersedes**: None
- **Superseded by**: None (ADR-018 layers the tenant/user credential model on top of this identity mechanism; the Tailscale LocalAPI decision stands)

## Concern Impact

- **security-owasp**: This ADR is the authentication leg of the concern — identity fully delegated to Tailscale (no password storage); it also anchors the project overrides for TLS termination, CSP, CSRF, and CORS in `docs/helix/01-frame/concerns.md`.
- **rust-cargo**: Adds `hyper` + Unix-socket client code rather than the unmaintained `tailscale-localapi`/`tsnet` crates — a deliberate dependency-surface choice.

## References

- [FEAT-012: Authorization](../../01-frame/features/FEAT-012-authorization.md)
- [ADR-006: Admin UI](ADR-006-admin-ui-sveltekit-bun.md) — UI inherits this identity
- [ADR-018: Tenant, User, and Credential Model](ADR-018-tenant-user-credential-model.md)
- Tailscale LocalAPI whois: `/localapi/v0/whois` on the tailscaled Unix socket
