# Agent Instructions

This project uses **bd** (beads) for issue tracking. Run `bd onboard` to get started.

This repository also contains the canonical HELIX workflow definitions under
`workflows/helix/`. If your task touches HELIX actions, skills, execution
control, or Beads integration, treat those docs as the source of truth for the
workflow contract.

## Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work atomically
bd close <id>         # Complete work
bd doctor             # Check local Beads workspace health
bd dolt remote -v     # Show configured Dolt remote, if any
bash tests/helix-cli.sh  # Deterministic HELIX wrapper tests
helix run             # Run bounded HELIX execution loop
helix check           # Decide next HELIX action
```

## HELIX Workflow Notes

When working on HELIX itself in this repo:

- top-level overview: `workflows/helix/README.md`
- operator loop and automation: `workflows/helix/EXECUTION.md`
- upstream Beads integration: `workflows/helix/BEADS.md`
- command summary: `workflows/helix/REFERENCE.md`

Key rules:

- Use upstream `bd`; do not invent HELIX-specific bead files.
- For ready-work detection, use `bd ready`, not `bd list --ready`.
- Keep `implementation` single-shot and bounded to one bead per run.
- Use `check` when the ready queue drains to decide whether to implement,
  align, backfill, wait, ask for guidance, or stop.
- Keep alignment and backfill as separate cross-phase actions:
  - `workflows/helix/actions/reconcile-alignment.md`
  - `workflows/helix/actions/backfill-helix-docs.md`
- Quality ratchets are documented in `workflows/helix/ratchets.md`. Ratchet
  enforcement scripts and floor fixtures belong in adopting projects, not in
  this repo. This repo defines the pattern and the integration points in
  action prompts and enforcers.

## HELIX CLI

This repo now ships a small HELIX wrapper CLI:

- script source: `scripts/helix`
- local launcher install: `scripts/install-local-skills.sh`
- installed command: `~/.local/bin/helix`

Useful commands:

```bash
helix run --review-every 5
helix implement
helix check repo
helix align auth
helix backfill repo
```

`helix run` is the preferred operator loop. It:

- loops only while true ready HELIX execution work exists
- executes one bounded implementation pass at a time
- runs `check` when the queue drains
- can run periodic alignment reviews

Do not replace this with an unconditional `while true` loop.

## Testing Requirements for HELIX Changes

If you change any of the following, run the HELIX wrapper harness:

- `scripts/helix`
- `scripts/install-local-skills.sh`
- `workflows/helix/actions/check.md`
- `workflows/helix/actions/implementation.md`
- `workflows/helix/actions/reconcile-alignment.md`
- `workflows/helix/EXECUTION.md`
- `workflows/helix/BEADS.md`
- other docs that materially change the HELIX execution contract

Required checks:

```bash
bash tests/helix-cli.sh
git diff --check
```

The wrapper tests are intentionally deterministic:

- they use temporary git workspaces
- they stub `bd`, `codex`, and `claude`
- they verify queue draining, periodic alignment, auto-alignment, dry-run
  output, and launcher installation behavior

Prefer these deterministic tests over live Codex or Claude calls when
validating wrapper behavior.

## Non-Interactive Shell Commands

**ALWAYS use non-interactive flags** with file operations to avoid hanging on confirmation prompts.

Shell commands like `cp`, `mv`, and `rm` may be aliased to include `-i` (interactive) mode on some systems, causing the agent to hang indefinitely waiting for y/n input.

**Use these forms instead:**
```bash
# Force overwrite without prompting
cp -f source dest           # NOT: cp source dest
mv -f source dest           # NOT: mv source dest
rm -f file                  # NOT: rm file

