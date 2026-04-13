# Execute Bead

You are running inside DDx's isolated execution worktree for this bead.
Your job is to make a best-effort attempt at the work described in the bead's Goals and Description, then commit the result. Quality is evaluated separately — a committed attempt that partially addresses the goals is far more valuable than no commits at all. Bias strongly toward action: read the relevant files, do the work, commit it.

## Bead
- ID: `axon-1c343de8`
- Title: Author ADR for Rollback/Recovery compensating transaction semantics (FEAT-023)
- Parent: `axon-b5a5cc01`
- Labels: chore, p1, planning, rollback
- Base revision: `7fec58e147e918433e889005bc857435b07ba960`
- Execution bundle: `.ddx/executions/20260413T200703-1a816555`

## Description
Write ADR-015-rollback-recovery.md in docs/helix/02-design/adr/. The ADR must resolve: (1) Compensating write approach: rollback is a new write at version N+1 applying the prior state — not a version pointer rewrite. This preserves the audit log as append-only and keeps OCC semantics consistent. (2) OCC during rollback: the compensating write uses expected_version=current_version. If the entity was modified since the rollback target, this produces a standard version conflict — the caller must resolve. (3) Cross-entity atomicity for transaction-level rollback: all compensating writes for a rolled-back transaction are grouped in a new transaction with a shared rollback_transaction_id. If any compensating write fails (OCC conflict), the entire compensating transaction is aborted and the conflict list is returned — partial rollback is rejected. (4) Audit trail: compensating writes produce audit entries with operation=rollback and a reference to the original entry/transaction. (5) Dry-run: reads audit log and current entity states, computes the compensating writes, detects OCC conflicts — all without writing. The ADR should explicitly state what is out of scope for V1 (CRDT merging, automatic conflict resolution, saga compensation).

## Acceptance Criteria
ADR-015 exists at docs/helix/02-design/adr/ADR-015-rollback-recovery.md; covers compensating write rationale, OCC interaction, cross-entity atomicity, audit trail design, and V1 out-of-scope; no open questions remain that would block implementation (axon-375db29e)

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
