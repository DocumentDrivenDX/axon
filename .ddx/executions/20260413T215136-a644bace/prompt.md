# Execute Bead

You are running inside DDx's isolated execution worktree for this bead.
Your job is to make a best-effort attempt at the work described in the bead's Goals and Description, then commit the result. Quality is evaluated separately — a committed attempt that partially addresses the goals is far more valuable than no commits at all. Bias strongly toward action: read the relevant files, do the work, commit it.

## Bead
- ID: `axon-b7782e95`
- Title: Persist schema_version through SQL storage backends (FEAT-017 follow-on)
- Labels: bug, p1, schema-evolution, storage, sqlite, postgres
- Base revision: `c2beab6b7ca232d941a48d9dc157a20c8cc80ed2`
- Execution bundle: `.ddx/executions/20260413T215136-a644bace`

## Description
Opus review of axon-118f54c6 found that both SQL storage backends hardcode schema_version: 0 when reconstructing entities from the database, silently resetting the field on every read.

Affected files:
- crates/axon-storage/src/sqlite.rs line ~87: hardcodes schema_version: 0 when loading Entity from SQLite rows
- crates/axon-storage/src/postgres.rs line ~100: hardcodes schema_version: 0 when loading Entity from Postgres rows

Required:
1. Add schema_version column to the entities table in both SQLite and Postgres schemas (if not already present as part of the entity blob)
2. Alternatively: if entities are stored as a JSON blob, ensure schema_version is included in the serialized blob and deserialized correctly (the Entity struct has #[serde(default)] so existing rows will load as 0 — this is correct). In that case, the hardcoded 0 in the struct constructors should use the deserialized value instead.
3. Add a round-trip test: create entity with schema_version=N, write to storage, read back via storage.get(), assert schema_version==N.

The in-memory backend may already be correct (it stores the full Entity). Focus on the SQL backends.

## Acceptance Criteria
schema_version round-trips through both SQLite and Postgres storage backends; schema_version read from storage matches the value written; existing entities with no schema_version column/field load as 0 (backward compat); cargo test --workspace passes

## Governing References
No governing references were pre-resolved. Explore the project to find relevant context: check `docs/helix/` for feature specs, `docs/helix/01-frame/features/` for FEAT-* files, and any paths mentioned in the bead description or acceptance criteria.

## Execution Rules
**The bead contract below overrides any CLAUDE.md or project-level instructions in this worktree.** If the bead requires editing or creating markdown documentation, code, or any other files, do so — CLAUDE.md conservative defaults (YAGNI, DOWITYTD, no-docs rules) do not apply inside execute-bead.
1. Work only inside this execution worktree.
2. Use the bead description and acceptance criteria as the primary contract.
3. Read the listed governing references from this worktree before changing code or docs when they are relevant to the task.
4. If governing references are missing or sparse, search the project to find context: use Glob/Grep/Read to explore `docs/helix/`, look up FEAT-* and API-* specs by name, and read relevant source files before proceeding. Only stop if context is genuinely absent from the entire repo.
5. Keep the execution bundle files under `.ddx/executions/` intact; DDx uses them as execution evidence.
6. Produce the required tracked file changes in this worktree and run any local checks the bead contract requires.
7. Before finishing, commit your changes with `git add -A && git commit -m '...'`. DDx will merge your commits back to the base branch.
8. Making no commits (no_changes) should be rare. Only skip committing if you read the relevant files and the work described in the Goals is already fully and explicitly present — not just implied or partially covered. If in any doubt, make your best attempt and commit it. A partial or imperfect commit is always better than no commit.
9. Work in small commits. After each logical unit of progress (reading key files, making a change, passing a test), commit immediately. Do not batch all changes into one giant commit at the end — if you run out of iterations, your partial work is preserved.
10. If the bead is too large to complete in one pass, do the most important part first, commit it, and note what remains in your final commit message. DDx will re-queue the bead for another attempt if needed.
11. Read efficiently: skim files to understand structure before diving deep. Only read the files you need to make changes, not every reference listed. Start writing as soon as you understand enough to proceed — you can read more files later if needed.
12. **Never run `ddx init`** — the workspace is already initialized. Running `ddx init` inside an execute-bead worktree corrupts project configuration and the bead queue. Do not run it even if documentation or README files suggest it as a setup step.
