# Axon — ADR-018 Implementation Execute Loop

Started: 2026-04-14
Primary harness: `agent` (vidar-coder / qwen/qwen3-coder-next)
Review harness: `claude` smart profile (claude-opus-4-6)
Fallback chain on failure: agent/vidar-coder → claude/sonnet → claude/opus

## Bead queue (28 total, 20 implementation + 8 epics)

| Phase | Bead | ID | Status | Primary | Reviewer | Notes |
|---|---|---|---|---|---|---|
| A1 | auth-schema | axon-0478a1de | rerun-sonnet | qwen→REQUEST_CHANGES | opus | [iter 1] e2b0d5d2; [iter 2] sonnet running |
| A2 | auth-core-types | axon-43ed769b | blocked(A1) | | | |
| B1 | auth-jwt-pipeline | axon-dccbdf73 | blocked(A2,A1) | | | |
| B2 | auth-error-mapping | axon-f68e7f20 | blocked(B1) | | | |
| B3 | auth-federation | axon-ceaadfcb | blocked(B1) | | | |
| B4 | auth-membership | axon-dab58328 | blocked(B1) | | | |
| C1 | control-tenants | axon-c6908e78 | blocked(B3,B4) | | | |
| C2 | control-databases | axon-df98e262 | blocked(C1) | | | |
| C3 | control-credentials | axon-906b527a | blocked(B1,B2,B4,C1) | | | |
| D1 | path-router | axon-3a2e4c4e | blocked(B1,A2) | | | |
| D2 | database-router | axon-75bdbd6c | blocked(D1,C2) | | | |
| D3 | remove-legacy-routes | axon-130f129f | blocked(D1,D2,B2) | | | |
| D4 | graphql-path-nesting | axon-93c7e048 | blocked(D1,D3,B1) | | | |
| E1 | default-tenant-bootstrap | axon-e77f0e5b | blocked(A1,B3,D1,C1) | | | |
| E2 | no-auth-namespace | axon-f9e7f523 | blocked(B1,D1) | | | |
| F1 | audit-attribution | axon-18fb7517 | blocked(A1,A2,B1,C1) | | | |
| G1 | ts-sdk-tenant-database-api | axon-8dbbad75 | blocked(C3,D3,D4) | | | |
| G2 | ts-sdk-error-enum | axon-d3c827c9 | blocked(B2,G1) | | | |
| H1 | ui-tenant-database-picker | axon-2e7e0d61 | blocked(D3,G1) | | | |
| H2 | ui-credential-management | axon-0cbd48ce | blocked(C3,G1,H1) | | | |

## Feedback log

### Iteration 0 — harness discovery
- bragi down (502); switched primary to vidar-coder (qwen/qwen3-coder-next)
- Set default_provider: vidar-coder in ~/.config/agent/config.yaml
- Review harness: `ddx agent run --harness claude --profile smart` (claude-opus-4-6)
- `ddx bead` has native `dep add` — used it to wire 39 dependency edges across the 20 children
- `ddx bead ready` correctly surfaces only A1 (axon-0478a1de) after dep wiring

### A1 — iteration 1 (qwen/qwen3-coder-next via agent harness)
- **Result**: merged at e2b0d5d2 (~14 min wall-clock)
- **Review** (opus): REQUEST_CHANGES
  - Missing round-trip test (AC explicit)
  - Zero Postgres tests (AC requires both backends; testcontainers pattern already present in this crate)
  - Scope creep: `audit_retention_policies` table invented; belongs to F1
  - ADR-018 internal inconsistency `external_id` vs `subject` (my doc gap from hardening pass, not the agent)
- **Doc fix committed**: 1d15850 normalizes to `external_id` everywhere
- **Bead notes + description + AC updated** to carry gap list
- **Iteration 2**: claude-sonnet-4-6 via claude harness (fallback per user's escalation rule)

### Automation gaps observed (candidates for ~/Projects/ddx beads)
1. **Review loop is not integrated with execute-loop**. execute-loop marks the bead closed on merge; a post-merge review step is manual. Needed: `ddx agent execute-loop --post-review <harness>` that runs a review agent against the merge commit + AC and can reopen on REQUEST_CHANGES.
2. **Reopening a bead for fallback** is a manual 3-step dance (update status, update notes, update description, update AC). Needed: `ddx bead reopen <id> --reason "<text>" --append-notes` that atomically flips status and appends review context.
3. **Fallback chain not expressed declaratively**. I had to manually re-invoke execute-loop with a different harness. Needed: `ddx agent execute-loop --fallback-chain agent/qwen,claude/sonnet,claude/opus` or a per-bead escalation policy.
4. **Provider selection is tangled**. `default_provider` in `~/.config/agent/config.yaml` is the only clean way to route `--harness agent` to a specific host/model combo. Needed: `ddx agent execute-loop --harness agent --provider bragi` or similar passthrough.
5. **Review prompt template is ad-hoc**. I wrote /tmp/review-A1.md by hand. Needed: a `ddx bead review <id>` command that templates the AC + commit diff + governing docs into a review prompt automatically.
6. **Output wrapping on claude harness is verbose**. `ddx agent run --harness claude` emits the full session JSONL to stdout; the final text has to be extracted manually. Needed: `--output plain` or `--extract result-text`.
