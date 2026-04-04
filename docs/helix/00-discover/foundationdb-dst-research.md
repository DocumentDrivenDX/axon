---
dun:
  id: helix.foundationdb-dst-research
  depends_on:
    - helix.product-vision
---
# Research: FoundationDB's Approach to Correctness and Deterministic Simulation Testing

**Version**: 0.1.0
**Date**: 2026-04-04
**Status**: Draft
**Author**: Erik LaBianca

---

## 1. The Simulator-First Philosophy

FoundationDB's founding team spent approximately **two years** building a deterministic simulation framework before writing any database code. The simulator itself was debugged and hardened for years so that debugging the database later would be tractable.

The philosophy: if you cannot reproduce a bug deterministically, you cannot fix it with confidence. Distributed systems produce failures that are combinatorially complex, timing-dependent, and nearly impossible to reproduce in production. Rather than chase heisenbugs, FDB made the entire execution environment deterministic from day one.

### What "correctness properties first" looked like in practice

- **Workloads define invariants, not just operations.** Each test workload specifies constraints verified after execution. The canonical example is the **Cycle test**: N key-value pairs form a ring. Transactions swap edges while preserving ring structure. After chaos, the CHECK phase walks the ring -- if it takes exactly N hops to return to the start, transactional isolation held. Fewer hops means a split cycle (isolation violation).
- **Workloads are composable.** TOML config files combine application workloads (Cycle, RandomReadWrite) with fault workloads (RandomClogging, Attrition, SwizzledDisk). This separates "what correctness means" from "what goes wrong."
- **Four-phase structure**: SETUP (initialize data), EXECUTION (concurrent transactions under chaos), CHECK (verify invariants), METRICS (report performance). Every workload implements all four.

### Key insight

They did not define correctness as "the system stays up." They defined it as "these specific invariants hold across all possible failure sequences." The simulator then explored the failure space exhaustively.

---

## 2. Deterministic Simulation Testing: How It Works

### Architecture

The entire FoundationDB cluster -- multiple storage servers, transaction logs, coordinators, proxies -- runs inside a **single-threaded process**. No real network. No real disk. No real clock.

This is enabled by **Flow**, a syntactic extension to C++ that adds `ACTOR` coroutines with `wait()` suspension (similar to async/await). Flow compiles to standard C++ but provides cooperative multitasking on a single event loop.

### The two runtimes

| | Production (`Net2`) | Simulation (`Sim2`) |
|---|---|---|
| Network | Real TCP sockets | In-memory buffers |
| Disk | Real filesystem | Simulated with configurable latency, failures, capacity |
| Clock | `gettimeofday()` | Virtual, advances discretely |
| Randomness | System PRNG | `deterministicRandom()` with known seed |

The same Flow code runs on both runtimes with zero changes. This is the critical design constraint -- you are always testing production code.

### How the event loop works

1. Run all ready actors until they hit `wait()`
2. When all actors are waiting, find the next scheduled event
3. Jump the simulated clock to that event's timestamp
4. Wake actors and repeat

This means 100 storage servers executing `wait(delay(random01() * 60.0))` advance 60 simulated seconds in microseconds of wall time. The compression ratio is roughly **10:1 to 1000:1** depending on workload.

### What makes it different from integration testing

| Integration testing | Deterministic simulation |
|---|---|
| Real processes, real network | Single process, simulated everything |
| Non-deterministic timing | Identical execution given same seed |
| Minutes per test, few failure scenarios | Seconds per test, millions of failure combinations |
| Bug reproduction: "try again and hope" | Bug reproduction: re-run with same seed |
| Tests a single configuration | Randomized knobs create unique environments per seed |

---

## 3. BUGGIFY: Fault Injection from the Inside

BUGGIFY is not an external fault injector. It is **hundreds of conditional code blocks embedded throughout the production codebase** that activate only in simulation.

### Mechanics

```cpp
// BUGGIFY evaluates to true ONLY in simulation
// First evaluation: randomly enabled/disabled for the entire run
// If enabled: 25% chance of firing on each evaluation
if (BUGGIFY) {
    // inject unusual-but-legal behavior
}

// Variant with custom probability
if (BUGGIFY_WITH_PROB(0.01)) {
    // 1% chance when enabled
}
```

