# Round 528 - era-history LSQ plan delegation

Date: 2026-05-20

## Goal

Continue stale-placement cleanup by moving the fixed `query-era-history`
LocalStateQuery wire plan out of the node binary and into the shared
cardano-cli LSQ owner.

## Changes

- Added `NtcQuery::EraHistory` to `yggdrasil_cardano_cli::lsq`.
- Moved the upstream hard-fork `GetInterpreter` query shape
  `[0, [2, [0]]]` and raw `era_history_cbor` decoder out of
  `crates/node/cardano-node/src/commands/query.rs`.
- Updated the node query bridge so `QueryCommand::EraHistory` delegates
  through `lsq::encode_query` / `lsq::decode_query_result`.
- Updated the node `cardano-cli query-era-history` wrapper to use
  `TokioLsqClient.run_query(..., NtcQuery::EraHistory)` directly.
- Exposed `query-era-history` and `query-current-epoch` through the
  standalone `yggdrasil-cardano-cli` command enum and run dispatcher so
  the shared LSQ plan is reachable without the node wrapper.
- Extended `scripts/check-stale-placement.py` so the era-history query
  must remain mapped to the shared LSQ plan.
- Updated AGENTS and parity docs to record the 22-query shared LSQ
  surface.

## Verification

- `cargo fmt --all -- --check`
- `cargo test -p yggdrasil-cardano-cli era_history`
- `cargo test -p yggdrasil-cardano-cli current_epoch`
- `cargo test -p yggdrasil-cardano-cli query_commands_dispatch_through_custom_lsq_client`
- `cargo test -p yggdrasil-node encode_ntc_query_emits_expected_tag_bytes`
- `cargo test -p yggdrasil-node decode_ntc_result_shapes_typed_json_for_new_queries`
- `cargo check-all`
- `cargo lint`
- `cargo test-all`
- `python scripts/check-stale-placement.py`
- `python scripts/check-stale-placement.py --self-test`
- `python scripts/check-strict-mirror.py --fail-on-violation`
- `python scripts/check-parity-matrix.py`
- `python scripts/check-fixture-manifest.py`
- `python .claude/scripts/filetree.py check`
- `git diff --check` (exit 0; Windows CRLF warnings only)
