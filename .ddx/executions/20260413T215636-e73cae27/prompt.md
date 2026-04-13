# Execute Bead

You are running inside DDx's isolated execution worktree for this bead.
Your job is to make a best-effort attempt at the work described in the bead's Goals and Description, then commit the result. Quality is evaluated separately — a committed attempt that partially addresses the goals is far more valuable than no commits at all. Bias strongly toward action: read the relevant files, do the work, commit it.

## Bead
- ID: `axon-4c08a548`
- Title: Complete PRD isolation fix — 7 residual false serializable claims after axon-f63ac16f
- Labels: bug, p0, docs, isolation
- Base revision: `abb3778785421c424ab453dd4ad6fa7100886c40`
- Execution bundle: `.ddx/executions/20260413T215636-e73cae27`

## Description
Fix 7 residual false serializable isolation claims left after axon-f63ac16f. All 7 require targeted edits to specific lines — do NOT edit alignment review docs, SPIKE-001, ADR-003, ADR-004, or storage-layer references.

EXACT CHANGES REQUIRED:

(1) docs/helix/01-frame/prd.md line ~137 — replace the prose paragraph starting 'The default and recommended level is **serializable**' with: 'The default isolation level in V1 is **snapshot isolation**, implemented via optimistic concurrency control (OCC) with write-set conflict detection. Serializable isolation (preventing write skew) is a P1 post-V1 goal; see the known gap below.'

(2) docs/helix/01-frame/principles.md line ~65 — in the P4 table row, replace 'Serializable isolation | Concurrent transactions produce results equivalent to serial execution' with 'Snapshot Isolation | Concurrent transactions are isolated from each other's writes; write skew prevention (full serializability) is P1.'

(3) docs/helix/01-frame/technical-requirements.md line ~84 — in the Storage Adapter Trait bullet, change 'begin, commit, abort with serializable isolation' to 'begin, commit, abort with snapshot isolation (V1); storage backends may provide serializable at the transaction layer.'

(4) docs/helix/01-frame/technical-requirements.md line ~343 — in the Correctness Invariants table, change 'Serializable isolation | Cycle test (ring integrity) under concurrent transactions' to 'Snapshot Isolation | Cycle test (ring integrity) under concurrent transactions; write skew prevention is P1.'

(5) docs/helix/03-test/test-plan.md lines ~82-84 — rename INV-002 from 'Serializable Isolation (Cycle Test)' to 'Snapshot Isolation (Cycle Test)'; remove 'No write skew' from the invariant statement (SI prevents phantom reads and dirty reads, not write skew); add 'INV-002b (P1): Write skew is prevented under serializable isolation.'

(6) docs/helix/01-frame/features/FEAT-008-acid-transactions.md line ~63 — change 'Under serializable isolation, the first transaction to commit wins' to 'Under snapshot isolation with write-set OCC, the first transaction to commit wins.'

(7) docs/helix/01-frame/competitive-analysis.md line ~149 — in the Axon column, change 'Full ACID, serializable isolation' to 'Full ACID, snapshot isolation (V1); serializable is P1.'

ACCEPTANCE VERIFICATION: after changes, run these greps and confirm zero matches:
  grep -ri 'serializable.*default' docs/
  grep -n 'no write skew' docs/helix/03-test/test-plan.md
  grep -n 'serializable isolation.*first' docs/helix/01-frame/features/FEAT-008-acid-transactions.md
  grep -n 'serializable isolation' docs/helix/01-frame/principles.md
  grep -n 'Full ACID, serializable' docs/helix/01-frame/competitive-analysis.md

## Acceptance Criteria
All 7 specific locations updated as described; grep -ri 'serializable.*default' docs/ returns zero results; grep -ri 'no write skew' docs/ returns zero V1 claims (only P1 deferred items); prd.md prose and table are internally consistent; cargo check passes (docs only change)

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
