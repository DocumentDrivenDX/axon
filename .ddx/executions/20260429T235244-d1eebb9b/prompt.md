<bead-review>
  <bead id="axon-db7a6d0a" iter=1>
    <title>feature: application audit-write API — first-class 'emit audit event' contract for downstream consumers</title>
    <description>
&lt;context&gt;
Filed on behalf of nexiq (/home/erik/Projects/nexiq). nexiq's FEAT-010 plan references writing audit events in multiple places:
  - access_denied events when a user hits a forbidden route (plan line 554, auth-design.md)
  - invoice approved / engagement status transition audit entries (plan line 104, line 486, line 497)
  - first-run user bootstrap events
  - dashboard views / PII exposures that policy wants recorded

Axon's current api-contracts.md (2026-04-19 state) covers audit READS at L269-L302 but does not document a consumer-facing audit WRITE path. Entity mutations already leave audit footprints in Axon's internal audit log (per ADR), but application-level events that are NOT tied to a specific entity mutation (e.g. 'user attempted to access /invoices but was denied by RBAC — no entity touched') need a first-class write contract.

Codex fresh-eyes review of nexiq's plan 2026-04-19 flagged this as a handwaved assumption that blocks Phase 2 implementation.
&lt;/context&gt;

&lt;what-nexiq-needs&gt;

1. **Event kinds used by nexiq** — in priority order:
   -  (route, attempted_action, caller_identity, reason)
   -  (already event-tied; could piggyback on the mutation)
   -  /  (piggyback on mutation)
   -  (no entity mutation; separate write)
   -  (no mutation; write before resolving identity)
   -  (user viewed rate_card, contractor rates, etc.)
   -  (user exported a report — compliance event)

   Most are piggyback-candidates; a few (access_denied, login_attempted, first_run) have no natural entity mutation to ride on.

2. **Shape options**:
   - A) Extend  to accept an optional  field alongside , so an app-level event can travel with an entity mutation OR alone (empty ops array + one event).
   - B) Separate endpoint  with idempotency semantics.
   - C) GraphQL mutation  once the GraphQL mutation layer ships.

   Downstream preference: (A) — matches the transaction-is-the-unit-of-change mental model, keeps idempotency uniform, and lets a route-denied event be written atomically with a downgrade operation if both are wanted.

3. **Schema**: same rationale as entity audit rows (actor from auth context, at from server time, ref is polymorphic entity reference or null, metadata is JSON). Nexiq doesn't need Axon to define application event_kinds ahead of time — a curated list is brittle; a free-form string is fine if Axon records what the consumer asserts.

4. **Query parity**: these events need to be readable via the same audit-read API so a nexiq dashboard can show 'all access_denied events on engagement X this week' regardless of whether the event came from an entity mutation or a standalone emit.
&lt;/what-nexiq-needs&gt;

&lt;interaction-with-axon-c5cc071a&gt;

The RBAC feature ask (axon-c5cc071a) includes an access_denied denial shape. If Axon's RBAC layer itself emits audit entries on denial, that might cover the 'access_denied' event kind automatically — in which case this bead narrows to the other event kinds above. Confirm whether RBAC denials go to audit automatically.
&lt;/interaction-with-axon-c5cc071a&gt;

&lt;priority-rationale&gt;

P1, not P0. Nexiq can ship Phase 2 without this by piggybacking on entity mutations for most cases and suppressing access_denied audit in the interim. But the workaround is cumbersome (every route component carries its own audit-write path) and creates a gap in compliance stories; filing now so Axon can scope it into the work-breakdown alongside RBAC enforcement.
&lt;/priority-rationale&gt;
    </description>
    <acceptance>
docs/helix/02-design/api-contracts.md adds an 'Audit Writes' section (alongside the existing 'Audit Reads' coverage at L269-L302) pinning: (a) the shape of a standalone audit entry a consumer can emit outside of a mutation (e.g. 'user viewed X', 'access_denied at route Y', 'login attempted from origin Z'); (b) whether audit entries are a separate endpoint/GraphQL mutation OR always piggyback on a transaction; (c) the event schema (event_kind, actor, subject_ref, payload_json, origin) and whether consumers can define new event_kind values or must use a curated list; (d) idempotency/replay semantics (do duplicate audit writes collapse?); (e) retention / queryability guarantees relative to entity audit rows; (f) the response shape when Axon rejects an audit write (rate limit, unknown event_kind, missing actor). A reference invocation example from a browser client and an integration worker is included. Downstream (nexiq) can then write access_denied events, user-facing login-attempt events, and business-level 'invoice approved' markers that are not coupled to a single entity mutation.
    </acceptance>
    <notes>
REVIEW:BLOCK

The change only adds an execution metadata file. None of the required API contract documentation for audit writes, schema, idempotency, rejection behavior, retention/queryability, or examples is present, so the bead is not implemented.
    </notes>
    <labels>helix, phase:frame, kind:feature-request, area:api, area:audit, downstream:nexiq, cross-repo</labels>
  </bead>

  <changed-files>
    <file>.ddx/executions/20260429T235214-c5346b75/result.json</file>
  </changed-files>

  <governing>
    <note>No governing documents found. Evaluate the diff against the acceptance criteria alone.</note>
  </governing>

  <diff rev="f2f9fef466299a6342230815f83dd7c6b605ac87">
diff --git a/.ddx/executions/20260429T235214-c5346b75/result.json b/.ddx/executions/20260429T235214-c5346b75/result.json
new file mode 100644
index 0000000..aac538c
--- /dev/null
+++ b/.ddx/executions/20260429T235214-c5346b75/result.json
@@ -0,0 +1,24 @@
+{
+  "bead_id": "axon-db7a6d0a",
+  "attempt_id": "20260429T235214-c5346b75",
+  "base_rev": "9e560d413a501061df1142df2b936f8167091fd8",
+  "result_rev": "0f374da7e657059d024a378475d8c629e97aab59",
+  "outcome": "task_succeeded",
+  "status": "success",
+  "detail": "success",
+  "harness": "agent",
+  "provider": "openrouter",
+  "model": "openai/gpt-5.4-mini",
+  "session_id": "eb-a1cfe2f9",
+  "duration_ms": 25513,
+  "tokens": 142721,
+  "cost_usd": 0.035646599999999994,
+  "exit_code": 0,
+  "execution_dir": ".ddx/executions/20260429T235214-c5346b75",
+  "prompt_file": ".ddx/executions/20260429T235214-c5346b75/prompt.md",
+  "manifest_file": ".ddx/executions/20260429T235214-c5346b75/manifest.json",
+  "result_file": ".ddx/executions/20260429T235214-c5346b75/result.json",
+  "usage_file": ".ddx/executions/20260429T235214-c5346b75/usage.json",
+  "started_at": "2026-04-29T23:52:15.426476395Z",
+  "finished_at": "2026-04-29T23:52:40.940466528Z"
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