# For recursive operations
rm -rf directory            # NOT: rm -r directory
cp -rf source dest          # NOT: cp -r source dest
```

**Other commands that may prompt:**
- `scp` - use `-o BatchMode=yes` for non-interactive
- `ssh` - use `-o BatchMode=yes` to fail instead of prompting
- `apt-get` - use `-y` flag
- `brew` - use `HOMEBREW_NO_AUTO_UPDATE=1` env var

<!-- BEGIN BEADS INTEGRATION -->
## Issue Tracking with bd (beads)

**IMPORTANT**: This project uses **bd (beads)** for ALL issue tracking. Do NOT use markdown TODOs, task lists, or other tracking methods.

### Why bd?

- Dependency-aware: Track blockers and relationships between issues
- Version-controlled: Built on Dolt with cell-level merge
- Agent-optimized: JSON output, ready work detection, discovered-from links
- Prevents duplicate tracking systems and confusion

### Quick Start

**Check for ready work:**

```bash
bd ready --json
```

**Create new issues:**

```bash
bd create "Issue title" --description="Detailed context" -t bug|feature|task -p 0-4 --json
bd create "Issue title" --description="What this issue is about" -p 1 --deps discovered-from:bd-123 --json
```

**Claim and update:**

```bash
bd update <id> --claim --json
bd update bd-42 --priority 1 --json
```

**Complete work:**

```bash
bd close bd-42 --reason "Completed" --json
```

### Issue Types

- `bug` - Something broken
- `feature` - New functionality
- `task` - Work item (tests, docs, refactoring)
- `epic` - Large feature with subtasks
- `chore` - Maintenance (dependencies, tooling)

### Priorities

- `0` - Critical (security, data loss, broken builds)
- `1` - High (major features, important bugs)
- `2` - Medium (default, nice-to-have)
- `3` - Low (polish, optimization)
- `4` - Backlog (future ideas)

### Workflow for AI Agents

1. **Check ready work**: `bd ready` shows unblocked issues
2. **Claim your task atomically**: `bd update <id> --claim`
3. **Work on it**: Implement, test, document
4. **Discover new work?** Create linked issue:
   - `bd create "Found bug" --description="Details about what was found" -p 1 --deps discovered-from:<parent-id>`
5. **Complete**: `bd close <id> --reason "Done"`

### Local DB and Dolt Remotes

- The repo-local Beads DB under `.beads/dolt` is the authoritative working database.
- A Dolt remote is optional.
- If a project uses a shared Dolt remote for coordination, it must be a real shared remote, not a machine-local `file://` path.
- Do NOT use machine-local or CIFS/SMB-backed `file://` remotes as a hot coordination path.
- A `file://` remote, if a project allows one at all, must be documented as manual backup only, not routine agent sync.
- If no proper shared remote exists, prefer local-only operation over a broken `file://` remote.

### Git Export / Import

bd automatically exports and imports issue JSON for git workflows:

- Exports to `.beads/issues.jsonl` after changes (5s debounce)
- Imports from JSONL when newer (e.g., after `git pull`)
- No manual export/import needed for the JSONL mirror.

This JSONL mirror is not the same thing as Dolt remote sync.

### Remote Health and Repair

When Beads errors look like corruption, distinguish local DB health from remote topology:

```bash
bd doctor
(cd .beads/dolt && dolt status && dolt fsck && dolt remote -v)
```

- If `.beads/dolt` is healthy but the configured remote is machine-local or on CIFS/SMB, fix or remove the remote instead of treating the local DB as corrupted.
- To repair a stale or bad remote, repoint it to a proper shared remote or leave remote sync unset:

```bash
bd dolt remote remove origin
bd dolt remote add origin <shared-dolt-remote>
bd dolt pull
```

### Important Rules

- ✅ Use bd for ALL task tracking
- ✅ Always use `--json` flag for programmatic use
- ✅ Link discovered work with `discovered-from` dependencies
- ✅ Check `bd ready` before asking "what should I work on?"
- ❌ Do NOT create markdown TODO lists
- ❌ Do NOT use external issue trackers
- ❌ Do NOT duplicate tracking systems

For more details, see README.md and docs/QUICKSTART.md.

## Landing the Plane (Session Completion)

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git push` succeeds.

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** - Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **PUSH TO GIT REMOTE** - This is MANDATORY:
   ```bash
   git pull --rebase
   git push
   git status  # MUST show "up to date with origin"
   ```
5. **OPTIONAL SHARED DOLT REMOTE SYNC** - Only if the project explicitly uses a proper shared Dolt remote:
   ```bash
   bd dolt remote -v
   bd dolt pull
   bd dolt push
   ```
   Skip this when no proper shared remote exists, and never use a machine-local or CIFS/SMB-backed `file://` path as the normal coordination backend.
6. **Clean up** - Clear stashes, prune remote branches
7. **Verify** - All changes committed AND pushed
8. **Hand off** - Provide context for next session

**CRITICAL RULES:**
- Work is NOT complete until `git push` succeeds
- NEVER stop before pushing - that leaves work stranded locally
- NEVER say "ready to push when you are" - YOU must push
- If push fails, resolve and retry until it succeeds
- Do not treat a machine-local or CIFS/SMB-backed `file://` Dolt remote as a normal shared coordination backend

<!-- END BEADS INTEGRATION -->
