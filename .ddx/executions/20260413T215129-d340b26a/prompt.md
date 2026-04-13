# Execute Bead

You are running inside DDx's isolated execution worktree for this bead.
Your job is to make a best-effort attempt at the work described in the bead's Goals and Description, then commit the result. Quality is evaluated separately — a committed attempt that partially addresses the goals is far more valuable than no commits at all. Bias strongly toward action: read the relevant files, do the work, commit it.

## Bead
- ID: `axon-f1c3183c`
- Title: Clarify transaction rollback behavior on create/delete source txs and PIT all-or-nothing semantics (ADR-015 gaps)
- Labels: bug, p1, rollback, adr, docs
- Base revision: `77e06b88fc00a2033c25f5e75b3b7e86f4cf1826`
- Execution bundle: `.ddx/executions/20260413T215129-d340b26a`

## Description
Opus review of the rollback implementation (commits bbc88b1, 5b1eb02, 91a6e6b, e22ebdb) identified two specification gaps not addressed by ADR-015:

GAP 1 — Transaction rollback refuses create/delete source txs: The current transaction rollback implementation (axon-api/src/handler.rs, ~RollbackTransaction handler) refuses to roll back transactions that include entity creates or deletes — it only handles update operations. ADR-015 does not explicitly document this constraint. Required change: (a) if this is intentional, add a clear code comment AND document it in ADR-015 under Known Constraints; (b) if this is a gap, file a separate bead to implement create/delete rollback support.

GAP 2 — PIT rollback is best-effort, not all-or-nothing: The point-in-time collection rollback handler (5b1eb02) rolls back entities individually. If some entities fail to roll back (e.g., OCC conflicts), the collection ends up in a partial state with no indication to the caller of which entities succeeded or failed. ADR-015 says compensating writes but does not specify atomicity guarantees. Required change: update ADR-015 to explicitly state whether PIT rollback is all-or-nothing or best-effort; update the HTTP response for POST /collections/{name}/rollback to include a partial_failures field listing which entity IDs failed to roll back.

Files: docs/helix/02-design/adr/ADR-015-rollback-recovery.md, crates/axon-api/src/handler.rs (RollbackTransaction and PIT rollback handlers), crates/axon-api/src/request.rs (response types)

## Acceptance Criteria
ADR-015 explicitly documents create/delete rollback constraint (or files a separate bead for full support); PIT rollback response includes partial_failures: Vec<EntityId> (may be empty on full success); cargo test --workspace passes; no ambiguity remains about rollback atomicity guarantees

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
