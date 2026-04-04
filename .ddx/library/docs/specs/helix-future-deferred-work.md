# Proposal: HELIX Parking Lot (Deferred/Future Work)

## Context
HELIX templates currently handle **Out of Scope** but do not offer a structured, non‑inline place to record work that is intentionally deferred. Adding deferred items inside core specs bloats context and reduces signal. We need a standard way to capture future/deferred work **without polluting core artifacts**.

## Goals
- Provide a **single, dedicated artifact** to capture future/deferred work.
- Keep **core templates clean** (no new inline sections required).
- Make deferred items **traceable** (source, rationale, triggers, dependencies).
- Establish a **consistent format and naming** for deferred work.
- Allow **any HELIX artifact** (feature specs, user stories, ADRs, solution designs, implementation plans, etc.) to be marked as parking‑lot while staying out of the main PRD flow.

## Non‑Goals
- No new enforcement or CI gates.
- No auto‑sync to bd/Jira.
- No changes to HELIX phase/state logic.

## Definitions
- **Out of Scope**: Explicitly excluded from current initiative; not planned.
- **Deferred Work**: Planned but intentionally postponed due to constraints.
- **Future Work**: Candidate ideas or enhancements not yet scheduled.

## Proposed Artifact
### New Project‑Level Artifact
- **Location**: `docs/helix/parking-lot.md`
- **Type**: HELIX project‑level artifact (cross‑phase)
- **Purpose**: Single source of truth for deferred/future work items across the project.

### Proposed Frontmatter (template)
```yaml
---
dun:
  id: helix.parking-lot
  depends_on:
    - helix.prd
  parking_lot: true
---
```

### Proposed Structure (template)
```markdown
# Parking Lot (Deferred / Future Work)

## Purpose
Define where deferred and future work is captured, and how it gets revisited.

## Policy
- **Out of Scope** items do not belong here.
- **Deferred** items must include a trigger and rationale.
- **Future** items must include a source and expected value.

## Deferred / Future Items
Use short, editable entries (list format) instead of wide tables.

### [Item Title]
- **Type**: Deferred | Future
- **Artifact Type**: Feature Spec | User Story | ADR | Solution Design | Implementation Plan | Other
- **Source**: FEAT-XXX / US-XXX / ADR-XXX / external (e.g., support ticket, customer note, roadmap item)
- **Rationale**: [Why it was deferred / why it is future work]
- **Impact if Omitted**: [Risk/impact]
- **Dependencies**: [Blocked by / prerequisites]
- **Revisit Trigger**: [What must happen before reconsidering]
- **Target Phase/Milestone**: [Phase or release]
- **Artifact File**: [Path to parked artifact, if any]
- **Link/Owner**: [Issue link; owner optional]
- **Last Reviewed**: [YYYY-MM-DD, optional]

## Parking Lot Full Artifacts (Optional)
If a deferred item warrants a full artifact, keep the **full document in its own file** and mark it as parking‑lot. The parking lot registry should link to it. Parked artifacts should remain in their **normal HELIX directories** (do not move them into a special folder).

### Rules
- Use the normal artifact format (FEAT/US/ADR/SD/IMP/etc.), but include a **Parking Lot marker** in the header and `parking_lot: true` in frontmatter.
- The artifact remains **out of the main PRD flow** to avoid confusion.
- In the Deferred/Future list, add an entry pointing to the full artifact file.

### Example Header
```markdown
## Parking Lot Feature Spec: FEAT-042-sso-just-in-time-provisioning
**Status**: Parking Lot (Deferred)
**Reason**: [Why it stays out of PRD flow]

## Parking Lot ADR: ADR-017-secrets-rotation
**Status**: Parking Lot (Future)
**Reason**: [Why it stays out of PRD flow]
```

## Review Cadence
- **Default**: Review at each phase transition and before planning new features.
- **Owner**: Product Owner (no per‑item owner required).

## Unparking (Promotion) Process
When a parked item becomes active, update **all** of the following:
1. **Parked artifact file**: remove `dun.parking_lot: true` and update the header status to active (e.g., Draft/Specified/Approved).
2. **Core flow references**: add the artifact to the appropriate in‑scope flow (e.g., PRD section, Feature Registry, Story lists, ADR index).
3. **Parking lot registry**: remove the entry immediately (the move is complete).
4. **Dependencies/DAG**: re‑generate or re‑validate dependency graphs to include the now‑active artifact.

## Notes
[Any extra context or decisions]
```

## Templates & Prompts
Add a new HELIX artifact template and prompt:
- `workflows/helix/phases/01-frame/artifacts/parking-lot/template.md`
- `workflows/helix/phases/01-frame/artifacts/parking-lot/prompt.md`
- `workflows/helix/phases/01-frame/artifacts/parking-lot/meta.yml`

Template output location should be `docs/helix/parking-lot.md` (project‑level, cross‑phase).

Add a lightweight discoverability hook:
- Update the PRD template to include a one‑line pointer to `docs/helix/parking-lot.md` (no inline deferred content).

Prompt guidance should:
- Emphasize **no inline deferred sections** in core specs.
- Require **source references** (e.g., FEAT/US/ADR links).
- Require **revisit triggers**.
- Call out that **any HELIX artifact type** can be parked here.

## Conventions & Documentation
Update:
- `workflows/helix/conventions.md` to list `parking-lot.md` under the `docs/helix/` root and define usage.
- (Optional) `workflows/helix/artifact-hierarchy.md` to mention the parking lot as a **cross‑cutting deferred work registry**.

## Automation Notes
- Dependency DAG tooling should **exclude** `docs/helix/parking-lot.md` (registry only).
- For other artifacts, `dun.parking_lot: true` is **authoritative** for exclusion from dependency graphs.
- `Status: Parking Lot` is human‑readable and should **not** override frontmatter.
- Tooling must ignore parked artifacts **regardless of directory location** (even if they live in normal phase folders).

## Open Questions
1. None (parking lot is optional by default).

## Acceptance Criteria
- A **new parking‑lot artifact template** is added (project‑level under `docs/helix/`).
- Conventions document explains **how to use** the parking lot.
- Prompts require **source + trigger** for each item.
- Core templates remain unchanged (no inline Future/Deferred sections).
- PRD template includes a **single‑line pointer** to the parking lot (no inline content).
- Dependency DAG guidance **excludes parking‑lot items** from dependency graphs.
- Parked full artifacts live in their **own files** and are referenced from the parking lot registry.
- Add a **dun-side DAG exclusion test** proving a parked artifact in a normal phase directory is ignored when `dun.parking_lot: true` is set.

## Migration
- No backfill required.
- When deferring work, add an entry to the parking lot and reference the source artifact.
- When work is scheduled, either:
  - Create or update the full artifact in its normal HELIX location and remove `dun.parking_lot: true`, or
  - Move the registry entry into active flow by adding it to the PRD/registries and removing it from the parking lot.
