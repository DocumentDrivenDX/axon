# Execute Bead

You are running inside DDx's isolated execution worktree for this bead.
Your job is to make a best-effort attempt at the work described in the bead's Goals and Description, then commit the result. Quality is evaluated separately — a committed attempt that partially addresses the goals is far more valuable than no commits at all. Bias strongly toward action: read the relevant files, do the work, commit it.

## Bead
- ID: `axon-686c066a`
- Title: Wire transition_lifecycle to HTTP, gRPC and MCP transports (FEAT-015 follow-on)
- Labels: feat, p1, lifecycle, graphql
- Base revision: `67b11301b5ef4b36a8c8f8b951bc0f7e577fdb15`
- Execution bundle: `.ddx/executions/20260413T211805-3842ef37`

## Description
Bead axon-0f9c54cf implemented AxonHandler::transition_lifecycle and the request/response types, but the operation is not reachable from any transport. Wire it up:

(1) HTTP (crates/axon-server/src/gateway.rs): add POST /collections/{name}/entities/{id}/lifecycle/{lifecycle_name}/transition accepting JSON body { target_state: String, expected_version: u64, actor: Option<String> }. Return 200 with entity JSON on success, 409 on OCC conflict, 422 on InvalidTransition with body { error, current_state, valid_transitions }, 404 on entity/lifecycle not found.

(2) gRPC (crates/axon-server/src/service.rs): add TransitionLifecycle RPC if the proto definition supports it, or add a comment pointing to the proto change needed. If proto changes are needed, note them explicitly — do not add a silent no-op.

(3) MCP (crates/axon-mcp/src/handlers.rs): add transition_lifecycle tool. Input schema: collection_id, entity_id, lifecycle_name, target_state, expected_version (optional). Return entity JSON on success, structured error with valid_transitions on InvalidTransition. The error mapping to_tool_error for LifecycleNotFound and InvalidTransition already exists from axon-0f9c54cf — verify it is wired to the new tool handler.

(4) Tests: add HTTP integration tests (start real server, POST to the route) following the pattern in crates/axon-server/tests/api_contract.rs. Test: valid transition succeeds and returns updated entity; invalid transition returns 422 with valid_transitions; wrong version returns 409.

## Acceptance Criteria
POST /collections/{name}/entities/{id}/lifecycle/{lifecycle_name}/transition is reachable via HTTP; valid transition updates entity and returns 200; InvalidTransition returns 422 with valid_transitions list; OCC conflict returns 409; integration tests using real server pass; MCP transition_lifecycle tool callable and returns entity on success

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
