# Build context is the repository root (see docker-compose.yml), so that the
# root .env file can be copied into the image.

# ---- Build stage ---------------------------------------------------------
FROM rust:1.100-bookworm AS builder
WORKDIR /build

COPY exporter/Cargo.toml exporter/Cargo.lock ./
COPY exporter/src ./src

# Cache mounts keep the crate registry and compiled dependencies between builds.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release --locked \
    && cp target/release/tapo-climate-exporter /usr/local/bin/tapo-climate-exporter

# ---- Runtime stage -------------------------------------------------------
FROM debian:bookworm-slim

RUN useradd --system --create-home --home-dir /app --shell /usr/sbin/nologin exporter
WORKDIR /app

COPY --from=builder /usr/local/bin/tapo-climate-exporter /usr/local/bin/tapo-climate-exporter

# HOME SERVER ONLY - DO NOT DO THIS IN PRODUCTION!!!
# This bakes the .env file, including the Tapo account password, into an image
# layer. Anyone who can pull or inspect the image can read it. That is fine for
# an image that is built and run on a private home server and never pushed to a
# registry. For anything else, drop this line and inject the variables at
# runtime (compose `env_file`, Docker secrets, ...). The exporter reads this
# file at startup; variables set in the container environment take precedence.
COPY --chown=exporter:exporter .env /app/.env

USER exporter
EXPOSE 9101

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD ["tapo-climate-exporter", "healthcheck"]

ENTRYPOINT ["tapo-climate-exporter"]
