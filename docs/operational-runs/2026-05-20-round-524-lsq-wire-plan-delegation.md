# Round 524 - LSQ wire plan delegation

Date: 2026-05-20

## Goal

Continue stale-placement cleanup by removing duplicated migrated
LocalStateQuery wire-plan ownership from the node binary command bridge.

## Changes

- Moved the migrated `NtcQuery` CBOR query plans and reply decoders into
  `yggdrasil_cardano_cli::lsq`.
- Added shared `encode_query` and `decode_query_result` APIs for migrated LSQ
  variants.
- Updated `lsq_tokio.rs` to use the shared `plan_for` implementation instead
  of keeping a private duplicate.
- Updated `crates/node/cardano-node/src/commands/query.rs` so shared variants
  map to `NtcQuery` and delegate encoding/decoding to `yggdrasil-cardano-cli`.
  The node bridge now keeps only `EraHistory`, `CurrentEpoch`, UTxO/reward,
  delegation/reward, and stake-pool-params query tags local.
- Updated local `AGENTS.md` guidance so future migrated LSQ wire-plan changes
  stay in `crates/tools/cardano-cli`.

## Verification

- `cargo check -p yggdrasil-cardano-cli`
- `cargo check -p yggdrasil-node`
- `cargo test -p yggdrasil-cardano-cli lsq`
- `cargo test -p yggdrasil-node query`
- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test-all`
- `python scripts/check-stale-placement.py`
- `python scripts/check-stale-placement.py --self-test`
- `python scripts/check-strict-mirror.py --fail-on-violation`
- `python scripts/check-parity-matrix.py`
- `python scripts/check-fixture-manifest.py`
- `python .claude/scripts/filetree.py check`
- `git diff --check`

All listed checks passed. `git diff --check` emitted only CRLF-normalization
warnings from the current Windows checkout.
