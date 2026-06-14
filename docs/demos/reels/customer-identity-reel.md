# Customer Identity Graph Reel

Release target: Axon 0.7.1

CRM, CDP, and MDM flows for contact merge, identity resolution, golden record survivorship, relationship traversal, and provenance.

Sample project: `examples/customer-identity-graph`

## Storyboard

1. Load duplicate contact records and company links.
2. Resolve both contacts into a profile with confidence metadata.
3. Traverse the contact-company-profile graph and verify no orphaned links after merge.
4. Inspect the audit trail for explainable identity decisions.

## Coverage Entries

- feature: FEAT-007 - Entity-Graph Data Model
- feature: FEAT-020 - Link Discovery and Graph Queries
- scenario: SCN-002 - CRM - Contact Merge (Duplicate Resolution)
- scenario: SCN-003 - CDP - Identity Resolution and Profile Merge
- scenario: SCN-008 - MDM - Golden Record Merge with Survivorship
- story: US-017 - Model Entities with Nested Structure
- story: US-018 - Create and Traverse Links
- story: US-019 - Query Across Entity-Link Graph
- use_case: USE-001 - CRM (Customer Relationship Management)
- use_case: USE-002 - CDP (Customer Data Platform)
- use_case: USE-008 - MDM (Master Data Management)
