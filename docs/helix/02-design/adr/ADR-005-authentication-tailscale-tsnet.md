---
dun:
  id: ADR-005
  depends_on:
    - helix.prd
    - ADR-001
    - FEAT-005
---
# ADR-005: Authentication via Tailscale (tsnet / LocalAPI)

| Date | Status | Deciders | Related | Confidence |
|------|--------|----------|---------|------------|
| 2026-04-05 | Accepted | Erik LaBianca | FEAT-005, PRD §P1 Auth | High |
| 2026-04-12 | *Implemented* | — | FEAT-012 V1 shipped | — |

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
   permissions.

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

### Implementation Plan

**Phase 1: Identity extraction (axum middleware + tonic interceptor)**

```rust
// Pseudocode — axum middleware
async fn tailscale_auth(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    mut req: Request,
    next: Next,
) -> Response {
    let identity = tailscale_whois(addr).await;
    match identity {
        Ok(id) => {
            req.extensions_mut().insert(id);
            next.run(req).await
        }
        Err(_) => StatusCode::UNAUTHORIZED.into_response(),
    }
}
```

The same pattern works for tonic via a `tower::Layer` or `Interceptor` that
reads peer address from connection info.

**Phase 2: Per-collection authorization**

A simple permission model based on Tailscale ACL tags.  Each of the primary
tags has accepted aliases (short form and bare name):

| Primary Tag | Aliases | Permissions |
|-------------|---------|-------------|
| `tag:axon-admin` | `tag:admin` | All operations on all collections |
| `tag:axon-write` | `tag:write`, `tag:axon-agent` | Create, update, delete entities and links |
| `tag:axon-read` | `tag:read` | Read entities, query, traverse, audit log |

`tag:axon-agent` is the conventional tag for automated agent workloads that
need read/write access.  When multiple role-granting tags are present, the
highest-privilege role wins.

Permissions are checked after identity extraction, before handler execution.

**Phase 3: Per-collection ACLs (optional)**

Fine-grained: "user X can write to collection `invoices` but only read
`customers`". This would require an Axon-side permission table, not just
Tailscale tags.

### Rust Crate Options

| Crate | Version | What it does | Verdict |
|-------|---------|-------------|---------|
| `tailscale-localapi` | 0.5.0 | Calls tailscaled LocalAPI (status, whois, cert) via Unix socket | Evaluated — not used (see below) |
| `tsnet` | 0.1.0 | Embeds Tailscale dataplane via libtailscale (cgo) | Not ready — stale, requires Go toolchain |
| `tailscale-client` | 0.1.5 | Tailscale control plane API (manage devices, keys) | Not relevant for request-time auth |

**Implementation choice**: Direct HTTP/1.1 over the Tailscale Unix socket via
`hyper` + `tokio::net::UnixStream`, with manual serde deserialization of the
JSON response.  `tailscale-localapi` was evaluated but not adopted — the crate
is small and under-maintained, and the whois API surface needed is only two
endpoints (`/localapi/v0/status` for startup verification and
`/localapi/v0/whois?addr=` for per-request identity).  Owning the HTTP
client code keeps the dependency surface minimal and the behavior explicit.

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
- `tailscale-localapi` crate is small (10 stars) — may need to vendor or
  contribute fixes
- Adds ~1-2ms latency per request for the whois Unix socket call (mitigable
  with a short TTL cache keyed by peer IP)

**Risks**:
- Tailscale LocalAPI is not a documented stability guarantee — the whois
  endpoint could change. Mitigation: pin tailscaled version in deployment.
- If tailscaled is down, all requests fail auth. Mitigation: health check
  includes tailscaled connectivity; consider a bypass flag for development.

## Operational Notes

- **Development mode**: `--no-auth` (or `AXON_NO_AUTH=1`) skips auth entirely.
  All requests get `actor="anonymous"`, `role=Admin`.
- **Guest mode**: `--guest-role <role>` (or `AXON_GUEST_ROLE=read`) allows
  unauthenticated access with a fixed role. Useful when Tailscale is not
  available but unrestricted open access is undesirable.
- **Server startup**: `AuthContext::verify()` contacts the tailscaled socket
  before accepting connections. If the socket is unreachable the server exits
  with an error rather than starting in a broken state.
- **Caching**: Whois results are cached per peer IP with a configurable TTL
  (`--auth-cache-ttl-secs`, default 60 s). The cache is an in-memory
  `HashMap<IpAddr, CachedIdentity>` behind a `RwLock`; reads are lock-free
  on cache hit. Cache is never actively evicted — stale entries are ignored
  on next access if their TTL has elapsed.
- **Actor name**: `Identity.actor` is the Tailscale `ComputedName` field
  (short node name), falling back to `Hostinfo.Hostname`, then the first
  label of the FQDN. This is what appears in audit log entries.
- **Embedded mode**: The CLI runs in-process; no network boundary, so no auth.
- **Both transports**: HTTP uses axum middleware (`authenticate_http_request`);
  gRPC uses `AxonServiceImpl::authorize` — both delegate to `AuthContext::resolve_peer`.

## Validation

- Auth middleware returns 401 for connections from outside the tailnet
- Whois correctly maps peer IP to user identity
- Audit log entries carry the Tailscale user login as actor
- ACL tags correctly gate write vs read operations
- `--no-auth` flag works for local development
- Both HTTP and gRPC transports get identity injection
- Whois cache reduces p99 auth overhead to <1ms after warmup
