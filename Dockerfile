# Axon server — multi-stage Docker build
#
# Build:  docker build -t axon .
# Run:    docker run -p 4170:4170 -v axon-data:/var/lib/axon axon
#
# The binary is `axon` (from crates/axon-cli, the unified binary).
# Default HTTP port is 4170. gRPC is opt-in via --grpc-port.
# Health check: GET /health

# ── Stage 1: Builder ──────────────────────────────────────────────────────────

FROM rust:1.94-bookworm AS builder

# protobuf-compiler is required by tonic-build for gRPC code generation.
RUN apt-get update && apt-get install -y --no-install-recommends \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/axon

# Copy the full workspace. The .dockerignore keeps out target/, node_modules/,
# .git/, etc. to keep the build context small.
COPY . .

# Build the unified binary in release mode.
RUN cargo build --release -p axon-cli

# ── Stage 2: Runtime ──────────────────────────────────────────────────────────

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
COPY scripts/ /scripts/

USER axon

# HTTP gateway port (default 4170).
EXPOSE 4170

ENTRYPOINT ["axon"]

# Default: serve with SQLite storage, no authentication (dev mode).
# Override CMD or use environment variables for production configuration.
CMD ["serve", "--no-auth", "--storage", "sqlite", "--sqlite-path", "/var/lib/axon/axon.db"]
