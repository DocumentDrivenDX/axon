---
ddx:
  id: FEAT-027
  parking_lot: true
  depends_on:
    - helix.prd
  review:
    self_hash: 6afe43852f2f84b6136fca386b8b6a9b8e0b895015c9fbbd8c9c2b025d26416d
    deps:
      helix.prd: d87a9cbc61d7abb53d32d8c675cc74c63fd9502e953c0ebee44285efde51df1f
    reviewed_at: "2026-06-14T03:52:45Z"
---
# Feature Specification: FEAT-027 — Git Mirror

**Feature ID**: FEAT-027
**Status**: draft
**Priority**: P2
**Owner**: Core Team
**Covered PRD Subsystem(s)**: Audit, Change Capture, and Repair (as a
change-feed consumer)
**Covered PRD Requirements**: Deferred — tracked in
`docs/helix/parking-lot.md`. No FR-n is allocated; the mirror consumes the
FR-18 change feed.
**Cross-Subsystem Rationale**: None — single subsystem.
**FR Prefix**: GIT

> This feature is deferred. The parking-lot entry ("Git Mirror —
> git-visible projection of collection state") records the rationale and
> revisit trigger. This spec is retained as the desired future state for
> when the trigger fires.

## Overview

A Git Mirror projects a collection's live state into a git repository,
committing each mutation as it occurs. Every entity is a file; every
change is a commit. Git history becomes a human-navigable, externally
accessible view of the collection's audit trail.

This is not a replacement for Axon's immutable audit log (FEAT-003) — the
audit log remains the authoritative, tamper-evident record. Git Mirror is
a projection: a familiar interface for developers, reviewers, and
compliance workflows that already speak git. The mirror consumes the
change feed (FEAT-021) like any other CDC consumer.

## Ideal Future State

A developer enables mirroring on a collection with one command and from
then on reviews entity changes with the tools they already use: `git log`
to see who changed what, `git diff` to see field-level changes, `git
blame` to trace a value to its mutation, and PR-style review of an agent's
batch output. A compliance officer clones the mirror repo without an Axon
client. Every commit links back to the authoritative audit record, and the
mirror recovers from network or credential failures on its own.

## Problem Statement

- **Current situation**: Entity history is queryable via the Axon API
  only.
- **Pain points**: No external audit-trail consumers without an Axon
  client; agent output is not reviewable via standard developer tooling;
  compliance exports require custom tooling.
- **Desired outcome**: Collection state mirrored to git automatically;
  standard git tools work against complete collection history.

## Functional Areas

| Area | User question or job | Feature responsibility |
|------|----------------------|------------------------|
| Mirror configuration and lifecycle | "Turn mirroring on/off and see whether it is healthy" | Per-collection enable/disable, status reporting, credential handling |
| Projection content | "Find an entity in the repo and read its change as a diff" | File layout (shard strategies), file formats, commit and snapshot conventions |
| Sync and recovery | "Trust the mirror to keep up and to heal itself" | Realtime/batched sync, transactional atomicity, failure recovery and resumption |

## Requirements

> **Surface note**: configuration fields, commit-trailer keys, and
> snapshot metadata are described here at the behavioral level only. The
> normative config/trailer/snapshot surface will be defined in a Contract
> artifact when this feature is scheduled.

### Functional Requirements by Area

#### Mirror Configuration and Lifecycle

- **GIT-01**. Operators MUST be able to enable, inspect, and disable a
  mirror per collection via the API and CLI. Mirror configuration is
  stored separately from the collection schema (same principle as
  FEAT-026 templates: projection concerns do not belong in the validation
  schema) and covers at least: remote repository, file format, shard
  strategy, sync mode, target branch, commit signing, and a credential
  reference.
- **GIT-02**. Disabling a mirror MUST stop mirroring without deleting the
  remote repository; mirror status MUST report the last mirrored audit
  position, current lag, and any failure state.
- **GIT-03**. Mirror credentials (SSH keys, access tokens) MUST be stored
  in Axon's encrypted credential store and referenced by name; credential
  material never appears in mirror configuration in plaintext.

#### Projection Content

- **GIT-04**. The mirror MUST organize entity files under a per-collection
  directory using a configurable shard strategy: `flat` (all files in the
  collection root), `id_prefix` (directory from a configurable-length ID
  prefix; stable paths, default), `index_field` (directory from a declared
  secondary-index field value, FEAT-013; human-navigable, files move when
  the field changes), and `hash` (two-level hash prefix; uniform, not
  human-navigable).
- **GIT-05**. When an `index_field` shard value changes, the file move
  (remove + add) MUST occur in the same commit, with old and new paths
  recorded in the commit message for traceability.
- **GIT-06**. The JSON file format MUST be round-trippable: entity data
  plus system metadata (ID, version, collection, timestamps, actors)
  sufficient to reconstruct the entity. The markdown format (requires a
  FEAT-026 template) is informational and not round-trippable; both
  formats MAY be enabled simultaneously (two files per entity).
- **GIT-07**. Each Axon mutation MUST produce one git commit; a
  multi-entity transaction MUST produce exactly one commit (atomic in both
  Axon and git). The commit author reflects the Axon actor (agent or user
  identity).
- **GIT-08**. Every commit message MUST carry machine-readable trailers
  linking the commit to the authoritative audit record: at minimum the
  audit ID(s), entity/collection identity, version, operation, actor,
  transaction ID, and timestamp. (Normative trailer keys and format: see
  Surface note.)
- **GIT-09**. Enabling a mirror on a collection with existing entities
  MUST produce an initial snapshot: all current entities committed as a
  single commit identifying the as-of point; subsequent mutations are
  committed incrementally from that boundary.
- **GIT-10**. Deleting an entity MUST remove its file in a commit; git
  history is the deletion record (no tombstone directory).

#### Sync and Recovery

- **GIT-11**. The mirror MUST support two sync modes: `realtime` (commit
  and push per transaction) and `batched` (accumulate and commit/push on a
  time window or size threshold, whichever first). Both modes preserve
  one-commit-per-transaction atomicity.
- **GIT-12**. The mirror MUST be read-only as a projection: pushes by
  consumers to the mirror repository are never reconciled back into Axon,
  and the mirror worker never force-pushes. On a non-fast-forward push,
  the worker MUST publish its state to a recovery branch and emit a
  divergence event to the audit log for operator resolution.
- **GIT-13**. After a failed push, the mirror MUST retry with exponential
  backoff; after a configurable number of consecutive failures (default
  10) in realtime mode, it MUST fall back to batched mode and emit a
  "mirror stuck" audit event. Mirror failures never block or fail Axon
  writes, and mirror unavailability never blocks Axon startup.
- **GIT-14**. On restart, the mirror MUST resume from the last mirrored
  audit position by replaying the audit log/change feed — without
  re-snapshotting. Operators MUST be able to force a full re-snapshot.

### Non-Functional Requirements

- **Realtime mode latency overhead**: < 200 ms added to write p99 for
  remote repositories; < 10 ms for local repositories.
- **Batched mode latency overhead**: < 1 ms (fire-and-forget enqueue).
- **Throughput**: ≥ 500 mutations/sec in batched mode; realtime mode is
  bounded by git push throughput.
- **Security**: credentials encrypted at rest (GIT-03).
- **Operational bound**: Axon enforces no repository size limit; repo
  lifecycle (archival, `git gc`) is the operator's responsibility.

## User Stories

- [US-140 — Enable Git Mirror on a Collection](../user-stories/US-140-enable-git-mirror-on-a-collection.md)
- [US-141 — Entity Changes Appear as Git Commits](../user-stories/US-141-entity-changes-appear-as-git-commits.md)
- [US-142 — Shard Strategy Organises the Repository](../user-stories/US-142-shard-strategy-organises-the-repository.md)
- [US-143 — Mirror Resumes After Failure](../user-stories/US-143-mirror-resumes-after-failure.md)

## Edge Cases and Error Handling

- **High-write-rate collections**: git cannot absorb thousands of commits
  per second; Axon emits a warning when realtime mode is enabled on a
  collection averaging > 10 mutations/sec and recommends batched mode.
- **Very large entities**: entities > 10 MB produce a warning at mirror
  enable; large-file storage (LFS) is not supported in V1.
- **Entity ID characters**: IDs are sanitized for filesystem use (invalid
  filename characters percent-encoded); UUIDv7 IDs are safe as-is.
- **Collection rename**: the mirror repo's directory structure keeps the
  original collection name until the mirror is reset.
- **Concurrent mirror workers**: exactly one mirror worker per collection;
  multi-instance deployments elect a single worker.
- **Markdown format without a template**: enabling `markdown` format on a
  collection with no FEAT-026 template fails with a descriptive error.
- **Both formats enabled**: each entity produces two files; the JSON file
  is authoritative, the markdown file informational.

## Success Metrics

- Mirror lag (realtime mode) < 200 ms p99 for remote repositories; lag
  (batched mode) ≤ the configured batch window.
- `git log` on a mirrored collection shows complete entity history with
  readable diffs, and every commit resolves to its audit record via
  trailers.
- The mirror recovers from transient failures without operator
  intervention.

## Constraints and Assumptions

- The audit log is authoritative; the mirror is a derived projection and
  may lawfully lag or be rebuilt at any time.
- The mirror is built as a change-feed consumer (FEAT-021) rather than a
  second hook into the write path.
- Deferred scheduling: this spec describes desired future state; no
  implementation is planned until the parking-lot revisit trigger fires
  (adopter demand for git-visible spec mirroring).

## Dependencies

- **Other features**:
  - FEAT-003 (Audit Log) — commit trailers reference audit IDs; resumption
    replays from the audit log.
  - FEAT-004 (Entity Operations) — the mirror observes create, update,
    patch, and delete operations.
  - FEAT-021 (Change Feeds) — the mirror is a CDC consumer.
  - FEAT-026 (Markdown Templates) — required only when markdown format is
    configured.
  - FEAT-013 (Secondary Indexes) — required only for the `index_field`
    shard strategy.
- **External services**: git remotes (SSH/HTTPS); normative config and
  trailer surface in a future Contract artifact.
- **PRD requirements**: none allocated — deferred (parking lot); consumes
  FR-18 change feeds.

## Out of Scope

- **Two-way sync**: the mirror is read-only; pushes to the mirror repo are
  not reconciled into Axon.
- **Git LFS**: large file support deferred.
- **Per-field exclusion**: no mechanism to exclude specific fields (e.g.,
  PII) from the mirror; use a separate collection for sensitive data.
- **Branch-per-entity**: all entities live on one branch.
- **PR-based write approval**: merging a PR to trigger an Axon write is
  not supported; the mirror is a projection, not an input.
- **Multiple remotes**: one remote per collection mirror in V1.
- **Webhooks on mirror events**: deferred.
