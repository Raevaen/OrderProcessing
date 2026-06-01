# ── Build stage ──
FROM rust:1.95-slim-bookworm AS build
WORKDIR /app

# Pre-build an empty crate to cache dependency compilation.
# Cargo.lock is generated on first build — create a stub so the COPY doesn't fail.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release
# The dummy binary is not needed, remove it.
RUN rm -f target/release/deps/processor*

# Copy real source and build.
COPY src ./src
RUN cargo build --release

# ── Runtime stage ──
FROM gcr.io/distroless/cc-debian12 AS final
COPY --from=build /app/target/release/order-processor /usr/local/bin/order-processor
ENTRYPOINT ["order-processor"]
