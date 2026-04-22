---
ddx:
  id: FEAT-027
  depends_on:
    - helix.prd
    - FEAT-001
    - FEAT-003
    - FEAT-004
    - FEAT-021
---
# Feature Specification: FEAT-027 - Git Mirror

**Feature ID**: FEAT-027
**Status**: Draft
**Priority**: P2
**Owner**: Core Team
**Created**: 2026-04-07
**Updated**: 2026-04-07

## Overview

A Git Mirror projects a collection's live state into a git repository,
committing each mutation as it occurs. Every entity is a file; every
change is a commit. Git history becomes a human-navigable, externally
accessible view of the collection's audit trail.

This is not a replacement for Axon's immutable audit log (FEAT-003) —
the audit log remains the authoritative, tamper-evident record. Git
Mirror is a projection: a familiar interface for developers, reviewers,
and compliance workflows that already speak git.

## Problem Statement

Axon's audit log captures everything, but querying it requires Axon.
Teams that review agent output, manage approval workflows, or need
compliance exports want to use tools they already have — `git log`,
`git diff`, GitHub PR reviews, GitLab CI checks, external diff tooling.

Storing entity state in git makes data changes reviewable as code
changes. An agent that modifies 50 entities in a batch can have that
batch reviewed as a PR before it's promoted. A compliance officer can
run `git blame` to trace who changed a field. An external audit tool
can clone the repo without an Axon client.

- **Current situation**: Entity history is queryable via Axon API only
- **Pain points**: No external audit trail consumers; agent output not
  reviewable via standard developer tooling; compliance exports require
  custom tooling
- **Desired outcome**: Collection state mirrored to git automatically;
  standard git tools work against collection history

## Requirements

### Functional Requirements

#### Mirror Configuration

Git Mirror is configured per collection as metadata, stored separately
from `CollectionSchema` (same principle as FEAT-026 markdown templates
— presentation/projection concerns don't belong in the validation
schema). Configuration is managed via the Axon API and CLI:

```bash
axon collection mirror add invoices \
  --repo git@github.com:org/invoices-mirror.git \
  --shard-strategy id_prefix \
  --format json \
  --sync realtime
```

**Mirror config fields:**

| Field | Required | Default | Description |
|---|---|---|---|
| `repo` | Yes | — | Git remote URL (SSH or HTTPS) |
| `format` | No | `json` | `json` or `markdown` (markdown requires FEAT-026 template) |
| `shard.strategy` | No | `id_prefix` | See Sharding Strategies below |
| `shard.field` | No | — | For `index_field` strategy: the index field to shard on |
| `shard.prefix_length` | No | `8` | For `id_prefix`: number of ID chars used as directory prefix |
| `sync` | No | `realtime` | `realtime` or `batched` |
| `batch_window` | No | `30s` | For `batched`: max time between commits |
| `branch` | No | `main` | Target branch |
| `signed_commits` | No | `false` | GPG-sign commits using Axon server key |
| `credentials` | No | — | SSH key name or credential reference |

#### Sharding Strategies

All entities live under a collection directory at the root:

```
invoices/
  <shard-dirs>/
    <entity-id>.json
```

**`flat`** — all entities in the collection root directory. Simple;
breaks at ~10K entities (filesystem and git performance).

```
invoices/
  01J3ABCDEF.json
  01J3ABCDEG.json
```

**`id_prefix`** (default) — first N characters of the entity ID as
a directory. Stable paths; entities never move; scales to millions.

```
invoices/            # prefix_length: 4
  01J3/
    01J3ABCDEF.json
  01J4/
    01J4XXXXXX.json
```

**`index_field`** — shard by the value of a declared index field.
Human-navigable; entities move between directories when the field
changes. Best for collections with a stable, low-cardinality shard
field (e.g., `status` with 4-6 values, `region` with fixed values).
The field must be declared as a secondary index on the collection
(FEAT-013).

```
invoices/            # field: status
  approved/
    01J3ABCDEF.json
  draft/
    01J4XXXXXX.json
```

