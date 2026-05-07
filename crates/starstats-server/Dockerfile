# syntax=docker/dockerfile:1.7
# Multi-stage build for the Axum API server.
# Final image: ~30 MB on debian:bookworm-slim with the compiled binary.

FROM rust:1.88-slim-bookworm@sha256:38bc5a86d998772d4aec2348656ed21438d20fcdce2795b56ca434cf21430d89 AS builder
WORKDIR /build

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Cache layer: copy only manifests, build a stub workspace, populate dep cache.
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates/starstats-core/Cargo.toml crates/starstats-core/
COPY crates/starstats-server/Cargo.toml crates/starstats-server/
COPY crates/starstats-client/Cargo.toml crates/starstats-client/

RUN mkdir -p crates/starstats-core/src crates/starstats-server/src/bin crates/starstats-client/src \
    && echo "" > crates/starstats-core/src/lib.rs \
    && echo "fn main() {}" > crates/starstats-server/src/main.rs \
    && echo "fn main() {}" > crates/starstats-server/src/bin/openapi.rs \
    && echo "fn main() {}" > crates/starstats-client/src/main.rs \
    && cargo build --release -p starstats-server \
    && rm -rf crates/starstats-core/src crates/starstats-server/src crates/starstats-client/src

# Real source — touch invalidates the stub artefacts so cargo recompiles.
COPY crates/starstats-core crates/starstats-core
COPY crates/starstats-server crates/starstats-server

RUN cargo build --release -p starstats-server

# Runtime image — minimal, has wget for HEALTHCHECK.
FROM debian:bookworm-slim@sha256:f9c6a2fd2ddbc23e336b6257a5245e31f996953ef06cd13a59fa0a1df2d5c252 AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates wget tini \
    && rm -rf /var/lib/apt/lists/*

RUN groupadd --system --gid 65532 starstats \
    && useradd --system --uid 65532 --gid starstats --no-create-home starstats

COPY --from=builder /build/target/release/starstats-server /usr/local/bin/starstats-server

USER starstats:starstats
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=30s --retries=3 \
    CMD wget --quiet --spider http://localhost:8080/healthz || exit 1

ENTRYPOINT ["tini", "--", "/usr/local/bin/starstats-server"]
