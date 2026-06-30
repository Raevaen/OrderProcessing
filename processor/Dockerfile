# ── Build stage ──
FROM rust:1.89-slim-bookworm AS build
WORKDIR /app

# Copy source and build the real binary.
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked

# ── Runtime stage ──
FROM gcr.io/distroless/cc-debian12 AS final
COPY --from=build /app/target/release/order-processor /usr/local/bin/order-processor
ENTRYPOINT ["order-processor"]