When an entity's shard field changes, the file moves: the old path
is `git rm`'d and the new path is `git add`'d in the same commit.
`git log --follow` loses continuity across the move; the commit
message records the old and new paths for traceability.

**`hash`** — two-level hash prefix (first 2 / next 2 hex chars of
SHA1 of entity ID). Uniform distribution; not human-navigable.
Mirrors git's own object store layout.

```
invoices/
  a8/f3/01J3ABCDEF.json
```

#### File Format

**JSON** (default): The entity's `data` field as a formatted JSON
document, plus a `_meta` block with system fields:

```json
{
  "_meta": {
    "id": "01J3ABCDEF",
    "version": 5,
    "collection": "invoices",
    "created_at": "2026-04-07T10:00:00Z",
    "updated_at": "2026-04-07T14:22:11Z",
    "created_by": "agent:invoice-processor",
    "updated_by": "agent:approver"
  },
  "invoice_number": "INV-2026-0042",
  "vendor": "Acme Corp",
  "status": "approved",
  "amount": { "value": 312.50, "currency": "USD" },
  "line_items": [...]
}
```

JSON format is always round-trippable: the file fully reconstructs
the entity.

**Markdown** (requires FEAT-026 template): The entity rendered via
the collection's markdown template. Human-readable diffs; not
round-trippable (use JSON if you need to reconstruct entities from
the mirror). Both formats can be enabled simultaneously (two files
per entity: `{id}.json` and `{id}.md`).

#### Commit Format

Each mutation produces one commit. Multi-entity transactions produce
one commit for the entire transaction (atomic in both Axon and git).

**Commit message format:**

```
<operation> <collection>/<entity-id>: <summary>

Axon-Collection: invoices
Axon-Entity: 01J3ABCDEF
Axon-Version: 5
Axon-Operation: update
Axon-Actor: agent:invoice-processor
Axon-Transaction: txn-a8f3e2b1
Axon-Audit-Id: aud-29f81c44
Axon-Timestamp: 2026-04-07T14:22:11.483Z
```

**Summary** is derived from the operation:
- Create: `create invoices/01J3ABCDEF: new invoice INV-2026-0042`
- Update: `update invoices/01J3ABCDEF: version 4 → 5`
- Patch: `patch invoices/01J3ABCDEF: version 4 → 5`
- Delete: `delete invoices/01J3ABCDEF`

For multi-entity transactions:
```
transaction txn-a8f3e2b1: 3 entities in invoices

Axon-Collection: invoices
Axon-Transaction: txn-a8f3e2b1
Axon-Actor: agent:batch-processor
Axon-Entity-Count: 3
Axon-Audit-Ids: aud-29f81c44,aud-30a92d55,aud-31b03e66
Axon-Timestamp: 2026-04-07T14:22:11.483Z
```

The `Axon-Audit-Id` trailer links every commit back to the
authoritative audit log entry.

#### Sync Modes

**Realtime**: After each committed Axon transaction, the mirror
worker writes and pushes a git commit before acknowledging the
Axon write to the caller.

- Latency impact: adds git commit + push time to write path
- Consistency: git mirror is always up-to-date with Axon state
- Risk: if git push fails, Axon write succeeds but mirror is behind
  (see Error Handling)

**Batched**: Mirror worker accumulates mutations and commits/pushes
on a time window (default 30s) or size threshold (default 100
mutations), whichever comes first.

- Latency impact: none on Axon write path
- Consistency: mirror may lag by up to `batch_window`
- Risk: if Axon process crashes, up to one window of mutations
  may not be mirrored (audit log is still intact)

Both modes guarantee transaction atomicity in git: a multi-entity
transaction is always a single git commit regardless of mode.

#### Initial Snapshot

When a mirror is first enabled on a collection with existing entities,
Axon creates an initial snapshot commit:

