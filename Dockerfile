# ── Candor AI — Multi-Stage Docker Image ──────────────────────────────────
# Builds a minimal container for `candor serve` mode.
#
# Usage:
#   docker build -t candor-ai .
#   docker run -p 31337:31337 \
#     -e ANTHROPIC_API_KEY=... \
#     -e OPENAI_API_KEY=... \
#     candor-ai candor serve --port 31337
#
# For voice/PDA features, mount ~/.candor/ and required audio devices.

# ── Stage 1: Build ────────────────────────────────────────────────────────
FROM rust:1.86-slim-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates crates/
COPY bin bin/

RUN cargo build --release --bin candor && \
    cp target/release/candor /candor && \
    strip /candor

# ── Stage 2: Runtime ──────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /candor /usr/local/bin/candor

EXPOSE 31337

ENV CANDOR_HOME=/home/candor/.candor

USER 1000

ENTRYPOINT ["candor"]
CMD ["serve", "--port", "31337"]
