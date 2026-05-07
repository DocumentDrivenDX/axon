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

<!-- DDX-AGENTS:START -->
<!-- Managed by ddx init / ddx update. Edit outside these markers. -->

# DDx

This project uses [DDx](https://github.com/DocumentDrivenDX/ddx) for
document-driven development. Use the `ddx` skill for beads, work,
review, agents, and status — every skills-compatible harness (Claude
Code, OpenAI Codex, Gemini CLI, etc.) discovers it from
`.claude/skills/ddx/` and `.agents/skills/ddx/`.

## Files to commit

After modifying any of these paths, stage and commit them:

- `.ddx/beads.jsonl` — work item tracker
- `.ddx/config.yaml` — project configuration
- `.agents/skills/ddx/` — the ddx skill (shipped by ddx init)
- `.claude/skills/ddx/` — same skill, Claude Code location
- `docs/` — project documentation and artifacts

## Conventions

- Use `ddx bead` for work tracking (not custom issue files).
- Documents with `ddx:` frontmatter are tracked in the document graph.
- Run `ddx doctor` to check environment health.
- Run `ddx doc stale` to find documents needing review.

## Merge Policy

Branches containing `ddx agent execute-bead` or `ddx work` commits
carry a per-attempt execution audit trail:

- `chore: update tracker (execute-bead <TIMESTAMP>)` — attempt heartbeats
- `Merge bead <bead-id> attempt <TIMESTAMP>- into <branch>` — successful lands
- `feat|fix|...: ... [ddx-<id>]` — substantive bead work

Bead records store `closing_commit_sha` pointers into this history. Any
SHA rewrite breaks the trail. **Never squash, rebase, or filter** these
branches. Use only:

- `git merge --ff-only` when the target is a strict ancestor, or
- `git merge --no-ff` when divergence exists

Forbidden on execute-bead branches: `gh pr merge --squash`,
`gh pr merge --rebase`, `git rebase -i` with fixup/squash/drop,
`git filter-branch`, `git filter-repo`, and `git commit --amend` on
any commit already in the trail.
<!-- DDX-AGENTS:END -->
