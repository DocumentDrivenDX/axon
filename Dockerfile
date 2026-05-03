# Axon server — multi-stage Docker build
#
# Build:  docker build -t axon .
# Run:    docker run -p 4170:4170 -v axon-data:/var/lib/axon axon
#
# The binary is `axon` (from crates/axon-cli, the unified binary).
# Default HTTP port is 4170. gRPC is opt-in via --grpc-port.
# Health check: GET /health

# ── Stage 1: Playwright E2E runner ────────────────────────────────────────────

FROM oven/bun:1.3.11 AS bun-runtime

FROM mcr.microsoft.com/playwright:v1.59.1-noble AS ui-e2e-runner

COPY --from=bun-runtime /usr/local/bin/bun /usr/local/bin/bun

# ── Stage 2: UI builder ───────────────────────────────────────────────────────

FROM oven/bun:1.3.11 AS ui-builder

WORKDIR /usr/src/axon

COPY ui ./ui

RUN cd ui && bun install --frozen-lockfile && bun run build

# ── Stage 3: Rust dependency planner ──────────────────────────────────────────

FROM rust:1.94-bookworm AS rust-base

# protobuf-compiler is required by tonic-build for gRPC code generation.
RUN apt-get update && apt-get install -y --no-install-recommends \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

RUN cargo install --locked cargo-chef

WORKDIR /usr/src/axon

FROM rust-base AS planner

# Copy only Cargo metadata, the pinned toolchain, and build-time protobuf inputs so dependency
# compilation stays cached when UI files or Rust sources change.
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/axon-api/Cargo.toml crates/axon-api/Cargo.toml
COPY crates/axon-audit/Cargo.toml crates/axon-audit/Cargo.toml
COPY crates/axon-cli/Cargo.toml crates/axon-cli/Cargo.toml
COPY crates/axon-config/Cargo.toml crates/axon-config/Cargo.toml
COPY crates/axon-control-plane/Cargo.toml crates/axon-control-plane/Cargo.toml
COPY crates/axon-core/Cargo.toml crates/axon-core/Cargo.toml
COPY crates/axon-cypher/Cargo.toml crates/axon-cypher/Cargo.toml
COPY crates/axon-graphql/Cargo.toml crates/axon-graphql/Cargo.toml
COPY crates/axon-mcp/Cargo.toml crates/axon-mcp/Cargo.toml
COPY crates/axon-render/Cargo.toml crates/axon-render/Cargo.toml
COPY crates/axon-schema/Cargo.toml crates/axon-schema/Cargo.toml
COPY crates/axon-server/Cargo.toml crates/axon-server/Cargo.toml
COPY crates/axon-server/build.rs crates/axon-server/build.rs
COPY crates/axon-server/proto/axon.proto crates/axon-server/proto/axon.proto
COPY crates/axon-sim/Cargo.toml crates/axon-sim/Cargo.toml
COPY crates/axon-storage/Cargo.toml crates/axon-storage/Cargo.toml

RUN mkdir -p crates/axon-cli/src \
    && printf '%s\n' 'fn main() {}' > crates/axon-cli/src/main.rs \
    && mkdir -p crates/axon-api/benches \
    && printf '%s\n' 'fn main() {}' > crates/axon-api/benches/benchmarks.rs \
    && for crate in \
        axon-api \
        axon-audit \
        axon-config \
        axon-control-plane \
        axon-core \
        axon-cypher \
        axon-graphql \
        axon-mcp \
        axon-render \
        axon-schema \
        axon-server \
        axon-sim \
        axon-storage \
    ; do \
        mkdir -p "crates/${crate}/src"; \
        printf '%s\n' 'pub fn cargo_chef_placeholder() {}' > "crates/${crate}/src/lib.rs"; \
    done

RUN cargo chef prepare --recipe-path recipe.json

# ── Stage 4: Rust builder ─────────────────────────────────────────────────────

FROM rust-base AS builder

COPY rust-toolchain.toml ./
COPY --from=planner /usr/src/axon/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

# Copy the full workspace after dependencies are cooked so source and UI-only
# changes reuse the dependency layer.
COPY . .
COPY --from=ui-builder /usr/src/axon/ui/build ui/build

# Build the unified binary in release mode.
RUN cargo build --release -p axon-cli

# ── Stage 5: Runtime ──────────────────────────────────────────────────────────

FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libsqlite3-0 \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create a non-root user for running the server.
RUN groupadd --system axon && useradd --system --gid axon axon

# Persistent data directory for SQLite databases.
RUN mkdir -p /var/lib/axon && chown axon:axon /var/lib/axon

COPY --from=builder /usr/src/axon/target/release/axon /usr/local/bin/axon
COPY --from=ui-builder /usr/src/axon/ui/build /usr/share/axon/ui
COPY scripts/ /scripts/

USER axon

# HTTP gateway port (default 4170).
EXPOSE 4170

ENTRYPOINT ["axon"]

# Default: serve with SQLite storage, no authentication (dev mode).
# Override CMD or use environment variables for production configuration.
CMD ["serve", "--no-auth", "--storage", "sqlite", "--sqlite-path", "/var/lib/axon/axon.db", "--control-plane-path", "/var/lib/axon/axon-control-plane.db", "--ui-dir", "/usr/share/axon/ui"]