### What BUGGIFY injects

| Category | Example |
|---|---|
| **Synthetic errors** | Return an error from an operation that usually succeeds |
| **Artificial delays** | `wait(delay(deterministicRandom()->random01() * 10.0))` in hot paths |
| **Timeout compression** | Shrink `DD_SHARD_METRICS_TIMEOUT` from 60s to 0.1s |
| **Intentional hangs** | Replace a future with `Never()` to force timeout code paths |
| **Knob randomization** | Randomize tuning parameters (buffer sizes, batch counts, retry limits) |
| **Disk swaps** | On simulated reboot, swap a storage server's disk 75% of the time (tests amnesia) |
| **Workflow restarts** | Randomly restart multi-stage operations mid-way |

### Why it works

Each BUGGIFY site is independently enabled/disabled per simulation run. With hundreds of sites, the combinatorial space is enormous. A single 30-second simulated test can encounter **187 network partitions** with triple replication. Every seed explores a different corner of the state space.

### Design rule

BUGGIFY injects behavior that is **unusual but not contract-breaking**. Returning an error is legal (callers must handle errors). Adding delay is legal (the network is asynchronous). Shrinking a timeout is legal (timeouts are not correctness guarantees). This means BUGGIFY cannot cause false positives -- any invariant violation is a real bug.

---

## 4. Scale of Testing and Results

### Testing volume

- **One trillion estimated CPU-hours** of accumulated simulation testing
- **Hundreds of thousands** of simulation tests run per pull request (on hundreds of cores, for hours) before human code review begins
- **Tens of thousands** of nightly simulation runs
- Separate **Circus** environment runs automated performance regression tests nightly

### Results

- After years of on-call, engineers report **never being woken up for a FoundationDB outage**
- Early FDB depended on Apache ZooKeeper for coordination. Real-world fault injection found **two independent bugs in ZooKeeper** (~2010), leading them to replace it with a de novo Paxos implementation in Flow. No production coordination bugs reported since.
- The SIGMOD 2021 paper states simulation testing enables "rapid cadence" of new features and releases with high confidence

### Known limitations (from the SIGMOD paper)

- **Cannot reliably detect performance issues** (e.g., suboptimal load balancing)
- **Cannot test third-party libraries or OS-level code** not written in Flow
- **Filesystem/OS contract misunderstandings** can slip through (simulation trusts its model of the OS)
- **No formal verification** -- simulation is probabilistic exploration, not proof

---

## 5. Lessons Learned and Key References

### What worked

1. **Building the simulator first** was the single most important decision. It shaped every subsequent architectural choice.
2. **Deterministic replay** transforms debugging from "stare at logs and guess" to "set a breakpoint and step through."
3. **BUGGIFY inside production code** ensures fault handling is tested where it matters -- not in a separate mock.
4. **Composable workloads** let one test exercise correctness (Cycle) while another exercises faults (Attrition), combined arbitrarily.
5. **Running simulation on every PR** catches regressions before review begins.

### What was hard

1. **Everything must go through the runtime.** Any direct system call (raw `malloc`, `gettimeofday`, `rand()`) breaks determinism. This is an all-or-nothing discipline.
2. **Flow was a major investment.** Building a custom language/runtime is a multi-year effort. (Will Wilson, FDB co-founder, later founded Antithesis to make this available as a service.)
3. **Performance testing requires a separate approach.** Simulation compresses time and runs single-threaded, making it unsuitable for performance analysis.
4. **Simulation fidelity is bounded.** The simulator's model of disk/network/OS behavior is an approximation. Bugs at the model boundary escape.

### Essential references