```
snapshot invoices: initial mirror of 2,847 entities

Axon-Collection: invoices
Axon-Operation: snapshot
Axon-Entity-Count: 2847
Axon-Snapshot-As-Of: 2026-04-07T14:00:00.000Z
```

All current entities are committed in a single snapshot commit.
Subsequent mutations are committed incrementally. The snapshot
represents the collection state at a specific point in time
(`Axon-Snapshot-As-Of`); the audit log covers all history before
that point.

#### Deletion Handling

When an entity is deleted, its file is removed (`git rm`). The file
is gone from the working tree but preserved in git history — `git log
-- invoices/01J3/01J3ABCDEF.json` recovers the full history including
the deletion commit.

There is no `_deleted/` directory in V1. Git history is the deletion
record.

#### Mirror is Read-Only

The mirror repo is a projection. Direct pushes to the mirror are
not reconciled back to Axon. The mirror has no write path. If a
consumer pushes to the mirror repo, Axon is unaffected. The next
Axon mutation will commit on top of the consumer's push (creating a
divergent history on the mirror branch) or fail if fast-forward is
not possible (see Error Handling).

### Non-Functional Requirements

- **Realtime mode latency overhead**: < 200ms added to write p99 for
  collections with remote git repos. Local repos (for testing/CI): < 10ms
- **Batched mode latency overhead**: < 1ms (fire-and-forget enqueue)
- **Throughput**: Mirror worker handles up to 500 mutations/sec in
  batched mode. Realtime mode is limited by git push throughput
  (typically 10-50 commits/sec for remote repos)
- **Repo size**: No size limit enforced by Axon. Large entities or
  high-mutation-rate collections will produce large repos over time.
  Operators are responsible for repo lifecycle (archival, git gc)
- **Credential security**: SSH keys and access tokens are stored in
  Axon's credential store (encrypted at rest), never in the mirror
  config plaintext

## Architecture

### Component: `axon-mirror` (or module in `axon-server`)

The mirror worker lives in `axon-server` as a per-collection
background task, or in a dedicated `axon-mirror` crate if the
implementation grows complex.

```
MirrorWorker
  - collection: CollectionId
  - config: MirrorConfig
  - repo: git2::Repository    -- local clone of mirror repo
  - pending: VecDeque<MirrorEvent>  -- for batched mode
```

The worker subscribes to the collection's change feed (same mechanism
as FEAT-021 CDC consumers). Each change feed event is:

1. Mapped to file operations (add/modify/remove paths)
2. Written to the local git working tree
3. Staged and committed
4. Pushed to the remote

The local clone is maintained as a cache. On startup, the worker
clones (or fetches) the remote. On failure, it re-clones.

### Storage for Mirror Config

```
collection_mirrors:
    PK: collection_id
    repo:             text     NOT NULL
    format:           text     NOT NULL  DEFAULT 'json'
    shard_strategy:   text     NOT NULL  DEFAULT 'id_prefix'
    shard_field:      text
    prefix_length:    int      DEFAULT 8
    sync_mode:        text     NOT NULL  DEFAULT 'realtime'
    batch_window_s:   int      DEFAULT 30
    branch:           text     NOT NULL  DEFAULT 'main'
    signed_commits:   bool     DEFAULT false
    credential_ref:   text
    enabled:          bool     DEFAULT true
    last_commit:      text               -- last Axon audit-id mirrored
    last_mirrored_at: timestamp
    created_at:       timestamp
    updated_at:       timestamp

    FK: collection_id → collections
```

`last_commit` enables resumption after crash: on restart, the worker
reads all audit log entries after `last_commit` and replays them.

### Error Handling

**Git push fails** (network, auth, force-push conflict):
- Realtime mode: Axon write has already committed. Mirror worker
  queues the failed push for retry with exponential backoff. The
  `last_mirrored_at` lag is surfaced in collection metadata so
  operators can see the mirror is behind.
- After N retries (configurable, default 10): worker emits a
  `mirror.stuck` event to the audit log and disables realtime mode
  automatically, falling back to batched. Operator must investigate
  and re-enable.

