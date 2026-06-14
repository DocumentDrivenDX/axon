# Invoice Approval Guardrails Reel

Release target: Axon 0.7.1

A finance workflow covering AP/AR partial payment, state machines, policy envelopes, mutation intent preview/approval, rollback, and audit-safe trusted agent writes.

Sample project: `examples/invoice-approval-guardrails`

## Storyboard

1. Seed vendors, invoices, and a partial payment.
2. Preview the payment application as a mutation intent before commit.
3. Approve the over-threshold write, commit it atomically, and verify audit grouping.
4. Replay the transaction in rollback mode to show recoverability.

## Coverage Entries

- feature: FEAT-003 - Audit Log
- feature: FEAT-008 - ACID Transactions
- feature: FEAT-010 - Entity State Machines and Transition Guards
- feature: FEAT-015 - GraphQL Query Layer
- feature: FEAT-022 - Agent Guardrails
- feature: FEAT-030 - Mutation Intents and Approval
- scenario: SCN-001 - AP/AR - Payment Application with Partial Payment
- scenario: SCN-005 - Workflow - Invoice Approval Chain
- scenario: SCN-010 - Time Tracking - Approval and Billing
- story: US-007 - Query the Audit Trail
- story: US-008 - Revert an Entity to Previous State
- story: US-009 - Attach Metadata to Mutations
- story: US-020 - Atomic Multi-Entity Update
- story: US-021 - Concurrent Agent Safety
- story: US-022 - Snapshot Isolation
- story: US-026 - Enforce Invoice Approval Workflow
- story: US-027 - Bead Lifecycle with Dependency Guards
- story: US-028 - Query Valid Transitions
- story: US-048 - Query Entities with Relationships
- story: US-049 - Discover the API via Introspection
- story: US-050 - Subscribe to Entity Changes
- story: US-051 - Use GraphQL from the Admin UI
- story: US-057 - Mutate Entities via GraphQL
- story: US-078 - JSON-LD Content Negotiation
- story: US-079 - Multi-Collection Audit Tail
- story: US-081 - Idempotent Transaction Submission
- story: US-092 - Keep Agent Writes Inside Assigned Scope
- story: US-093 - Throttle Agent Mutation Bursts
- story: US-094 - Configure Guardrails Per Agent Identity
- story: US-105 - Preview A GraphQL Mutation
- story: US-106 - Route Risky Writes For Approval
- story: US-107 - Prevent Stale Approval Execution
- story: US-108 - Use Mutation Intents From MCP
- story: US-110 - Enforce Policy Across GraphQL Traversal
- story: US-111 - Preview And Commit Mutation Intents
- story: US-120 - PROV-O Audit Shape
- use_case: USE-003 - AP/AR (Accounts Payable / Accounts Receivable)
- use_case: USE-004 - Time Tracking
- use_case: USE-009 - Workflow Automation
