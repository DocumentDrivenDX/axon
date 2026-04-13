# Execute Bead

You are running inside DDx's isolated execution worktree for this bead.
Your job is to make a best-effort attempt at the work described in the bead's Goals and Description, then commit the result. Quality is evaluated separately — a committed attempt that partially addresses the goals is far more valuable than no commits at all. Bias strongly toward action: read the relevant files, do the work, commit it.

## Bead
- ID: `axon-ee53b52d`
- Title: Wire GraphQL entity CRUD and link mutations to handler methods (FEAT-015)
- Parent: `axon-b5a5cc01`
- Labels: feat, p1, graphql
- Base revision: `1f930bd286c65dd12a0a2c64ad601b571966624a`
- Execution bundle: `.ddx/executions/20260413T214540-3931ea5c`

## Description
Scope: entity CRUD mutations and link mutations only. Lifecycle transitions and commitTransaction are a separate task (axon-49c241c2). File: axon-graphql/src/dynamic.rs. Currently all mutation resolvers return FieldValue::NULL at lines ~72 and ~81. For each collection, wire: (1) create{Collection}: extract fields from MutationArgs, build CreateEntityRequest, call handler.create_entity(), return entity as FieldValue::Object or propagate error. (2) update{Collection}: build UpdateEntityRequest with expected_version from args, call handler.update_entity(). (3) patch{Collection}: build PatchEntityRequest with JSON merge patch body, call handler.patch_entity(). (4) delete{Collection}: call handler.delete_entity(). (5) create{Collection}Link / delete{Collection}Link: call handler.create_link() / delete_link(). Version conflict must surface as GraphQL error with code=VERSION_CONFLICT and currentEntity in extensions. Schema validation errors must surface with code=VALIDATION_ERROR and violations list in extensions. TEST LOCATION: tests must be in axon-graphql/tests/mutations.rs OR axon-server/tests/graphql_mutations.rs using the real HTTP test server pattern from axon-server/tests/api_contract.rs (start a real server, issue HTTP requests with a GraphQL client). Handler unit tests alone are insufficient.

## Acceptance Criteria
createBead mutation creates an entity and returns it with id and version; updateBead with correct expectedVersion succeeds; updateBead with wrong expectedVersion returns GraphQL error with code=VERSION_CONFLICT and currentEntity in extensions; patchBead modifies only specified fields; deleteBead removes entity; createBeadLink creates a typed link; tests are integration tests using the real HTTP server (not resolver unit tests); error extension fields are asserted in tests (not just that an error occurred)

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
