# syntax=docker/dockerfile:1

FROM lukemathwalker/cargo-chef:latest-rust-1.98 AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

COPY . .
RUN cargo build --release --bin accreta-metrics

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --no-create-home \
       --shell /usr/sbin/nologin appuser

WORKDIR /app

COPY --from=builder --chown=appuser:appuser \
    /app/target/release/accreta-metrics \
    /app/accreta-metrics

USER appuser:appuser

EXPOSE 8080

ENTRYPOINT ["/app/accreta-metrics"]
