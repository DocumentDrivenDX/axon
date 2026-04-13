# Execute Bead

You are running inside DDx's isolated execution worktree for this bead.
Your job is to make a best-effort attempt at the work described in the bead's Goals and Description, then commit the result. Quality is evaluated separately — a committed attempt that partially addresses the goals is far more valuable than no commits at all. Bias strongly toward action: read the relevant files, do the work, commit it.

## Bead
- ID: `axon-d0302596`
- Title: Add gate_results storage round-trip and backward-compat serde tests (axon-88b4b5f1 acceptance gap)
- Labels: test, p1, gate-materialization
- Base revision: `4291e167c50437b363bfa1b36df3bf996128a333`
- Execution bundle: `.ddx/executions/20260413T215115-7aa4e893`

## Description
Bead axon-88b4b5f1 added gate_results to Entity but added no new tests. The acceptance criteria call out persistence and serde scenarios that are not exercised by existing tests (existing tests only check resp.gates, not the fetched entity's gate_results field). Add the following tests:

(1) Storage round-trip test in crates/axon-api/src/handler.rs or crates/axon-api/tests/: create an entity that fails a custom gate, then call storage.get(col, id) directly and assert fetched.gate_results[gate_name].pass == false and .failures non-empty. Then update the entity to satisfy the gate and assert fetched.gate_results[gate_name].pass == true and .failures is empty. Validates handler.rs:206 and handler.rs:321 actually write to storage, not just the response.

(2) Serde round-trip test in crates/axon-core/src/types.rs: construct an Entity with a non-empty gate_results containing a GateResult with multiple RuleViolation failures, call serde_json::to_value / from_value, assert equality. Validates the new field round-trips through the entity blob correctly.

(3) Backward-compat test: deserialize an Entity JSON blob that lacks the gate_results field (simulating pre-FEAT-019 stored data) and assert it deserializes successfully with gate_results.is_empty(). Validates #[serde(default)] works as expected.

(4) Clarify transaction-path semantics at handler.rs:1080 which carries forward existing.gate_results rather than re-evaluating: add a code comment explaining whether this is intentional (carry-forward for transaction fast-path is acceptable) or a gap. If carry-forward is intentional, a test asserting that transaction writes do NOT re-evaluate gates would lock in the behavior.

## Acceptance Criteria
Four new tests added as described; all pass; cargo test --workspace passes; the acceptance criteria from axon-88b4b5f1 ('ROUND-TRIP', 'CUSTOM GATE', 'PASSING GATE', 'UPDATE recomputed') can now be traced to specific test cases; transaction-path behavior is documented in a code comment

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
