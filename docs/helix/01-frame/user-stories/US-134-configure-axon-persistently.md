---
ddx:
  id: US-134
  review:
    self_hash: b7a2970c953d369fc819c94d911f6d9bd51ebb71784bc5fe1be4b0f3181728ef
    deps: {}
    reviewed_at: "2026-06-15T00:35:16Z"
---

# US-134: Configure Axon persistently

**Feature**: FEAT-028 — Unified Binary & Service Management
**Feature Requirements**: BIN-04, BIN-07
**PRD Requirements**: FR-23, FR-24
**Priority**: P1
**Status**: Draft

## Story

**As a** developer (Ava) customizing Axon
**I want** a TOML config file at a standard location
**So that** I don't have to pass flags every time

## Context

Renumbered from US-075 (collision with FEAT-009 "Schema-declared named
query"). Settings scattered across flags and environment variables make
installations irreproducible. This story exercises BIN-04 and BIN-07:
layered configuration with deterministic precedence, plus commands to
print the resolved configuration and the config file location. The TOML
schema, env-var naming, and precedence order are normative in
CONTRACT-008.

## Walkthrough

1. Ava edits the config file at the standard config directory to change
   the server port.
2. `axon serve` picks the value up from the file.
3. In CI she overrides the same setting with the corresponding
   environment variable, which wins over the file.
4. On a one-off run she passes a CLI flag, which wins over the
   environment variable.
5. `axon config show` prints the resolved configuration; `axon config
   path` prints where the file lives.

## Acceptance Criteria

- [ ] **US-134-AC1** — Given a config file at the standard
      (CONTRACT-008) location, when Axon starts, then values from the
      file are applied over compiled defaults.
- [ ] **US-134-AC2** — Given a setting present in both the config file
      and its corresponding environment variable, when Axon resolves
      configuration, then the environment variable wins.
- [ ] **US-134-AC3** — Given a setting present as an environment variable
      and a CLI flag, when Axon resolves configuration, then the CLI flag
      wins.
- [ ] **US-134-AC4** — Given any configuration state, when the
      resolved-config command runs, then it prints the effective
      configuration after all layers are applied.
- [ ] **US-134-AC5** — Given any configuration state, when the
      config-path command runs, then it prints the config file location
      in use.

## Edge Cases

- **Malformed TOML**: startup fails with the line number; no silent
  fallback.
- **Unknown keys in the file**: surfaced as a warning rather than
  silently ignored, so typos are discoverable.
- **Environment variable with invalid value type**: a clear error names
  the variable and expected type.

## Test Scenarios

| Scenario | AC ID | Input / State | Action | Expected Result |
|----------|-------|---------------|--------|-----------------|
| File over defaults | US-134-AC1 | File sets non-default port | `axon serve` | Server listens on the file's port |
| Env over file | US-134-AC2 | File port A; env var port B | `axon serve` | Server listens on B |
| Flag over env | US-134-AC3 | Env port B; flag port C | `axon serve` with flag | Server listens on C |
| Resolved view | US-134-AC4 | All three layers set | `axon config show` | Effective values reflect precedence |

## Dependencies

- **Stories**: US-126 (serve consumes the configuration)
- **Feature Spec**: FEAT-028
- **Feature Requirements**: BIN-04, BIN-07
- **PRD Requirements**: FR-23, FR-24
- **External**: CONTRACT-008 (TOML schema, env naming, precedence order,
  standard paths)

## Out of Scope

- The specific key set (owned by CONTRACT-008); diagnostics beyond config
  printing (US-127); per-tenant configuration.

## Review Checklist

- [ ] Stored as its own file `US-NNN-<slug>.md` (one file per story — never a single monolithic `user-stories.md`)
- [ ] Covers one persona completing one goal, demonstrable end-to-end in a single flow
- [ ] Links to its parent `FEAT-NNN` and names the PRD `FR-n` it covers
- [ ] Every acceptance criterion is independently testable and carries a stable `US-NNN-ACm` ID
- [ ] Walkthrough traces a complete path from trigger to outcome; at least one edge case documented
- [ ] No exact API/CLI/event/schema/config/telemetry/adapter surface is defined inline; normative surface links to Contract artifacts
