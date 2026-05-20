# Wave 9 PR 25 — top-level developer recipes.
#
# This file layers ON TOP of the existing cargo aliases in
# `.cargo/config.toml` (`check-all`, `lint`, `test-all`,
# `lint-no-default`, `xtask`). Cargo aliases stay verbatim; the
# justfile orchestrates *workflows* (multi-step rehearsals, parity
# validators, scaffolding) that cargo aliases cannot express.
#
# Defaults:
#   - `just`             → `default` recipe (check + lint + test-fast).
#   - `just ci-local`    → mirrors the order of .github/workflows/ci.yml
#                          so a green local run strongly predicts green CI.

set shell := ["bash", "-uc"]
set dotenv-load := false
export CARGO_TERM_COLOR := "always"

# ─── Default ──────────────────────────────────────────────────────────
default: check lint test-fast

# ─── Cargo orchestration (workspace aliases stay the source of truth) ─
check:
    cargo check-all

lint:
    cargo lint

lint-no-default:
    cargo lint-no-default

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

test:
    cargo test-all

test-fast:
    cargo nextest run --profile default --workspace

test-slow:
    cargo nextest run --profile slow --workspace --run-ignored only

bench:
    cargo criterion --workspace

audit:
    cargo deny check advisories bans licenses sources

# ─── Parity invariants (Python validators) ────────────────────────────
parity-check:
    python3 scripts/check-parity-matrix.py

parity-fixtures:
    python3 scripts/check-fixture-manifest.py

parity-mirror:
    python3 scripts/check-strict-mirror.py --fail-on-violation

placement-check:
    python3 scripts/check-stale-placement.py --self-test
    python3 scripts/check-stale-placement.py

parity-all: parity-check parity-fixtures parity-mirror placement-check

# Scaffold a parity-matrix.json entry for a newly-added Rust file.
# Wave 9 PR 27 lands the xtask subcommand; this recipe wraps it.
parity-add CRATE FILE:
    cargo xtask parity-add --crate {{CRATE}} --file {{FILE}}

# ─── Upstream Haskell reference tree ──────────────────────────────────
# Sources-only is what CI uses (~14s); --force rebuilds the full
# ~1.3GB install tree locally for forensic byte-equivalence work.
upstream-fetch:
    bash scripts/setup-reference.sh --sources-only

upstream-full:
    bash scripts/setup-reference.sh --force

# ─── Operational rehearsals (long-running) ────────────────────────────
preview-producer:
    bash scripts/preview_producer_harness.sh

mainnet-relay-rehearsal:
    bash scripts/parallel_blockfetch_soak.sh

# ─── Sister-tool recipes (Wave 8 PR 24) ───────────────────────────────
#
# Each sister tool gets a `<tool>-build`, `<tool>-test`,
# `<tool>-help`, and (where a comparison harness exists) `<tool>-parity`
# recipe. The recipes call `cargo` / `bash` against fully-qualified
# crate names so they remain stable even after Wave 5 sub-crate
# reorganization. Comparison harnesses live under
# `scripts/` after Wave 4 PR 6.

# cardano-cli — the Yggdrasil-side cardano-cli port
cardano-cli-build:
    cargo build --release -p yggdrasil-cardano-cli

cardano-cli-test:
    cargo nextest run --profile default -p yggdrasil-cardano-cli

cardano-cli-help:
    cargo run --release -p yggdrasil-cardano-cli -- --help

# bech32 — BIP-0173 encoder + CLI mirror
bech32-build:
    cargo build --release -p yggdrasil-bech32

bech32-test:
    cargo nextest run --profile default -p yggdrasil-bech32

bech32-help:
    cargo run --release -p yggdrasil-bech32 -- --help

# cardano-submit-api — HTTP tx-submission server
cardano-submit-api-build:
    cargo build --release -p yggdrasil-cardano-submit-api

cardano-submit-api-test:
    cargo nextest run --profile default -p yggdrasil-cardano-submit-api

cardano-submit-api-parity:
    bash scripts/compare_submit_api_to_upstream.sh

# cardano-tracer — trace-forwarder aggregator
cardano-tracer-build:
    cargo build --release -p yggdrasil-cardano-tracer

cardano-tracer-test:
    cargo nextest run --profile default -p yggdrasil-cardano-tracer

cardano-tracer-help:
    cargo run --release -p yggdrasil-cardano-tracer -- --help

# kes-agent / kes-agent-control — KES secret custody
kes-agent-build:
    cargo build --release -p yggdrasil-kes-agent -p yggdrasil-kes-agent-control

kes-agent-test:
    cargo nextest run --profile default -p yggdrasil-kes-agent -p yggdrasil-kes-agent-control

# db-truncater — ChainDB rollback tool
db-truncater-build:
    cargo build --release -p yggdrasil-db-truncater

db-truncater-test:
    cargo nextest run --profile default -p yggdrasil-db-truncater

db-truncater-parity:
    bash scripts/compare_db_truncater_to_upstream.sh

# db-analyser — ChainDB forensic analyser
db-analyser-build:
    cargo build --release -p yggdrasil-db-analyser

db-analyser-test:
    cargo nextest run --profile default -p yggdrasil-db-analyser

# db-synthesizer — synthetic chain generator
db-synthesizer-build:
    cargo build --release -p yggdrasil-db-synthesizer

db-synthesizer-test:
    cargo nextest run --profile default -p yggdrasil-db-synthesizer

# snapshot-converter — ledger snapshot format converter
snapshot-converter-build:
    cargo build --release -p yggdrasil-snapshot-converter

snapshot-converter-test:
    cargo nextest run --profile default -p yggdrasil-snapshot-converter

# cardano-testnet — multi-node testnet harness
cardano-testnet-build:
    cargo build --release -p yggdrasil-cardano-testnet

cardano-testnet-test:
    cargo nextest run --profile default -p yggdrasil-cardano-testnet

# tx-generator — transaction-stream load generator
tx-generator-build:
    cargo build --release -p yggdrasil-tx-generator

tx-generator-test:
    cargo nextest run --profile default -p yggdrasil-tx-generator

# dmq-node — Mithril-related Delegated Mempool Queue node
dmq-node-build:
    cargo build --release -p yggdrasil-dmq-node

dmq-node-test:
    cargo nextest run --profile default -p yggdrasil-dmq-node

# Tip-compare against a running upstream cardano-node — runs from the
# yggdrasil-node binary against the operator-configured Unix socket.
tip-compare:
    bash scripts/compare_tip_to_haskell.sh

# Build every sister tool in one shot. Useful for verifying the
# `crates/tools/` tree hasn't drifted out of sync with the workspace.
tools-build-all:
    cargo build --release \
      -p yggdrasil-cardano-cli \
      -p yggdrasil-bech32 \
      -p yggdrasil-cardano-submit-api \
      -p yggdrasil-cardano-tracer \
      -p yggdrasil-kes-agent \
      -p yggdrasil-kes-agent-control \
      -p yggdrasil-db-truncater \
      -p yggdrasil-db-analyser \
      -p yggdrasil-db-synthesizer \
      -p yggdrasil-snapshot-converter \
      -p yggdrasil-cardano-testnet \
      -p yggdrasil-tx-generator \
      -p yggdrasil-dmq-node

# ─── Pre-push convenience: locally equivalent to ci.yml ───────────────
ci-local: fmt-check check lint test-fast parity-all audit
