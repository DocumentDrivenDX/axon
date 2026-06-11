# Axon Browser API Contracts

> **Deprecated as an authoritative source.** Per HELIX 0.6.1, the Contract
> catalog is the sole home of exact shared interface surface. The normative
> content that previously lived in this document has moved to the contract
> suite under [`contracts/`](./contracts/). This file remains only as an
> index; do not add normative surface here.

| Topic (former section) | Normative home |
|------------------------|----------------|
| Route grammar, tenancy addressing, CORS, headers, error envelope, status codes | [CONTRACT-001 — HTTP API Surface](./contracts/CONTRACT-001-http-api-surface.md) |
| Transactions, OCC, idempotency protocol, rate-limit envelope | [CONTRACT-001 — HTTP API Surface](./contracts/CONTRACT-001-http-api-surface.md) |
| Markdown template render/management, lifecycle, rollback, audit REST endpoints, schema handshake | [CONTRACT-001 — HTTP API Surface](./contracts/CONTRACT-001-http-api-surface.md) |
| GraphQL schema generation, naming, filtering, mutations, subscriptions, aggregations, error extensions | [CONTRACT-002 — GraphQL Surface](./contracts/CONTRACT-002-graphql-surface.md) |
| Control-plane GraphQL (tenants, users, members, databases, credentials, current user) | [CONTRACT-002 — GraphQL Surface](./contracts/CONTRACT-002-graphql-surface.md) |
| Policy/intent GraphQL fields (`effectivePolicy`, `previewMutation`, intents) | [CONTRACT-002 — GraphQL Surface](./contracts/CONTRACT-002-graphql-surface.md) |
| MCP tools, resources, prompts, transports, structured outcomes | [CONTRACT-003 — MCP Surface](./contracts/CONTRACT-003-mcp-surface.md) |
| Policy grammar and policy document format | [CONTRACT-004 — Policy Grammar](./contracts/CONTRACT-004-policy-grammar.md) |
| Audit record schema and application audit events | [CONTRACT-005 — Audit Record](./contracts/CONTRACT-005-audit-record.md) |
| Change-feed / CDC envelope | [CONTRACT-006 — CDC Envelope](./contracts/CONTRACT-006-cdc-envelope.md) |
| Cypher query surface (`axonQuery` language semantics, named queries) | [CONTRACT-007 — Cypher Query Surface](./contracts/CONTRACT-007-cypher-query-surface.md) |
| CLI command tree, TOML config, env precedence, client mode | [CONTRACT-008 — CLI and Config](./contracts/CONTRACT-008-cli-and-config.md) |
| TypeScript SDK surface (clients, methods, governed-workflow verbs, errors) | [CONTRACT-009 — SDK Surface](./contracts/CONTRACT-009-sdk-surface.md) |
| ESF schema format | [CONTRACT-010 — ESF Schema Format](./contracts/CONTRACT-010-esf-schema-format.md) |
