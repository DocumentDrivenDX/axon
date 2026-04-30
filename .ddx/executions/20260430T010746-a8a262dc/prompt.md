<bead-review>
  <bead id="axon-9490abe4" iter=1>
    <title>investigate: createXxx duplicate-id rejection scope — typed-only or full storage path? (precursor for axon-27ee5f04)</title>
    <description>
Codex review of axon-27ee5f04 disagreed with my 'narrow GraphQL bug' framing. Storage put is overwrite by contract (matches NoSQL conventions); the rejection logic for create-with-existing-id lives in commit_transaction's op:create handler, NOT in the storage adapter itself. Fixing only the typed GraphQL path leaves HTTP and gRPC create endpoints with the same upsert behavior — inconsistent semantics across surfaces.

Investigate and decide:

Pattern A: make ALL create paths (typed GraphQL, HTTP /entities POST, gRPC CreateEntity, commitTransaction op:create) reject duplicates uniformly. Move the duplicate-check into the storage adapter or a shared helper, OR enforce at every callsite. Cost: more surface to test; may break existing callers that rely on upsert.

Pattern B: keep storage put as overwrite (existing contract); make the typed GraphQL createXxx + commitTransaction op:create the strict surfaces, leave HTTP/gRPC as documented upsert with createOrFail variants if needed. Cost: API surface inconsistency; need clear docs.

Locations to inspect (codex-supplied):
- crates/axon-api/src/handler.rs:3031-3285 (HTTP entity create handler)
- crates/axon-graphql/src/dynamic.rs:7310-7355 (typed createXxx generation)
- crates/axon-server/tests/graphql_mutations.rs:391-430 (existing test scaffolding)
- Storage layer: search for 'put' and 'AlreadyExists' to map current rejection vs upsert behavior

Document the decision in docs/helix/02-design/decisions/create-semantics.md (new file). Then file an implementation bead pointing at the chosen path.
    </description>
    <acceptance>
AC1. docs/helix/02-design/decisions/create-semantics.md exists and chooses pattern A or B with rationale, including a survey table of current create behavior across (typed GraphQL, untyped commitTransaction, HTTP, gRPC, storage adapter).

AC2. The document explicitly addresses nexiq's downstream contract (which already routes around the bug via commitTransaction) and whether their migration cost is zero (just unskip tests) or non-zero.

AC3. A follow-up implementation bead is filed targeting the chosen pattern. axon-27ee5f04 itself can then be closed as 'superseded by &lt;implementation bead&gt;' or kept open as the implementation tracker.

AC4. No code changes required beyond optional cargo check / cargo test runs to confirm current behavior matches the survey table.
    </acceptance>
    <notes>
REVIEW:BLOCK

The diff only adds execution metadata. It does not include the required decision document or any follow-up implementation bead/tracker change, so AC1-AC3 are not satisfied.
    </notes>
    <labels>helix, decomp, kind:investigation, area:storage, area:graphql</labels>
  </bead>

  <changed-files>
    <file>.ddx/executions/20260430T010717-2e03a2af/result.json</file>
  </changed-files>

  <governing>
    <note>No governing documents found. Evaluate the diff against the acceptance criteria alone.</note>
  </governing>

  <diff rev="8ed3c76dbc66d66ae3afd726ea98606df5dd7a02">
diff --git a/.ddx/executions/20260430T010717-2e03a2af/result.json b/.ddx/executions/20260430T010717-2e03a2af/result.json
new file mode 100644
index 0000000..e0a8712
--- /dev/null
+++ b/.ddx/executions/20260430T010717-2e03a2af/result.json
@@ -0,0 +1,24 @@
+{
+  "bead_id": "axon-9490abe4",
+  "attempt_id": "20260430T010717-2e03a2af",
+  "base_rev": "ad68e596dfded95800e033f85ac23a997d49c54b",
+  "result_rev": "86e59bea4d6266241aa6b7e742b6017e66a20de1",
+  "outcome": "task_succeeded",
+  "status": "success",
+  "detail": "success",
+  "harness": "agent",
+  "provider": "openrouter",
+  "model": "openai/gpt-5.4-mini",
+  "session_id": "eb-d1d8c8e5",
+  "duration_ms": 24922,
+  "tokens": 180517,
+  "cost_usd": 0.040398899999999995,
+  "exit_code": 0,
+  "execution_dir": ".ddx/executions/20260430T010717-2e03a2af",
+  "prompt_file": ".ddx/executions/20260430T010717-2e03a2af/prompt.md",
+  "manifest_file": ".ddx/executions/20260430T010717-2e03a2af/manifest.json",
+  "result_file": ".ddx/executions/20260430T010717-2e03a2af/result.json",
+  "usage_file": ".ddx/executions/20260430T010717-2e03a2af/usage.json",
+  "started_at": "2026-04-30T01:07:18.188839882Z",
+  "finished_at": "2026-04-30T01:07:43.111465358Z"
+}
\ No newline at end of file
  </diff>

  <instructions>
You are reviewing a bead implementation against its acceptance criteria.

For each acceptance-criteria (AC) item, decide whether it is implemented correctly, then assign one overall verdict:

- APPROVE — every AC item is fully and correctly implemented.
- REQUEST_CHANGES — some AC items are partial or have fixable minor issues.
- BLOCK — at least one AC item is not implemented or incorrectly implemented; or the diff is insufficient to evaluate.

## Required output format (schema_version: 1)

Respond with EXACTLY one JSON object as your final response, fenced as a single ```json … ``` code block. Do not include any prose outside the fenced block. The JSON must match this schema:

```json
{
  "schema_version": 1,
  "verdict": "APPROVE",
  "summary": "≤300 char human-readable verdict justification",
  "findings": [
    { "severity": "info", "summary": "what is wrong or notable", "location": "path/to/file.go:42" }
  ]
}
```

Rules:
- "verdict" must be exactly one of "APPROVE", "REQUEST_CHANGES", "BLOCK".
- "severity" must be exactly one of "info", "warn", "block".
- Output the JSON object inside ONE fenced ```json … ``` block. No additional prose, no extra fences, no markdown headings.
- Do not echo this template back. Do not write the words APPROVE, REQUEST_CHANGES, or BLOCK anywhere except as the JSON value of the verdict field.
  </instructions>
</bead-review>
