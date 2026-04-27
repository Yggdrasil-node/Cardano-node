# syntax=docker/dockerfile:1.7

# -----------------------------------------------------------------------------
# Yggdrasil Cardano Node — multi-stage Dockerfile.
#
# Build:
#   docker build -t yggdrasil-node:latest .
#
# Run a relay (mainnet):
#   docker run --rm -it \
#     -v yggdrasil-db:/var/lib/yggdrasil/db \
#     -p 3001:3001 \
#     -p 127.0.0.1:12798:12798 \
#     yggdrasil-node:latest \
#     run --network mainnet \
#         --database-path /var/lib/yggdrasil/db \
#         --port 3001 --host-addr 0.0.0.0 \
#         --metrics-port 12798
#
# Run a block producer:
#   See docs/manual/docker.md for credential mounting and topology.
# -----------------------------------------------------------------------------

# ---------- Stage 1: build ----------
FROM rust:1.95-bookworm AS builder

ENV CARGO_TERM_COLOR=always \
    RUSTFLAGS="-C target-cpu=x86-64-v2" \
    DEBIAN_FRONTEND=noninteractive

RUN apt-get update -qq && \
    apt-get install -y -qq --no-install-recommends \
        pkg-config \
        ca-certificates \
        build-essential && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /src

# Copy the entire workspace. (Dependency-only layer caching could be added,
# but Yggdrasil's Cargo.lock changes infrequently and the build is already
# reasonably fast.)
COPY . .

RUN cargo build --release --bin yggdrasil-node --locked

# ---------- Stage 2: runtime ----------
FROM debian:bookworm-slim AS runtime

ENV DEBIAN_FRONTEND=noninteractive \
    YG_DB_PATH=/var/lib/yggdrasil/db \
    YG_CONFIG_PATH=/etc/yggdrasil

RUN apt-get update -qq && \
    apt-get install -y -qq --no-install-recommends \
        ca-certificates \
        tini \
        curl && \
    rm -rf /var/lib/apt/lists/* && \
    groupadd --system --gid 1000 yggdrasil && \
    useradd --system --uid 1000 --gid yggdrasil --home-dir /var/lib/yggdrasil --shell /usr/sbin/nologin yggdrasil && \
    mkdir -p "${YG_DB_PATH}" "${YG_CONFIG_PATH}" && \
    chown -R yggdrasil:yggdrasil /var/lib/yggdrasil "${YG_CONFIG_PATH}"

# Copy the binary and the vendored network presets.
COPY --from=builder /src/target/release/yggdrasil-node /usr/local/bin/yggdrasil-node
COPY --from=builder /src/node/configuration /usr/share/yggdrasil/configuration

# Operator scripts — useful for one-shot ops via `docker exec`.
COPY --from=builder /src/node/scripts/check_upstream_drift.sh /usr/local/bin/yggdrasil-check-upstream-drift
COPY --from=builder /src/node/scripts/restart_resilience.sh    /usr/local/bin/yggdrasil-restart-resilience
RUN chmod +x /usr/local/bin/yggdrasil-*

USER yggdrasil
WORKDIR /var/lib/yggdrasil

# Default ports: NtN inbound (3001), Prometheus metrics (12798).
EXPOSE 3001/tcp 12798/tcp

# Healthcheck — relies on metrics endpoint being enabled.
HEALTHCHECK --interval=30s --timeout=5s --start-period=120s --retries=3 \
    CMD curl -fsS "http://127.0.0.1:12798/health" >/dev/null || exit 1

# tini runs PID 1 so SIGTERM forwards correctly to yggdrasil-node and
# graceful shutdown semantics behave as documented.
ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/yggdrasil-node"]

# Override per deployment. Defaults to a dry `--help` so the container exits
# cleanly if started with no args.
CMD ["--help"]
