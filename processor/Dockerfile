# syntax=docker/dockerfile:1.7
# ── Build stage ──
FROM rust:1.95-slim-bookworm AS build
WORKDIR /app

# Pre-build an empty crate to cache dependency compilation.
# Cargo.lock is generated on first build — create a stub so the COPY doesn't fail.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
# Cache the cargo registry and the target dir across builds so dependencies
# aren't recompiled from scratch every time (only invalidated by Cargo.toml/lock changes).
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release && \
    cp target/release/order-processor /tmp/order-processor-stub || true
# The dummy binary is not needed, remove it.
RUN rm -f target/release/deps/processor* 2>/dev/null || true

# Copy real source and build.
COPY src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release && \
    cp target/release/order-processor /app/order-processor

# ── Runtime stage ──
FROM gcr.io/distroless/cc-debian12 AS final
COPY --from=build /app/order-processor /usr/local/bin/order-processor
ENTRYPOINT ["order-processor"]
