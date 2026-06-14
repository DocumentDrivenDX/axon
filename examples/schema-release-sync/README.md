# Schema Release Sync

Schema evolution, secondary indexes, validation rules, CDC, markdown templates, git mirror output, and rollback preview.

- Release target: Axon 0.7.1
- Persona: Developers shipping schema-backed applications.
- Demo reel: `schema-release-reel`
- Website page: `website/content/docs/demo-reels/schema-release-reel.md`
- Coverage entries: 42

## Files

- `schemas/`: JSON Schemas for every collection in the example.
- `seed/`: JSONL seed data by collection.
- `demo.sh`: CLI script that loads schemas, entities, links, and representative queries.

## Workflow

1. Apply a compatible schema change, then dry-run a breaking change.
2. Render a document through a markdown template.
3. Emit a CDC event and mirror the rendered entity to a git path.
4. Preview rollback to an earlier revision before applying it.

## Covered HELIX Entries

- feature: FEAT-002 - Schema Engine
- feature: FEAT-013 - Secondary Indexes and Query Acceleration
- feature: FEAT-017 - Schema Evolution and Migration
- feature: FEAT-019 - Validation Rules and Actionable Errors
- feature: FEAT-021 - Change Feeds (CDC)
- feature: FEAT-023 - Rollback and Recovery
- feature: FEAT-026 - Markdown Template Rendering
- feature: FEAT-027 - Git Mirror
- scenario: SCN-009 - Document Management - Version Chain
- story: US-004 - Define a Collection Schema
- story: US-005 - Get Clear Validation Errors
- story: US-006 - Inspect a Schema
- story: US-031 - Declare a Secondary Index
- story: US-032 - Enforce Uniqueness via Index
- story: US-033 - Compound Index for Multi-Field Queries
- story: US-034 - Background Index Build
- story: US-058 - Detect Breaking Schema Changes
- story: US-059 - Force-Apply a Breaking Change
- story: US-060 - Revalidate Entities Against Current Schema
- story: US-061 - View Schema Diff
- story: US-066 - Cross-Field Validation Rules
- story: US-067 - Validation Gates
- story: US-068 - Actionable Error Messages
- story: US-069 - Validate Rules on Schema Save
- story: US-074b - Query by Gate Status
- story: US-095 - Preview Recovery Before Commit
- story: US-096 - Revert One Entity Safely
- story: US-097 - Undo a Bad Transaction or Time Window
- story: US-125 - Lazy-Read Schema Migration
- story: US-130 - Emit CDC Events to Kafka
- story: US-132 - Replay Events from a Point in Time
- story: US-133 - Define a Markdown Template
- story: US-135 - Discover Entity Schemas via Registry
- story: US-136 - Render an Entity as Markdown
- story: US-137 - Stream Changes Without Kafka
- story: US-138 - Template Survives Schema Evolution
- story: US-139 - Link Events in CDC
- story: US-140 - Enable Git Mirror on a Collection
- story: US-141 - Entity Changes Appear as Git Commits
- story: US-142 - Shard Strategy Organises the Repository
- story: US-143 - Mirror Resumes After Failure
- use_case: USE-007 - Document Management
