# Round 527 - current-epoch LSQ plan delegation

Date: 2026-05-20

## Goal

Continue stale-placement cleanup by moving another node-local
LocalStateQuery wire plan into the shared cardano-cli LSQ owner.

## Changes

- Added `NtcQuery::CurrentEpoch` to `yggdrasil_cardano_cli::lsq`.
- Moved the Yggdrasil-extension `[101]` query tag and `{ "epoch": ... }`
  decoder out of `crates/node/cardano-node/src/commands/query.rs`.
- Updated the node query bridge so `QueryCommand::CurrentEpoch` delegates
  through `lsq::encode_query` / `lsq::decode_query_result`.
- Updated the node `cardano-cli query-current-epoch` wrapper to use
  `TokioLsqClient.run_query(..., NtcQuery::CurrentEpoch)` directly.
- Extended `dev/test/check-stale-placement.py` so the current-epoch query
  must remain mapped to the shared LSQ plan.
- Updated AGENTS guidance to keep simple LSQ tags in `crates/tools/cardano-cli`.

## Verification

- `cargo fmt --all -- --check`
- `cargo test -p yggdrasil-cardano-cli current_epoch`
- `cargo test -p yggdrasil-node encode_ntc_query_emits_expected_tag_bytes`
- `cargo test -p yggdrasil-node decode_ntc_result_shapes_typed_json_for_new_queries`
- `cargo check-all`
- `cargo lint`
- `cargo test-all`
- `python dev/test/check-stale-placement.py`
- `python dev/test/check-stale-placement.py --self-test`
- `python dev/test/check-strict-mirror.py --fail-on-violation`
- `python dev/test/check-parity-matrix.py`
- `python dev/test/check-fixture-manifest.py`
- `python dev/test/filetree.py check`
- `git diff --check` (exit 0; Windows CRLF warnings only)
