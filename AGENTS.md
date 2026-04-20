# Axon — Agent Instructions

This file provides guidance for AI agents (Codex, Claude, etc.) working in this repository.

## What This Project Is

Axon is a **Rust** Cargo workspace implementing an agent-native, auditable, schema-first transactional data store. See `CLAUDE.md` for the full layout and `docs/helix/` for governing documents.

## How to Get Oriented

1. Read `docs/helix/00-discover/product-vision.md` for the mission.
2. Read `docs/helix/01-frame/prd.md` for requirements.
3. Read `docs/helix/01-frame/technical-requirements.md` for architecture constraints.
4. Run `ddx bead ready --json` to see pending work items.

## Build and Test

```bash
cargo check
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

All four commands must pass before committing.

## Issue Management

Use `ddx bead` subcommands:

- `ddx bead ready --json` — list ready issues
- `ddx bead show <id>` — show details and acceptance criteria
- `ddx bead update <id> --claim` — mark in-progress before starting work
- `ddx bead close <id>` — mark done after verification

Always re-read the issue immediately before claiming and before closing.

Before closing a bead, verify there is durable evidence for the closure:
a commit referencing the bead id, an execution bundle under `.ddx/executions/`,
or an explicit notes entry documenting why implementation was deferred or why
the bead is tracker-only. If a review step is malformed, empty, over-large, or
errors before producing a valid verdict with rationale, do not close the bead;
leave it open/in progress or set a retry path.

## Commit Format

```
<type>(<scope>): <short description> [<issue-id>]
```

Example: `feat(storage): add memory storage adapter [axon-25033ab0]`

## Constraints

- No `unwrap()` in library code.
- Clippy must be clean with `-D warnings`.
- Tests are truth — do not skip or modify tests to make them pass.
- Authority order: Vision > PRD > Technical Requirements > Features > Tests > Code.
