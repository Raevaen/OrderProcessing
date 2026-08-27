# syntax=docker/dockerfile:1.7

# ── Shared build base ──
FROM rust:1.89-slim-bookworm AS base
WORKDIR /app
RUN cargo install --locked cargo-chef

# ── Dependency planner ──
FROM base AS planner
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo chef prepare --recipe-path recipe.json

# ── Build stage ──
FROM base AS build
COPY --from=planner /app/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo chef cook --release --locked --recipe-path recipe.json

COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo build --release --locked \
    && strip target/release/order-processor

# ── Runtime stage ──
FROM gcr.io/distroless/cc-debian12:nonroot AS final
COPY --from=build /app/target/release/order-processor /usr/local/bin/order-processor
USER nonroot:nonroot
ENTRYPOINT ["/usr/local/bin/order-processor"]
