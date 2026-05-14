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

parity-all: parity-check parity-fixtures parity-mirror

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
    bash crates/node/yggdrasil-node/scripts/preview_producer_harness.sh

mainnet-relay-rehearsal:
    bash crates/node/yggdrasil-node/scripts/parallel_blockfetch_soak.sh

# ─── Pre-push convenience: locally equivalent to ci.yml ───────────────
ci-local: fmt-check check lint test-fast parity-all audit
