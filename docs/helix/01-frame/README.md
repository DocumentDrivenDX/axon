# Frame Documents

**Project**: Axon
**Last Updated**: 2026-06-10

## Overview

This directory captures product framing and context for Axon.

## Contents

- [Product Requirements](prd.md) — authoritative functional requirements (FR-n)
- [Principles](principles.md)
- [Feature Registry](feature-registry.md) — canonical catalog of feature IDs, status, priority, dependencies, and trace links
- [Feature Specs](features/README.md) — one spec per feature (`FEAT-XXX-*.md`)
- [User Stories](user-stories/README.md) — extracted story files plus the authoritative US-ID allocation ledger
- [Concerns](concerns.md) — project concern selection (operator slot overrides in `concerns.local.yml`)
- [Security Requirements](security-requirements.md) and [Threat Model](threat-model.md) — security framing (in authoring)
- [Technical Requirements](technical-requirements.md) — deprecated; retained as a pointer (requirements live in the PRD, feature specs, and design contracts)
- [Parking Lot](../parking-lot.md) — deferred/parked ideas with revisit triggers

## Conventions

- Keep framing docs focused on scope, context, and intent.
- Feature specs live in `features/`; user stories live in `user-stories/`
  (one file per story), ledgered in `user-stories/README.md`.
- Feature identity, status, and traceability are maintained in
  `feature-registry.md`, not duplicated here.
