# Round 525 - submit-tx hex parser delegation

Date: 2026-05-20

## Goal

Continue the post-reorganization cleanup by removing a small node-local
transaction parsing fork while preserving the `yggdrasil-node submit-tx`
compatibility surface.

## Changes

- Moved the lenient transaction `--tx-hex` parser into
  `yggdrasil_cardano_cli::era_based::transaction::run::decode_tx_hex_arg`.
- Updated `read_tx_input` to reuse the shared parser for every
  cardano-cli transaction subcommand that accepts `--tx-hex`.
- Changed `crates/node/cardano-node/src/commands/submit_tx.rs` to re-export
  the cardano-cli helper instead of carrying a duplicate parser.
- Updated node and cardano-cli AGENTS guidance so transaction input parsing
  remains owned by `crates/tools/cardano-cli`.

## Verification

- `cargo fmt --all -- --check` passed.
- `cargo test -p yggdrasil-cardano-cli decode_tx_hex_arg` passed.
- `cargo test -p yggdrasil-node decode_tx_hex_arg` passed.
- `cargo check-all` passed.
- `cargo lint` passed.
- `cargo test-all` passed.
- `python dev/test/check-stale-placement.py` passed.
- `python dev/test/check-stale-placement.py --self-test` passed.
- `python dev/test/check-strict-mirror.py --fail-on-violation` passed.
- `python dev/test/check-parity-matrix.py` passed.
- `python dev/test/check-fixture-manifest.py` passed.
- `python dev/test/filetree.py accept-current` refreshed the manifest
  for this round's changed files.
- `python dev/test/filetree.py check` passed after manifest refresh.
- `git diff --check` passed with only existing Windows CRLF warnings.
