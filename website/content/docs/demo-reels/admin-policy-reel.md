---
title: Admin Policy Workbench Reel
weight: 16
prev: ../
---

# Admin Policy Workbench Reel

Release target: Axon 0.7.1

Policy authoring, dry-runs, redacted browsing, intent queues, approval review, stale-intent handling, and UI parity with GraphQL/MCP.

- Sample project: [admin-policy-workbench](https://github.com/DocumentDrivenDX/axon/tree/master/examples/admin-policy-workbench)
- Script source: [`docs/demos/reels/admin-policy-reel.md`](https://github.com/DocumentDrivenDX/axon/blob/master/docs/demos/reels/admin-policy-reel.md)
- Coverage entries: 28

## Storyboard

1. Open the policy workspace and dry-run subjects against the active policy.
2. Browse entities as restricted users and verify redacted fields render as null.
3. Review intent diffs, approve or reject with a reason, then follow the audit link.
4. Open an MCP-originated intent and verify its delegated authority envelope.

## Covered HELIX Entries

| Type | ID | Title | Source | Sample | Demo reel |
|---|---|---|---|---|---|
| feature | FEAT-011 | Admin Web UI | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/features/FEAT-011-admin-web-ui.md) | [admin-policy-workbench](https://github.com/DocumentDrivenDX/axon/tree/master/examples/admin-policy-workbench) | [admin-policy-reel](../admin-policy-reel/) |
| feature | FEAT-024 | Application Substrate | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/features/FEAT-024-application-substrate.md) | [admin-policy-workbench](https://github.com/DocumentDrivenDX/axon/tree/master/examples/admin-policy-workbench) | [admin-policy-reel](../admin-policy-reel/) |
| feature | FEAT-029 | Data-Layer Access Control Policies | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/features/FEAT-029-access-control.md) | [admin-policy-workbench](https://github.com/DocumentDrivenDX/axon/tree/master/examples/admin-policy-workbench) | [admin-policy-reel](../admin-policy-reel/) |
| feature | FEAT-031 | Policy and Intents Admin UI | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/features/FEAT-031-policy-intents-admin-ui.md) | [admin-policy-workbench](https://github.com/DocumentDrivenDX/axon/tree/master/examples/admin-policy-workbench) | [admin-policy-reel](../admin-policy-reel/) |
| scenario | SCN-017 | Trusted Agent Invoice Write via GraphQL and MCP (FEAT-015, FEAT-016, FEAT-029, FEAT-030) | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/03-test/test-plan.md) | [admin-policy-workbench](https://github.com/DocumentDrivenDX/axon/tree/master/examples/admin-policy-workbench) | [admin-policy-reel](../admin-policy-reel/) |
| story | US-040 | Navigate the Tenant and Database Model | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-040-navigate-the-tenant-and-database-model.md) | [admin-policy-workbench](https://github.com/DocumentDrivenDX/axon/tree/master/examples/admin-policy-workbench) | [admin-policy-reel](../admin-policy-reel/) |
| story | US-041 | Administer Users, Members, and Credentials | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-041-administer-users-members-and-credentials.md) | [admin-policy-workbench](https://github.com/DocumentDrivenDX/axon/tree/master/examples/admin-policy-workbench) | [admin-policy-reel](../admin-policy-reel/) |
| story | US-042 | Manage Collections and Entities | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-042-manage-collections-and-entities.md) | [admin-policy-workbench](https://github.com/DocumentDrivenDX/axon/tree/master/examples/admin-policy-workbench) | [admin-policy-reel](../admin-policy-reel/) |
| story | US-045 | Use Advanced Database Tools | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-045-use-advanced-database-tools.md) | [admin-policy-workbench](https://github.com/DocumentDrivenDX/axon/tree/master/examples/admin-policy-workbench) | [admin-policy-reel](../admin-policy-reel/) |
| story | US-046 | Field-Level Masking | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-046-field-level-masking.md) | [admin-policy-workbench](https://github.com/DocumentDrivenDX/axon/tree/master/examples/admin-policy-workbench) | [admin-policy-reel](../admin-policy-reel/) |
| story | US-047 | Attribute-Based Write Control | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-047-attribute-based-write-control.md) | [admin-policy-workbench](https://github.com/DocumentDrivenDX/axon/tree/master/examples/admin-policy-workbench) | [admin-policy-reel](../admin-policy-reel/) |
| story | US-098 | Generate a Typed Client from Schema | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-098-generate-a-typed-client-from-schema.md) | [admin-policy-workbench](https://github.com/DocumentDrivenDX/axon/tree/master/examples/admin-policy-workbench) | [admin-policy-reel](../admin-policy-reel/) |
| story | US-099 | Generate a Schema-Driven Admin App | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-099-generate-a-schema-driven-admin-app.md) | [admin-policy-workbench](https://github.com/DocumentDrivenDX/axon/tree/master/examples/admin-policy-workbench) | [admin-policy-reel](../admin-policy-reel/) |
| story | US-100 | Deploy a Schema-Backed App with One Command | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-100-deploy-a-schema-backed-app-with-one-command.md) | [admin-policy-workbench](https://github.com/DocumentDrivenDX/axon/tree/master/examples/admin-policy-workbench) | [admin-policy-reel](../admin-policy-reel/) |
| story | US-101 | Hide Inaccessible Entities | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-101-hide-inaccessible-entities.md) | [admin-policy-workbench](https://github.com/DocumentDrivenDX/axon/tree/master/examples/admin-policy-workbench) | [admin-policy-reel](../admin-policy-reel/) |
| story | US-102 | Redact Sensitive Fields | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-102-redact-sensitive-fields.md) | [admin-policy-workbench](https://github.com/DocumentDrivenDX/axon/tree/master/examples/admin-policy-workbench) | [admin-policy-reel](../admin-policy-reel/) |
| story | US-103 | Reject Denied Writes | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-103-reject-denied-writes.md) | [admin-policy-workbench](https://github.com/DocumentDrivenDX/axon/tree/master/examples/admin-policy-workbench) | [admin-policy-reel](../admin-policy-reel/) |
| story | US-104 | Explain Effective Policy | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-104-explain-effective-policy.md) | [admin-policy-workbench](https://github.com/DocumentDrivenDX/axon/tree/master/examples/admin-policy-workbench) | [admin-policy-reel](../admin-policy-reel/) |
| story | US-109 | Author And Test Policy Before Activation | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-109-author-and-test-policy-before-activation.md) | [admin-policy-workbench](https://github.com/DocumentDrivenDX/axon/tree/master/examples/admin-policy-workbench) | [admin-policy-reel](../admin-policy-reel/) |
| story | US-113 | Inspect Effective Policy In The Web UI | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-113-inspect-effective-policy-in-the-web-ui.md) | [admin-policy-workbench](https://github.com/DocumentDrivenDX/axon/tree/master/examples/admin-policy-workbench) | [admin-policy-reel](../admin-policy-reel/) |
| story | US-114 | Author And Dry-Run Policies Before Activation | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-114-author-and-dry-run-policies-before-activation.md) | [admin-policy-workbench](https://github.com/DocumentDrivenDX/axon/tree/master/examples/admin-policy-workbench) | [admin-policy-reel](../admin-policy-reel/) |
| story | US-115 | Browse Entities With Policy-Safe UI Semantics | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-115-browse-entities-with-policy-safe-ui-semantics.md) | [admin-policy-workbench](https://github.com/DocumentDrivenDX/axon/tree/master/examples/admin-policy-workbench) | [admin-policy-reel](../admin-policy-reel/) |
| story | US-116 | Preview And Commit Mutation Intents From The Web UI | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-116-preview-and-commit-mutation-intents-from-the-web-ui.md) | [admin-policy-workbench](https://github.com/DocumentDrivenDX/axon/tree/master/examples/admin-policy-workbench) | [admin-policy-reel](../admin-policy-reel/) |
| story | US-117 | Review, Approve, And Reject Pending Intents | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-117-review-approve-and-reject-pending-intents.md) | [admin-policy-workbench](https://github.com/DocumentDrivenDX/axon/tree/master/examples/admin-policy-workbench) | [admin-policy-reel](../admin-policy-reel/) |
| story | US-118 | Handle Stale And Mismatched Intents Safely | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-118-handle-stale-and-mismatched-intents-safely.md) | [admin-policy-workbench](https://github.com/DocumentDrivenDX/axon/tree/master/examples/admin-policy-workbench) | [admin-policy-reel](../admin-policy-reel/) |
| story | US-119 | Inspect MCP-Originated Policy And Intent Outcomes | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-119-inspect-mcp-originated-policy-and-intent-outcomes.md) | [admin-policy-workbench](https://github.com/DocumentDrivenDX/axon/tree/master/examples/admin-policy-workbench) | [admin-policy-reel](../admin-policy-reel/) |
| story | US-121 | Manage Schemas Visually | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-121-manage-schemas-visually.md) | [admin-policy-workbench](https://github.com/DocumentDrivenDX/axon/tree/master/examples/admin-policy-workbench) | [admin-policy-reel](../admin-policy-reel/) |
| story | US-122 | Inspect Audit and Recover Entity State | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-122-inspect-audit-and-recover-entity-state.md) | [admin-policy-workbench](https://github.com/DocumentDrivenDX/axon/tree/master/examples/admin-policy-workbench) | [admin-policy-reel](../admin-policy-reel/) |
