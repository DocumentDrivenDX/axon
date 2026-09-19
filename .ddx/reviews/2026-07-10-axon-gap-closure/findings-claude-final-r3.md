# Claude Final Confirmation — Remaining Findings

Blocking findings:

- 10 MB entity payload conflicts with 10 MB expanded commit/audit cap.
- Integral float canonicalization depends on SDK token rendering.
- Blanket PostgreSQL SERIALIZABLE conflicts with ADR-004 SI default.
- Server-resolved per-batch handles lack bounded durable ACK/GC state.
- Slash-delimited core link keys are a finish-line-A bug, not replica-only.

Warnings accepted for correction: CONTRACT-007 overrides, global auth blast
radius, PRD citation, nonexistent Rust SDK wording, handler schema chokepoint,
server sealing call sites, managed-PG restore verification, explicit cursor
contract narrowing, tracker seed accuracy, new `ChangeBatch` type, and
dedicated benchmark hardware.

Verdict: BLOCK until corrected.