- **SIGMOD 2021 paper**: Zhou et al., "FoundationDB: A Distributed Unbundled Transactional Key Value Store" -- the definitive technical reference ([PDF](https://www.foundationdb.org/files/fdb-paper.pdf))
- **Will Wilson's Strange Loop 2014 talk**: "Testing Distributed Systems w/ Deterministic Simulation" -- the original public presentation of the approach ([notes](https://alex-ii.github.io/notes/2018/04/29/distributed_systems_with_deterministic_simulation.html))
- **Pierre Zemb's deep dive**: "Diving into FoundationDB's Simulation Framework" -- code-level walkthrough with examples ([blog](https://pierrezemb.fr/posts/diving-into-foundationdb-simulation/))
- **Pierre Zemb's DST resource list**: Curated links to talks, papers, and implementations ([blog](https://pierrezemb.fr/posts/learn-about-dst/))
- **FoundationDB official docs**: Simulation and Testing ([docs](https://apple.github.io/foundationdb/testing.html))
- **Antithesis**: Will Wilson's company productizing DST as a service ([antithesis.com](https://antithesis.com/docs/resources/deterministic_simulation_testing/))

---

## 6. Applicability to Rust: Frameworks and Trade-offs

FoundationDB required Flow (a custom C++ extension) because C++ lacked async/await and cooperative scheduling. Rust's async ecosystem provides these natively, making DST more accessible -- but with caveats.

### Framework comparison

| Framework | Approach | Fault injection | Tokio compat | Production use |
|---|---|---|---|---|
| **MadSim** | Full runtime replacement; intercepts libc calls (`getrandom`, `clock_gettime`). Drop-in Tokio API. | Network partitions, latency, process crashes via libc interception | Full API compat via `madsim-tokio`, `madsim-tonic` wrappers | **RisingWave** (streaming database) uses in CI |
| **Turmoil** | Simulated network layer for Tokio. Multiple hosts in one thread. | Network partitions, latency, link manipulation | Partial (network only; disk/time not simulated) | Tokio ecosystem; used by several projects |
| **Loom** | Exhaustive exploration of thread interleavings for `std::sync` primitives | Explores all possible schedules of atomic ops, mutexes, condvars | No (replaces `std::sync`, not `tokio`) | Tokio itself uses Loom for correctness |
| **Shuttle** | Randomized concurrency testing (like a probabilistic Loom) | Random thread scheduling with controlled seeds | No (similar scope to Loom) | AWS (created by Amazon) |

### Practical recommendations for a new Rust database

**MadSim is the closest to FoundationDB's approach.** It provides:
- Deterministic single-threaded execution of the full async stack
- libc interception for time and randomness (critical for determinism)
- Simulated network with fault injection
- Tokio API compatibility (switch via `RUSTFLAGS="--cfg madsim"`)
- Cargo `[patch]` for third-party crate compatibility

**Key challenges in Rust DST** (from RisingWave and S2 experience):
1. **Hidden non-determinism**: `HashMap` iteration order, timestamps in serialized protocols, any `getrandom` call from a dependency. MadSim's libc interception catches most but not all.
2. **Third-party crates**: Libraries with internal randomness or direct syscalls need patching. RisingWave maintains patched forks of several crates.
3. **BUGGIFY equivalent**: No Rust framework provides built-in BUGGIFY. You must build it yourself -- a `buggify!()` macro gated on `#[cfg(test)]` or a simulation feature flag, backed by the simulator's deterministic RNG.
4. **Disk simulation**: MadSim and Turmoil focus on network. Disk fault simulation (latency, corruption, capacity) requires custom implementation.
5. **CI integration**: S2 runs DST "on every PR, commit, and in thousands of nightly trials," catching 17 bugs before production. This cadence is achievable with Rust's compilation model.

### Suggested architecture for Axon

```
Production:   tokio runtime + real I/O
Simulation:   madsim runtime + simulated network/disk/time
                              + buggify!() macro for internal fault injection
                              + composable workloads with invariant checks
```

The runtime boundary should be an internal trait (like FDB's Net2/Sim2 split), not a framework dependency. This lets you swap simulators or move to Antithesis later without rewriting application code.

---

## Summary: What to Take from FDB for Axon

| FDB principle | Axon application |
|---|---|
| Build the simulator first | Define the runtime abstraction layer and simulation harness before storage engine internals |
| Correctness = invariants that hold under chaos | Define Axon's invariants (audit trail completeness, schema enforcement, transaction isolation) as executable checks |
| BUGGIFY inside production code | `buggify!()` macro throughout Axon, deterministically controlled, zero-cost in production |
| Composable workloads | Separate "what the app does" from "what goes wrong" in test configuration |
| Run simulation on every PR | Hundreds of seeds per PR, thousands nightly; track seed coverage over time |
| Deterministic replay for debugging | Single seed reproduces any failure exactly; CI preserves failing seeds as regression tests |
