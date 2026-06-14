---
ddx:
  id: US-137
  review:
    self_hash: ddc81da77d8675401acc194c5db7c1e94ddd03aa18029b9f1dee23f901b01339
    deps: {}
    reviewed_at: "2026-06-14T04:25:45Z"
---

# US-137: Stream Changes Without Kafka

**Feature**: FEAT-021 — Change Feeds (CDC)
**Feature Requirements**: CDC-14, CDC-15, CDC-16
**PRD Requirements**: FR-18, FR-31
**Priority**: P1
**Status**: Draft

## Story

**As a** developer in a non-Kafka environment
**I want** CDC events written to files or streamed over HTTP
**So that** I can consume Axon changes without Kafka infrastructure

## Context

Renumbered from US-077 (collision with FEAT-009). Many adopters —
especially embedded, air-gapped, or small deployments — have no Kafka.
FEAT-021 makes Kafka optional (CDC-14): file and HTTP streaming (SSE)
sinks deliver the same envelope with the same cursor semantics (CDC-15,
CDC-16). Envelope and cursor fields are normative in CONTRACT-006.

## Walkthrough

1. The developer enables the file sink (and leaves Kafka disabled) via
   the CDC configuration (CONTRACT-006).
2. Entity mutations are appended to JSONL files in the standard envelope,
   with rotation per configuration.
3. The developer also connects an HTTP streaming client and receives the
   same events live.
4. After a restart, both sinks resume from their persisted cursors and
   the developer's consumers deduplicate any re-emitted events by audit
   cursor.

## Acceptance Criteria

- [ ] **US-137-AC1** — Given the file sink is enabled, when mutations
      commit, then events are written as JSONL in the CONTRACT-006
      envelope with configurable rotation.
- [ ] **US-137-AC2** — Given the SSE sink is enabled, when mutations
      commit, then connected HTTP clients receive the events as a live
      stream (shared delivery semantics with GraphQL subscriptions).
- [ ] **US-137-AC3** — Given Kafka is disabled, when file and SSE sinks
      are enabled, then both operate fully without any Kafka
      infrastructure.
- [ ] **US-137-AC4** — Given any enabled sink, when the same mutation is
      observed on each, then all sinks emit an identical envelope for it.
- [ ] **US-137-AC5** — Given a producer restart, when sinks resume, then
      each sink resumes from its own persisted per-collection cursor.
- [ ] **US-137-AC6** — Given file and SSE events, when compared with Kafka
      events for the same mutation, then they preserve the same cursor
      fields.

## Edge Cases

- **File sink disk full**: tailing pauses for the file sink; entity
  writes are unaffected; the sink catches up after space is freed.
- **SSE client disconnect**: the client reconnects and resumes from its
  last cursor; missed events are re-delivered from the audit log.
- **Multiple sinks at different positions**: cursors are independent per
  sink; a slow file sink never holds back the SSE stream.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| JSONL output | US-137-AC1 | File sink on, rotation 10MB | Create 3 entities | 3 envelope lines appended to current JSONL file |
| Kafka-free operation | US-137-AC3 | No Kafka configured | Enable file + SSE sinks; mutate | Both sinks deliver; no Kafka connection attempted |
| Envelope parity | US-137-AC4 | File + SSE sinks on | One update | Identical envelope (incl. cursor fields) on both sinks |
| Restart resume | US-137-AC5 | File sink cursor at audit 90; 10 more events; restart | Restart producer | Events after 90 (re-)emitted; consumer dedups by cursor |

## Dependencies

- **Stories**: US-130 (envelope/emission semantics)
- **Feature Spec**: FEAT-021
- **Feature Requirements**: CDC-14, CDC-15, CDC-16
- **PRD Requirements**: FR-18, FR-31
- **External**: CONTRACT-006 (envelope, cursor fields, sink config);
  FEAT-015 (shared SSE delivery semantics)

## Out of Scope

- Additional transports (NATS, Pulsar, Redis Streams); Kafka-specific
  behavior (US-130); registry (US-135).

## Review Checklist

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
