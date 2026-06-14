# Admin Policy Workbench Reel

Release target: Axon 0.7.1

Policy authoring, dry-runs, redacted browsing, intent queues, approval review, stale-intent handling, and UI parity with GraphQL/MCP.

Sample project: `examples/admin-policy-workbench`

## Storyboard

1. Open the policy workspace and dry-run subjects against the active policy.
2. Browse entities as restricted users and verify redacted fields render as null.
3. Review intent diffs, approve or reject with a reason, then follow the audit link.
4. Open an MCP-originated intent and verify its delegated authority envelope.

## Coverage Entries

- feature: FEAT-011 - Admin Web UI
- feature: FEAT-024 - Application Substrate
- feature: FEAT-029 - Data-Layer Access Control Policies
- feature: FEAT-031 - Policy and Intents Admin UI
- scenario: SCN-017 - Trusted Agent Invoice Write via GraphQL and MCP (FEAT-015, FEAT-016, FEAT-029, FEAT-030)
- story: US-040 - Navigate the Tenant and Database Model
- story: US-041 - Administer Users, Members, and Credentials
- story: US-042 - Manage Collections and Entities
- story: US-045 - Use Advanced Database Tools
- story: US-046 - Field-Level Masking
- story: US-047 - Attribute-Based Write Control
- story: US-098 - Generate a Typed Client from Schema
- story: US-099 - Generate a Schema-Driven Admin App
- story: US-100 - Deploy a Schema-Backed App with One Command
- story: US-101 - Hide Inaccessible Entities
- story: US-102 - Redact Sensitive Fields
- story: US-103 - Reject Denied Writes
- story: US-104 - Explain Effective Policy
- story: US-109 - Author And Test Policy Before Activation
- story: US-113 - Inspect Effective Policy In The Web UI
- story: US-114 - Author And Dry-Run Policies Before Activation
- story: US-115 - Browse Entities With Policy-Safe UI Semantics
- story: US-116 - Preview And Commit Mutation Intents From The Web UI
- story: US-117 - Review, Approve, And Reject Pending Intents
- story: US-118 - Handle Stale And Mismatched Intents Safely
- story: US-119 - Inspect MCP-Originated Policy And Intent Outcomes
- story: US-121 - Manage Schemas Visually
- story: US-122 - Inspect Audit and Recover Entity State
