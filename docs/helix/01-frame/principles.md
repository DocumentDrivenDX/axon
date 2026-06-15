---
ddx:
  id: helix.principles
  depends_on: [helix.prd]
  review:
    self_hash: 68d05c2f025124f224f952adb2e7b93671c8f099011975fcbb3619e18fde38dd
    deps:
      helix.prd: dff98156a6cc934f406611b78b513892d85cee1bd7b4c011f045146fcdfd23e1
    reviewed_at: "2026-06-15T00:35:16Z"
---
# Axon Project Principles

These principles guide judgment calls across all HELIX activities. They are
lenses for choosing between two valid options, not requirements: acceptance
criteria live with their owning PRD functional requirements and FEAT specs,
and verification detail lives in test plans.

HELIX core principles (specification completeness, test-first development,
simplicity first, observable interfaces, continuous validation) apply through
the workflow itself and are not restated here; "simplicity first" as applied
to Axon means the well-lit path — schema plus basic CRUD with no extra
configuration — stays simple, with escape hatches available but never
required. Exceptions to principles, if ever needed, are tracked in the issue
tracker, not in this file.

## Principles

1. **Guardrails Are the Product** — Every mutation path goes through the same
   policy, intent, and audit enforcement; no surface — GraphQL, MCP, CLI, SDK,
   or embedded — bypasses the shared handler path, and parity is verified by
   shared fixtures. When a feature or optimization seems to need a bypass,
   change the handler path instead. (Owned by PRD FR-11, FR-12, FR-22, FR-28.)

2. **Test Suite First, Implementation Second** — FoundationDB-style: the
   correctness suite (deterministic simulation, fault injection,
   property-based tests) specifies behavior before implementation ships. When
   delivery speed and correctness evidence compete on governed behavior,
   strengthen the suite first. (Verification detail lives in FEAT acceptance
   criteria and test plans.)

3. **Audit is Not Optional** — The audit log is the architecture, not a
   feature; prefer designs where applying a mutation and producing its audit
   record are inseparable, even when a side channel would be cheaper. (Owned
   by PRD FR-15 through FR-17.)

4. **Entities and Links are the Model** — The world is things and
   relationships; when tempted by ad-hoc blobs or implicit join semantics,
   model the relationship as a typed, first-class, audited link instead.
   (Owned by PRD FR-1 through FR-3.)

5. **Transactions Mean Transactions** — If an operation can be partially
   applied, it is not a transaction; prefer rejecting a write with retryable
   context over weakening atomicity, isolation, or lost-update protection.
   (Owned by PRD FR-5 through FR-8.)

6. **Schema Earns Its Keep** — Every obligation a schema places on developers
   must pay for itself in validation, queryability, generated surfaces, or
   migration safety; if a schema feature does not return value, cut it rather
   than make declaration heavier. (Owned by PRD FR-1, FR-10, and the P1
   schema-evolution requirement.)

7. **Agents are First-Class Citizens** — Design APIs for programmatic
   consumption first: when human-UI conventions and machine-parseable
   structure conflict, prefer structured errors, self-describing schemas, and
   machine-checkable outcomes. (Owned by PRD FR-20 through FR-22, FR-29.)

8. **Local-First is a Requirement** — Governed local-first and embedded
   operation is a first-class deployment, not a bolt-on: prefer designs that
   keep schema, policy, transaction, audit, and query semantics
   location-transparent across embedded and server modes, work offline, and
   treat sync as something that must honor the same invariants. (Owned by PRD
   FR-23, FR-26, FR-32.)

## Tension Resolution

- **Simplicity first (HELIX core) vs Test Suite First (2)** — Simplicity
  argues for the minimal approach now; test-suite-first demands simulation,
  fault injection, and property tests before code. Resolution: simplicity
  governs product surface and implementation, never evidence. Rigor scales
  with governance: anything on the guarded mutation, policy, or audit path
  gets the full correctness suite first; non-invariant conveniences may ship
  with lighter verification.

- **Transactions Mean Transactions (5) vs Local-First is a Requirement (8)** —
  Strict ACID assumes one committing authority; offline clients accept writes
  that can conflict on sync. Resolution: ACID governs each node's committed
  state; cross-node convergence belongs to the sync protocol's deterministic
  conflict resolution, which surfaces conflicts through the same intent,
  policy, and audit machinery rather than silently merging. Never weaken
  single-node isolation to make sync easier, and never let sync apply a
  mutation that skips policy or audit.

- **Agents are First-Class Citizens (7) vs Guardrails Are the Product (1)** —
  Agent ergonomics push for fewer round-trips and frictionless writes;
  guardrails add preview, intent, and approval steps. Resolution: reduce
  friction by making the governed path more ergonomic — better explanations,
  structured retry context, discoverable approval requirements — never by
  offering a bypass. Low-risk writes may be streamlined within policy, but
  every write still traverses the shared handler path.
