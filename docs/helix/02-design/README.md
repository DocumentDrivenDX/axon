# Phase 2: Design

**Project**: Axon
**Last Updated**: 2026-04-22

## Overview

Design artifacts capturing architecture decisions and technical spikes.

## Contents

### Architecture Decision Records
- [ADR-001: Rust as Implementation Language](adr/ADR-001-implementation-language.md)
- [ADR-002: Schema Format — JSON Schema + Link-Type Definitions](adr/ADR-002-schema-format.md)
- [ADR-003: Backing Store Architecture — SQLite + PostgreSQL with Application-Layer Audit](adr/ADR-003-backing-store-architecture.md)
- [ADR-019: Policy Authoring and Mutation Intents](adr/ADR-019-policy-authoring-and-intents.md)

### Technical Spikes
- [SPIKE-001: Backing Store Evaluation](spikes/SPIKE-001-backing-store-evaluation.md) — PostgreSQL, SQLite, FoundationDB, fjall benchmarks

## Conventions

- ADRs are numbered sequentially: `ADR-XXX-short-name.md`
- Spikes are numbered sequentially: `SPIKE-XXX-short-name.md`
- ADRs trace back to PRD requirements and feature specs
