# R529 Parameterized LSQ Plan Delegation

Date: 2026-05-20

## Goal

Move the remaining parameterized LocalStateQuery CBOR plans out of the
node-binary wrapper and into the shared `yggdrasil-cardano-cli` owner while
preserving the existing wire envelopes and JSON result shapes.

## Scope

- `crates/tools/cardano-cli/src/lsq.rs`
- `crates/tools/cardano-cli/src/lsq_tokio.rs`
- `crates/tools/cardano-cli/src/command.rs`
- `crates/tools/cardano-cli/src/run.rs`
- `crates/tools/cardano-cli/src/parser.rs`
- `crates/node/cardano-node/src/commands/query.rs`
- `crates/node/cardano-node/src/commands/cardano_cli.rs`
- `scripts/check-stale-placement.py`
- `crates/tools/cardano-cli/AGENTS.md`
- `docs/COMPLETION_ROADMAP.md`
- `docs/parity-matrix.json`

## Result

`NtcQuery` now owns the payload-carrying query variants for:

- `query-utxo --address`
- `query-utxo --tx-in`
- `query-reward-balance --account`
- `query-delegations-and-rewards --credential`
- `query-stake-pool-params --pool-hash`

The node wrapper maps its compatibility `QueryCommand` variants directly to
those shared query plans, and the standalone `yggdrasil-cardano-cli` parser and
dispatcher expose the same parameterized query surface.

## Verification

- `cargo fmt --all -- --check`
- `cargo test -p yggdrasil-cardano-cli parameterized_query`
- `cargo test -p yggdrasil-cardano-cli query_commands_dispatch_through_custom_lsq_client`
- `cargo test -p yggdrasil-node encode_ntc_query_accepts_0x_prefixed_arguments_end_to_end`
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
- `git diff --check` (clean; Windows line-ending warnings only)
