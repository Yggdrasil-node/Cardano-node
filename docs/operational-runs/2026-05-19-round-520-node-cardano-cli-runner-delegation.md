# Round 520 - node cardano-cli runner delegation

Date: 2026-05-19

## Goal

Continue the stale-placement cleanup inside
`crates/node/cardano-node/src/commands/cardano_cli.rs`. After R519 moved helper
ownership to `crates/tools/cardano-cli`, this round moves complete offline
subcommand dispatch to the tool crate as well.

## Changes

- Added a small node-side `LsqClient` bridge that adapts
  `yggdrasil-cardano-cli` LocalTxSubmission calls to the node binary runtime.
- Replaced the node-local implementations for these command arms with
  `crates/tools/cardano-cli` runners:
  - `transaction-submit`
  - `transaction-txid`
  - `transaction-sign`
  - `address-key-gen`
  - `address-key-hash`
  - `address-build`
  - `stake-address-key-gen`
  - `stake-address-build`
- Routed the query variants that already exist in the tool crate through the
  tool crate's `NtcQuery` enum and the node-side `LsqClient` bridge.
- Left Yggdrasil-only query wrappers (`query-utxo`, `query-current-epoch`,
  `query-era-history`, `query-reward-balance`,
  `query-delegations-and-rewards`, `query-stake-pool-params`) in the node
  dispatcher until their tool-crate command variants exist.

## Verification

- `cargo check -p yggdrasil-node`
- `cargo check -p yggdrasil-cardano-cli`
- `cargo test -p yggdrasil-node cardano_cli_`
- `cargo test -p yggdrasil-cardano-cli`
- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test-all`
- `python scripts/check-stale-placement.py --self-test`
- `python scripts/check-stale-placement.py`
- `python scripts/check-strict-mirror.py --fail-on-violation`
- `python scripts/check-parity-matrix.py`
- `python scripts/check-fixture-manifest.py`
- `python .claude/scripts/filetree.py check`
- `git diff --check`

All listed checks passed in this round.
