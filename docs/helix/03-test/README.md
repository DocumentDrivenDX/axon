# Phase 3: Test

**Project**: Axon
**Last Updated**: 2026-06-10

## Overview

Test specifications that define Axon's correctness guarantees. Per Principle P1, these artifacts govern the implementation — code exists to pass these tests.

## Contents

- [Test Plan](test-plan.md) — Master test plan: invariants (L1), business scenarios (L2), property tests (L3), backend conformance (L4), benchmarks (L5), API contracts (L6)
- [Feature Story and E2E Traceability Review](feature-story-e2e-traceability.md) — Superseded; kept as a pointer to Test Plan §3 and the STPs, which now own AC↔test traceability
- `test-plans/` — Per-feature/story test plans (in authoring)

## Key Principle

> "Test suite first, implementation second." — Axon Principle P1

The test plan is a higher-authority artifact than the implementation. If the tests pass but the behavior seems wrong, fix the tests (they define correctness). If the implementation passes the tests, it is correct by definition.
