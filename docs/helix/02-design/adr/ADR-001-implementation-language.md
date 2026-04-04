---
dun:
  id: ADR-001
  depends_on:
    - helix.prd
---
# ADR-001: Rust as Implementation Language

| Date | Status | Deciders | Related | Confidence |
|------|--------|----------|---------|------------|
| 2026-04-04 | Accepted | Erik LaBianca | All FEATs | High |

## Context

Axon is a transactional data store with ACID guarantees, deterministic simulation testing, and embedded + server deployment modes. The implementation language must support: memory safety without GC pauses, high-performance I/O, embeddability as a library, and a strong type system that catches bugs at compile time.

| Aspect | Description |
|--------|-------------|
| Problem | Choosing a language that supports correctness-first development, embeddability, and performance for a database engine |
| Current State | Team has production Rust experience (niflheim) and Go experience (DDx) |
| Requirements | Memory safety, no GC pauses, embeddable as library, async I/O, strong ecosystem for database internals |

## Decision

We will implement Axon in **Rust**.

**Key Points**: Memory safety without GC | Embeddable as native library | Ecosystem for database internals (serde, tokio, SQLite bindings)

## Alternatives

| Option | Pros | Cons | Evaluation |
|--------|------|------|------------|
| Go | Fast compilation, simpler concurrency model, team expertise (DDx) | GC pauses hurt tail latency, harder to embed as library in non-Go consumers, weaker type system for invariant enforcement | Rejected: GC pauses and embedding limitations |
| C++ | Maximum performance, mature database ecosystem (RocksDB, LevelDB) | Memory unsafety, build system complexity, slower development velocity | Rejected: memory safety risks unacceptable for correctness-first project |
| **Rust** | Memory safety at compile time, no GC, embeddable via C ABI/FFI, strong type system, async ecosystem (tokio), serde for serialization | Steeper learning curve, longer compile times, smaller talent pool | **Selected**: best fit for correctness-first database with embedded mode |

## Consequences

| Type | Impact |
|------|--------|
| Positive | Compile-time memory safety eliminates a class of bugs. No GC means predictable latency. Embeddable via C ABI for any language. Niflheim experience transfers directly |
| Negative | Longer compile times. Smaller hiring pool. Some ecosystem gaps vs Go/Java |
| Neutral | Client SDKs (Go, TypeScript, Python) are unaffected — they consume the gRPC/HTTP API |

## Implementation Impact

| Aspect | Assessment |
|--------|------------|
| Effort | Low - team already has Rust production experience from niflheim |
| Skills | Rust, async (tokio), database internals |
| Performance | Excellent - no GC pauses, zero-cost abstractions, predictable memory usage |
| Scalability | Strong async runtime (tokio) for concurrent connections |
| Security | Memory safety eliminates buffer overflows, use-after-free, data races |

## Risks

| Risk | Prob | Impact | Mitigation |
|------|------|--------|------------|
| Compile times slow iteration | Medium | Low | Incremental compilation, workspace structure, cargo-nextest for fast test runs |
| Ecosystem gaps for specific needs | Low | Medium | Niflheim has already solved most Rust database-ecosystem challenges |

## Dependencies

- **Technical**: Rust stable toolchain, tokio async runtime, serde, tonic (gRPC)
- **Decisions**: None (foundational decision)

## Validation

| Success Metric | Review Trigger |
|----------------|----------------|
| Build time < 2 min incremental | If builds consistently exceed this, investigate workspace structure |
| No memory-safety CVEs | If a memory safety issue is found, review unsafe usage |

## References

- Niflheim (~/Projects/niflheim) — production Rust database engine
- FoundationDB correctness approach — motivation for choosing a language with strong safety guarantees
