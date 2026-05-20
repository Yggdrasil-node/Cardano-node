# Round 519 - cardano-cli helper delegation

Date: 2026-05-19

## Goal

Continue the stale-placement cleanup after the node crate rename by removing
reusable `cardano-cli` helper implementations from the node executable shell.
The node crate must remain a thin dispatcher while preserving the
`yggdrasil-node cardano-cli ...` compatibility surface.

## Changes

- Promoted canonical helper APIs in `crates/tools/cardano-cli`:
  - payment address Bech32 construction
  - reward address Bech32 construction
  - TextEnvelope writing
  - transaction single-signer witness replacement
  - transaction id extraction from CBOR
- Replaced duplicated helper bodies in
  `crates/node/cardano-node/src/commands/cardano_cli.rs` with wrappers that
  delegate to `yggdrasil-cardano-cli`.
- Kept node-local query and transaction-submission bridges in the binary crate,
  because they still adapt the node runtime and its extra LocalStateQuery
  variants.
- Removed the now-unused direct `ciborium` dependency from the node binary
  manifest.
- Updated `AGENTS.md` guidance so future work keeps reusable `cardano-cli`
  behavior under `crates/tools/cardano-cli`.

## Verification

- `cargo check -p yggdrasil-cardano-cli`
- `cargo check -p yggdrasil-node`
- `cargo test -p yggdrasil-cardano-cli`
- `cargo test -p yggdrasil-node cardano_cli_`
- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test-all`
- `python scripts/check-stale-placement.py --self-test`
- `python scripts/check-stale-placement.py`
- `python scripts/check-strict-mirror.py --fail-on-violation`
- `python scripts/check-parity-matrix.py`
- `python scripts/check-fixture-manifest.py`

All listed checks passed in this round.
