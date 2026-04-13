# Execute Bead

You are running inside DDx's isolated execution worktree for this bead.
Your job is to make a best-effort attempt at the work described in the bead's Goals and Description, then commit the result. Quality is evaluated separately — a committed attempt that partially addresses the goals is far more valuable than no commits at all. Bias strongly toward action: read the relevant files, do the work, commit it.

## Bead
- ID: `axon-88b4b5f1`
- Title: Add gate_results field to Entity and persist via StorageAdapter (FEAT-019 phase 1)
- Parent: `axon-b5a5cc01`
- Labels: feat, p1, gate-materialization
- Base revision: `84d15732c58411c33ba23b730398a50734913aa8`
- Execution bundle: `.ddx/executions/20260413T195813-02756db5`

## Description
Scope: data model, persistence, and removal of the superseded gate-results side-table. Gate query filter and lifecycle enforcement are a separate task (axon-ba577e48).

Changes:
(1) axon-core/src/types.rs: add gate_results: HashMap<String, GateResult> to Entity struct, where GateResult is re-exported from axon-schema::gates (use the existing GateResult { gate: String, pass: bool, failures: Vec<RuleViolation> } type — do NOT define a new type). Add #[serde(default)] so entities persisted before this field was added deserialize with gate_results=HashMap::new().

(2) axon-api/src/handler.rs: in create_entity, update_entity, and patch_entity, populate entity.gate_results directly from the results of evaluate_gates() — GateEvaluation.gate_results is already HashMap<String, GateResult>, so this is a direct assignment into entity.gate_results before the entity is written to storage. REMOVE the existing calls to self.storage.put_gate_results() at the three write sites (~lines 257, 394, 526). REMOVE the call to self.storage.delete_gate_results() in delete_entity (~line 604).

(3) axon-storage/src/adapter.rs: REMOVE the following methods from the StorageAdapter trait: put_gate_results, get_gate_results, delete_gate_results, gate_lookup. These are replaced by the entity blob as the single system of record for gate results. Future gate-based query filtering will be backed by the general-purpose index infrastructure (FEAT-013) rather than this bespoke side-table.

(4) axon-storage/src/memory.rs: REMOVE the gate_results: HashMap<(CollectionId, EntityId), HashMap<String, bool>> field from MemoryStorageAdapter and its impl blocks for put_gate_results, get_gate_results, delete_gate_results, and gate_lookup.

(5) axon-storage/src/sqlite.rs: same removal — remove any put_gate_results, get_gate_results, delete_gate_results, gate_lookup impls.

(6) No storage schema change required — gate_results is serialized as part of the entity data blob (opaque JSON). The new field appears automatically when the entity struct is serialized.

## Acceptance Criteria
SAVE GATE (save_violations non-empty): create entity violating a save-gate rule → write REJECTED with validation error; gate_results NOT stored (write never committed). CUSTOM GATE: create entity violating a custom gate rule → write PROCEEDS; entity.gate_results[gate_name].pass = false; entity.gate_results[gate_name].failures is non-empty. PASSING GATE: entity satisfying all rules in a custom gate → gate_results[gate_name].pass = true; gate_results[gate_name].failures is empty. UPDATE: gate_results recomputed on every update/patch; passing then failing a gate updates the stored value. ROUND-TRIP: entity with gate_results persisted to MemoryStorageAdapter and re-fetched returns identical gate_results (HashMap<String, GateResult> round-trips through serde including failures vec). SIDE-TABLE REMOVED: StorageAdapter trait has no put_gate_results, get_gate_results, delete_gate_results, or gate_lookup methods; MemoryStorageAdapter has no gate_results HashMap field; handler.rs has no calls to put_gate_results or delete_gate_results. Existing tests still pass.

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