**Push diverged** (consumer pushed to mirror branch):
- Worker detects non-fast-forward on push. It does not force-push.
- It creates a new branch `axon/recovery-{timestamp}` and pushes
  there, then emits a `mirror.diverged` audit event.
- Operator resolves manually (typically: reset mirror branch to
  Axon's version).

**Repo unavailable at startup**:
- Mirror worker retries connection with backoff.
- Does not block Axon startup or write path.
- Logs a `mirror.unavailable` event every 5 minutes until resolved.

### Credential Management

SSH keys and access tokens are stored in a `mirror_credentials` table
(encrypted at rest using Axon's key management). The `credential_ref`
field in mirror config references a credential by name.

```bash
axon collection mirror credential add github-deploy \
  --type ssh-key \
  --key ~/.ssh/axon_mirror_ed25519
```

## User Stories

### Story US-078: Enable Git Mirror on a Collection [FEAT-027]

**As a** developer or operator
**I want** to mirror a collection to a git repo
**So that** entity changes are reviewable with standard git tooling

**Acceptance Criteria:**
- [ ] `axon collection mirror add <collection> --repo <url>` enables
      mirroring and triggers initial snapshot commit
- [ ] Initial snapshot commits all current entities in one commit
      with `Axon-Operation: snapshot` trailer
- [ ] `axon collection mirror status <collection>` shows last mirrored
      audit-id, lag, and any errors
- [ ] `axon collection mirror remove <collection>` disables mirroring
      (does not delete the remote repo)
- [ ] Mirror config is retrievable via API

### Story US-079: Entity Changes Appear as Git Commits [FEAT-027]

**As a** developer reviewing entity changes
**I want** each Axon mutation to appear as a git commit
**So that** I can use `git log`, `git diff`, and `git blame` on
entity history

**Acceptance Criteria:**
- [ ] Creating an entity produces a git commit adding `{shard}/{id}.json`
- [ ] Updating or patching an entity produces a git commit modifying
      the file
- [ ] Deleting an entity produces a git commit removing the file
- [ ] Each commit message includes `Axon-Audit-Id` trailer linking
      to the audit log
- [ ] Multi-entity transactions produce exactly one git commit
- [ ] Commit author is the Axon actor (agent ID or user ID)
- [ ] `git diff` between two commits shows field-level changes in
      readable JSON format

### Story US-080: Shard Strategy Organises the Repository [FEAT-027]

**As a** developer navigating the mirror repo
**I want** entities organised in a predictable directory structure
**So that** I can find entities without knowing the full entity ID

**Acceptance Criteria:**
- [ ] `id_prefix` strategy places entities under `{prefix}/{id}.json`
      with configurable prefix length (default 8 chars)
- [ ] `index_field` strategy places entities under `{field_value}/{id}.json`
- [ ] When shard field changes, `index_field` strategy moves the file
      (rm + add in same commit) with old/new paths in commit message
- [ ] `flat` strategy places all entities directly in collection root
- [ ] Shard strategy is set at mirror creation time; changing it
      requires disabling and re-enabling the mirror (full re-snapshot)

### Story US-081: Mirror Resumes After Failure [FEAT-027]

**As an** operator
**I want** the mirror to recover automatically after transient failures
**So that** temporary network or auth issues don't leave the mirror
permanently behind

**Acceptance Criteria:**
- [ ] After a failed push, worker retries with exponential backoff
- [ ] On Axon restart, mirror worker resumes from `last_commit`
      without re-snapshotting
- [ ] After 10 consecutive push failures, realtime mode falls back
      to batched and a `mirror.stuck` audit event is emitted
- [ ] `axon collection mirror status` shows lag and failure count
- [ ] Operator can force a full re-snapshot with
      `axon collection mirror reset <collection>`

## Edge Cases and Error Handling

- **High-write-rate collections**: Git is not designed for thousands
  of commits per second. Operators should use batched mode for
  high-write-rate collections. Axon does not enforce this but emits a
  warning if realtime mode is enabled on a collection averaging > 10
  mutations/sec
- **Very large entities**: Entities > 10MB produce a warning on mirror
  enable. Git LFS is not supported in V1
- **Entity ID characters**: Entity IDs are sanitised for filesystem
  use (any character that is invalid in a filename is percent-encoded).
  UUIDv7 IDs are safe as-is
- **Collection rename**: If a collection is renamed, the mirror repo
  is not renamed. The directory structure inside the repo uses the
  original collection name unless the mirror is reset
- **Concurrent mirror workers**: Only one mirror worker per collection.
  If multiple Axon server instances run (server mode), the mirror
  worker is elected via a lock in the database
- **Markdown format without template**: If `format: markdown` is
  configured but the collection has no markdown template (FEAT-026),
  mirror enable fails with a descriptive error
- **Both formats**: If `format: json+markdown` is configured, each
  entity produces two files (`{id}.json` and `{id}.md`). The markdown
  file is informational; the JSON file is authoritative

## Example: Invoice Mirror with `index_field` Sharding

```
invoices/
  approved/
    01J3ABCDEF.json    ← approved invoice, full JSON
  draft/
    01J4XXXXXX.json
  paid/
    01J2YYYYYY.json
```

When INV-2026-0043 is approved (status: draft → approved):

```bash
$ git log --oneline -3
a1b2c3d  update invoices/01J4XXXXXX: version 3 → 4
         (draft/01J4XXXXXX.json → approved/01J4XXXXXX.json)
...

$ git diff a1b2c3d^..a1b2c3d
diff --git a/invoices/draft/01J4XXXXXX.json b/invoices/draft/01J4XXXXXX.json
deleted file mode 100644
diff --git a/invoices/approved/01J4XXXXXX.json b/invoices/approved/01J4XXXXXX.json
new file mode 100644
+  "status": "approved",
+  "approver": "jane@example.com",
-  "status": "draft",
```

## Dependencies

- **FEAT-003** (Audit Log): Commit trailers reference audit IDs;
  worker replays from audit log on restart
- **FEAT-004** (Entity Operations): Mirror subscribes to create,
  update, patch, delete operations
- **FEAT-021** (Change Feeds): Mirror worker is a CDC consumer;
  shares the change feed subscription mechanism
- **FEAT-026** (Markdown Templates): Required only if `format:
  markdown` is configured

## Out of Scope

- **Two-way sync**: Mirror is read-only. Pushes to the mirror repo
  are not reconciled into Axon
- **Git LFS**: Large file support deferred
- **Per-field gitignore**: No mechanism to exclude specific fields
  from the mirror (e.g., PII). If field-level exclusion is needed,
  use a separate collection for sensitive data
- **Branch-per-entity**: Each entity as a branch is not supported.
  All entities live on one branch
- **PR-based write approval**: Merging a PR to trigger an Axon write
  is not supported in V1. The mirror is a projection, not an input
- **Multiple remotes**: One remote per collection mirror in V1
- **Webhooks on mirror events**: Deferred

## Success Metrics

- Mirror lag (realtime mode) < 200ms p99 for remote repos
- Mirror lag (batched mode) ≤ configured `batch_window`
- `git log` on a mirrored collection shows complete entity history
  with readable diffs
- Mirror resumes automatically after transient failures without
  operator intervention

## Traceability

### Related Artifacts
- **Parent PRD Section**: "Not Scheduled" — Git backend (architecturally
  compatible, not prioritised). This spec frames it as a mirror/projection
  rather than a storage backend, making it tractable as P2
- **User Stories**: US-078, US-079, US-080, US-081
- **Test Suites**: `tests/FEAT-027/`
- **Implementation**: `crates/axon-server/src/mirror/` or
  `crates/axon-mirror/`

### Feature Dependencies
- **Depends On**: FEAT-003, FEAT-004, FEAT-021
- **Optional**: FEAT-026 (markdown format only)
- **Depended By**: None in V1
