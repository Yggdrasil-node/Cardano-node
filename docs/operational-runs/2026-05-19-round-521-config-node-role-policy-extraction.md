# Round 521 - config node-role policy extraction

Date: 2026-05-19

## Goal

Continue stale-placement cleanup inside the active `crates/node/cardano-node`
binary crate by moving reusable config interpretation out of the command
adapter layer.

## Changes

- Moved Shelley block-producer credential field inspection, credential policy
  enforcement, and node-role classification into `crates/node/config`.
- Kept the binary crate responsible only for CLI adaptation, JSON/trace
  reporting, and feature-gated credential file loading.
- Updated node/config and node-binary `AGENTS.md` guidance so future changes
  keep node-role and credential-field policy in `yggdrasil-node-config`.
- Added config-crate regression tests for absent/partial/complete credential
  status and partial-credential rejection for producing nodes.

## Verification

- `cargo check -p yggdrasil-node-config`
- `cargo check -p yggdrasil-node`
- `cargo test -p yggdrasil-node-config credential`
- `cargo test -p yggdrasil-node validate_config_report_rejects_partial_block_producer_credentials`
- `cargo test -p yggdrasil-node node_role_report`
- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test-all`
- `python dev/test/check-stale-placement.py`
- `python dev/test/check-strict-mirror.py --fail-on-violation`
- `python dev/test/check-parity-matrix.py`
- `python dev/test/check-fixture-manifest.py`
- `python dev/test/filetree.py check`
- `git diff --check`

All listed checks passed. `git diff --check` emitted only CRLF-normalization
warnings from the current Windows checkout.
