# Round 523 - query helper delegation

Date: 2026-05-20

## Goal

Continue stale-placement cleanup inside `crates/node/cardano-node` by moving
pure LocalStateQuery helper behavior into the `cardano-cli` owner crate while
leaving the node binary responsible only for its NtC command bridge.

## Changes

- Added shared `format_utc_time` and `decode_optional_prefixed_hex` helpers to
  `yggdrasil_cardano_cli::lsq`.
- Updated `lsq_tokio.rs` to reuse the shared SystemStart formatter instead of
  keeping a private duplicate.
- Updated `crates/node/cardano-node/src/commands/query.rs` to re-export and use
  the shared helpers for node-local query encoders and SystemStart rendering.
- Updated local `AGENTS.md` guidance so future query-helper changes stay in
  `crates/tools/cardano-cli` rather than drifting back into the node binary.

## Verification

- `cargo check -p yggdrasil-cardano-cli`
- `cargo check -p yggdrasil-node`
- `cargo test -p yggdrasil-cardano-cli format_utc_time`
- `cargo test -p yggdrasil-cardano-cli decode_optional_prefixed_hex`
- `cargo test -p yggdrasil-node format_utc_time`
- `cargo test -p yggdrasil-node decode_optional_prefixed_hex`
- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test-all`
- `python scripts/check-stale-placement.py`
- `python scripts/check-strict-mirror.py --fail-on-violation`
- `python scripts/check-parity-matrix.py`
- `python scripts/check-fixture-manifest.py`
- `python .claude/scripts/filetree.py check`
- `git diff --check`

All listed checks passed. `git diff --check` emitted only CRLF-normalization
warnings from the current Windows checkout.
