# Alignment Review: AR-2026-04-05f — Final V1 Pre-Ship

**Date**: 2026-04-05
**Scope**: Full repository — final pre-ship review
**Reviewer**: Claude (automated)
**Epic**: axon-114ad5fb
**Prior review**: AR-2026-04-05e (G-E01 gRPC server startup fixed)

## Context

AR-2026-04-05e identified one critical gap (gRPC server not started in binary),
which was fixed in commit c4508d9. This review is the final pre-ship audit,
checking for any remaining issues before V1 can ship.

**334 passing tests. 0 open tracker issues.**

## Gap Register

### G-F01: HTTP and gRPC Servers Use Separate Handler Instances [P0, CRITICAL]

- **Planning**: The server should present a unified data store across both protocols
- **Implementation**: `main.rs` creates two independent `AxonHandler<MemoryStorageAdapter>` instances — one for HTTP, one for gRPC. Data written via HTTP is invisible to gRPC and vice versa.
- **Classification**: CRITICAL BUG — protocols serve disconnected data stores
- **Resolution**: Share a single `Arc<Mutex<AxonHandler>>` between both servers, or pass the same handler to `AxonServiceImpl::from_handler()`.

### G-F02: CLI Missing `schema` Subcommand [P2]

- **Planning**: FEAT-005 CLI Requirements: "`axon schema show|validate`"
- **Implementation**: Schema operations exist via `collection describe` and `--schema` on `collection create`, but no dedicated `schema` subcommand.
- **Classification**: GAP — minor, operations available through alternative paths
- **Resolution**: Add `axon schema show <collection>` as alias for the schema portion of `collection describe`

### G-F03: CLI Missing YAML Output [P2]

- **Planning**: FEAT-005: "Output formats: Human-readable table (default), JSON, YAML"
- **Implementation**: `OutputFormat` only supports `Table` and `Json`
- **Classification**: GAP — minor
- **Resolution**: Add `serde_yaml` and `Yaml` variant to `OutputFormat`

### G-F04: CLI Missing `config` Subcommand [P2]

- **Planning**: FEAT-005: "`axon config` for connection settings, defaults"
- **Implementation**: No config subcommand; config is only via CLI flags
- **Classification**: GAP — minor, V1 embedded mode doesn't need connection settings
- **Resolution**: Defer to server-mode CLI evolution (P2)

### G-F05: CLI Missing `entity query` with Filter Syntax [P2]

- **Planning**: FEAT-005: "`axon entity query <collection> --filter "status=pending"`"
- **Implementation**: `entity list` exists but has no `--filter` flag
- **Classification**: GAP — minor, filtering works via API/SDK
- **Resolution**: Add `--filter` flag parsing to `entity list` or add `entity query` subcommand

### All Audit Paths Verified [SATISFIED]

All 8 mutation operations produce audit entries: `create_entity`, `update_entity`,
`delete_entity`, `create_link`, `delete_link`, `create_collection`, `drop_collection`,
`put_schema`. No bypass paths found.

## Execution Issues

| Gap | Priority | Action |
|-----|----------|--------|
| G-F01: Shared handler between HTTP/gRPC | P0 | Create issue — 5-line fix in main.rs |
| G-F02–G-F05: CLI polish | P2 | Defer to post-V1 iteration |

## V1 Launch Assessment

**BLOCKED on G-F01.** The HTTP and gRPC servers are disconnected — writing via
one protocol is invisible to the other. This is a ~5-line fix (share the handler
instance). All other gaps are P2 CLI polish that can ship in a follow-up.

Once G-F01 is fixed, V1 is shippable.
