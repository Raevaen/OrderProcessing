# ── Build stage ──
FROM rust:1.78-slim-bookworm AS build
WORKDIR /app

# Pre-build an empty crate to cache dependency compilation.
# Cargo.lock is generated on first build — create a stub so the COPY doesn't fail.
COPY processor/Cargo.toml ./
RUN touch Cargo.lock && mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release 2>/dev/null || true
RUN rm -rf src

# Copy real source and build.
COPY processor/src ./src
RUN touch src/main.rs   # invalidate the dummy binary
RUN cargo build --release

# ── Runtime stage ──
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /app/target/release/order-processor /usr/local/bin/order-processor
ENTRYPOINT ["order-processor"]
