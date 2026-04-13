# Execute Bead

You are running inside DDx's isolated execution worktree for this bead.
Your job is to make a best-effort attempt at the work described in the bead's Goals and Description, then commit the result. Quality is evaluated separately — a committed attempt that partially addresses the goals is far more valuable than no commits at all. Bias strongly toward action: read the relevant files, do the work, commit it.

## Bead
- ID: `axon-375db29e`
- Title: Implement entity-level and transaction-level rollback (FEAT-023 — excludes point-in-time)
- Parent: `axon-b5a5cc01`
- Labels: feat, p1, rollback
- Base revision: `7ed5220c921de42440a047b1c27b1786cc4184de`
- Execution bundle: `.ddx/executions/20260413T205152-7dccfb34`

## Description
Scope: entity-level and transaction-level rollback only. Point-in-time (collection-level) rollback is a separate task (axon-eed075ae). Changes in axon-api/src/request.rs: (1) RollbackEntityRequest { collection_id, entity_id, target_version: u64, dry_run: bool, expected_version: Option<u64> }. (2) RollbackTransactionRequest { transaction_id: String, dry_run: bool }. Handler methods in axon-api/src/handler.rs: rollback_entity: read audit log for entity, find audit entry with requested target_version, extract the 'before' state, issue update_entity call with that state (standard OCC via expected_version). rollback_transaction: find all audit entries with the given transaction_id, group by entity_id, find 'before' state of each, issue compensating writes in a NEW single transaction. ALL-OR-NOTHING SEMANTICS: if ANY compensating write has an OCC conflict (entity was modified after the original transaction), NO compensating writes are applied — the entire rollback aborts and returns the conflict list. This must be explicitly enforced: do not apply partial compensating writes and then fail. Dry-run: compute planned changes and predicted OCC conflicts without writing.

## Acceptance Criteria
rollback_entity to version 2 on entity at version 5: entity returns to version-2 state at version 6; audit log shows new entry with operation=rollback; dry_run=true returns planned state without writing; OCC conflict returns conflict error with current state; rollback_transaction: all entities touched by the transaction are reverted atomically in one new transaction; ALL-OR-NOTHING TEST: entity A can be rolled back, entity B has a conflict; result is NEITHER A nor B is rolled back; entire rollback aborts returning both in the conflicts list (not silently skipping B and applying A); partial rollback is impossible

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
