---
title: Customer Identity Graph Reel
weight: 12
prev: ../
---

# Customer Identity Graph Reel

Release target: Axon 0.7.1

CRM, CDP, and MDM flows for contact merge, identity resolution, golden record survivorship, relationship traversal, and provenance.

- Sample project: [customer-identity-graph](https://github.com/DocumentDrivenDX/axon/tree/master/examples/customer-identity-graph)
- Script source: [`docs/demos/reels/customer-identity-reel.md`](https://github.com/DocumentDrivenDX/axon/blob/master/docs/demos/reels/customer-identity-reel.md)
- Coverage entries: 11

## Storyboard

1. Load duplicate contact records and company links.
2. Resolve both contacts into a profile with confidence metadata.
3. Traverse the contact-company-profile graph and verify no orphaned links after merge.
4. Inspect the audit trail for explainable identity decisions.

## Covered HELIX Entries

| Type | ID | Title | Source | Sample | Demo reel |
|---|---|---|---|---|---|
| feature | FEAT-007 | Entity-Graph Data Model | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/features/FEAT-007-entity-graph-model.md) | [customer-identity-graph](https://github.com/DocumentDrivenDX/axon/tree/master/examples/customer-identity-graph) | [customer-identity-reel](../customer-identity-reel/) |
| feature | FEAT-020 | Link Discovery and Graph Queries | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/features/FEAT-020-link-discovery-and-graph-queries.md) | [customer-identity-graph](https://github.com/DocumentDrivenDX/axon/tree/master/examples/customer-identity-graph) | [customer-identity-reel](../customer-identity-reel/) |
| scenario | SCN-002 | CRM - Contact Merge (Duplicate Resolution) | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/03-test/test-plan.md) | [customer-identity-graph](https://github.com/DocumentDrivenDX/axon/tree/master/examples/customer-identity-graph) | [customer-identity-reel](../customer-identity-reel/) |
| scenario | SCN-003 | CDP - Identity Resolution and Profile Merge | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/03-test/test-plan.md) | [customer-identity-graph](https://github.com/DocumentDrivenDX/axon/tree/master/examples/customer-identity-graph) | [customer-identity-reel](../customer-identity-reel/) |
| scenario | SCN-008 | MDM - Golden Record Merge with Survivorship | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/03-test/test-plan.md) | [customer-identity-graph](https://github.com/DocumentDrivenDX/axon/tree/master/examples/customer-identity-graph) | [customer-identity-reel](../customer-identity-reel/) |
| story | US-017 | Model Entities with Nested Structure | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-017-model-entities-with-nested-structure.md) | [customer-identity-graph](https://github.com/DocumentDrivenDX/axon/tree/master/examples/customer-identity-graph) | [customer-identity-reel](../customer-identity-reel/) |
| story | US-018 | Create and Traverse Links | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-018-create-and-traverse-links.md) | [customer-identity-graph](https://github.com/DocumentDrivenDX/axon/tree/master/examples/customer-identity-graph) | [customer-identity-reel](../customer-identity-reel/) |
| story | US-019 | Query Across Entity-Link Graph | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/01-frame/user-stories/US-019-query-across-entity-link-graph.md) | [customer-identity-graph](https://github.com/DocumentDrivenDX/axon/tree/master/examples/customer-identity-graph) | [customer-identity-reel](../customer-identity-reel/) |
| use case | USE-001 | CRM (Customer Relationship Management) | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/00-discover/use-case-research.md) | [customer-identity-graph](https://github.com/DocumentDrivenDX/axon/tree/master/examples/customer-identity-graph) | [customer-identity-reel](../customer-identity-reel/) |
| use case | USE-002 | CDP (Customer Data Platform) | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/00-discover/use-case-research.md) | [customer-identity-graph](https://github.com/DocumentDrivenDX/axon/tree/master/examples/customer-identity-graph) | [customer-identity-reel](../customer-identity-reel/) |
| use case | USE-008 | MDM (Master Data Management) | [source](https://github.com/DocumentDrivenDX/axon/blob/master/docs/helix/00-discover/use-case-research.md) | [customer-identity-graph](https://github.com/DocumentDrivenDX/axon/tree/master/examples/customer-identity-graph) | [customer-identity-reel](../customer-identity-reel/) |
