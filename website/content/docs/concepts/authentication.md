---
title: Authentication & Authorization
weight: 6
---

Axon uses **Tailscale** for identity and **role-based access control (RBAC)** for authorization. In development you can disable auth entirely with `--no-auth`.

## Auth modes

| Mode | Flag | Who can connect | Actor in audit log |
|------|------|-----------------|--------------------|
| **Tailscale** *(default)* | *(none)* | Nodes on your tailnet | Tailscale node name |
| **No-auth** | `--no-auth` | Anyone | `"anonymous"` |
| **Guest** | `--guest-role <role>` | Anyone | `"guest"` |

```bash
# Development — no Tailscale required
axon serve --no-auth --storage memory

# Guest mode — unauthenticated callers get read-only access
axon serve --guest-role read

# Production (default) — Tailscale required
axon serve
```

## Roles

Every request is assigned one of four built-in roles.

| Role | What it can do |
|------|----------------|
| `admin` | All operations: schema changes, collection drops, entity CRUD |
| `write` | Create, update, delete entities and links in any collection |
| `read` | Read entities, query, traverse graph, browse audit log |
| `none` | No access — request is rejected with 403 |

## Tailscale ACL tag mapping

When a node connects, Axon calls the Tailscale daemon (`tailscaled`) to resolve
its identity. ACL tags on the node are mapped to Axon roles:

| Tailscale ACL tag | Role |
|-------------------|------|
| `tag:axon-admin` or `tag:admin` | `admin` |
| `tag:axon-write`, `tag:axon-agent`, or `tag:write` | `write` |
| `tag:axon-read` or `tag:read` | `read` |
| *(no matching tag)* | `--tailscale-default-role` (default `read`) |

When a node carries multiple role-granting tags, the **highest-privilege role
wins** — admin beats write, write beats read.

`tag:axon-agent` is the conventional tag for automated AI agents that need
read/write access but should not be able to drop collections or modify schemas.

## Example Tailscale ACL policy

```json
{
  "tagOwners": {
    "tag:axon-server": ["autogroup:admin"],
    "tag:axon-admin":  ["autogroup:admin"],
    "tag:axon-agent":  ["autogroup:admin"],
    "tag:axon-read":   ["autogroup:admin"]
  },
  "acls": [
    {
      "action": "accept",
      "src":    ["tag:axon-admin"],
      "dst":    ["tag:axon-server:*"]
    },
    {
      "action": "accept",
      "src":    ["tag:axon-agent"],
      "dst":    ["tag:axon-server:4170,4171"]
    },
    {
      "action": "accept",
      "src":    ["autogroup:member"],
      "dst":    ["tag:axon-server:4170"]
    }
  ]
}
```

This gives admins full access, agents HTTP + gRPC access, and regular tailnet
members HTTP-only access. Nodes outside your tailnet cannot reach the server at all.

## `/auth/me` endpoint

Returns the resolved identity for the current request — useful for debugging
auth issues or populating a UI session header.

```bash
curl http://localhost:4170/auth/me
```

```json
{
  "actor": "erik-laptop",
  "role": "admin"
}
```

In `--no-auth` mode:

```json
{
  "actor": "anonymous",
  "role": "admin"
}
```

## Auth flags for `axon serve`

| Flag | Env var | Default | Description |
|------|---------|---------|-------------|
| `--no-auth` | `AXON_NO_AUTH` | `false` | Disable auth — all requests are anonymous admin |
| `--tailscale-socket` | `AXON_TAILSCALE_SOCKET` | `/run/tailscale/tailscaled.sock` | Path to the Tailscale daemon socket |
| `--tailscale-default-role` | `AXON_TAILSCALE_DEFAULT_ROLE` | `read` | Role for authenticated nodes with no recognized tag |
| `--guest-role` | `AXON_GUEST_ROLE` | *(disabled)* | Enable guest mode with this role |
| `--auth-cache-ttl-secs` | `AXON_AUTH_CACHE_TTL_SECS` | `60` | How long to cache resolved identities (seconds) |

## How auth works under the hood

1. A request arrives at the HTTP gateway (or gRPC service).
2. Auth middleware extracts the peer socket address.
3. `AuthContext::resolve_peer` checks an in-memory cache keyed by peer IP.
4. On a cache miss, it calls `GET /localapi/v0/whois?addr=<peer>` over the
   Tailscale Unix socket. This is a fast local call — no network hop.
5. ACL tags from the whois response are mapped to an Axon role.
6. The resolved `Identity` is cached (default TTL: 60 s) and injected into
   the request context. All handlers see it via `Extension<Identity>`.

If the Tailscale daemon is unreachable, requests fail with **503 Service
Unavailable** — Axon never silently bypasses auth.

## Audit log integration

The `actor` field in every audit entry reflects the authenticated identity:

| Mode | `actor` value |
|------|---------------|
| Tailscale | Tailscale node name (e.g. `"erik-laptop"`) |
| No-auth | `"anonymous"` |
| Guest | `"guest"` |

```bash
axon audit list --actor erik-laptop
```

## Troubleshooting

**`503 auth_unavailable` on every request**
: The Tailscale daemon is not running or `--tailscale-socket` points to the wrong path.
  Check with `sudo systemctl status tailscaled` and verify the socket exists at
  `/run/tailscale/tailscaled.sock`.

**`401 unauthorized` for a node on the tailnet**
: The connecting node's IP was not recognized by `whois`. Confirm the node is
  logged in to your tailnet (`tailscale status`) and that Tailscale ACLs allow
  it to reach the Axon server.

**Node connects but gets `403 forbidden`**
: The node's ACL tags don't match any recognized Axon tag, so it received the
  default role (`read`), which is insufficient for the attempted operation.
  Add `tag:axon-write` or `tag:axon-admin` to the node's ACL tags in the
  Tailscale admin console.

**Server won't start: "failed to initialize auth"**
: The Tailscale daemon socket is unreachable at startup. Either start `tailscaled`
  first, or run with `--no-auth` for local development.
