---
title: "Round 587 tx-generator DumpToFile PParamsUpdate voting thresholds"
parent: Reference
---

# Round 587 tx-generator DumpToFile PParamsUpdate voting thresholds

Date: 2026-05-20

## Scope

This round closes two more PParamsUpdate composite fields:
- `cppPoolVotingThresholds` (5-field record of UnitIntervals).
- `cppDRepVotingThresholds` (10-field record of UnitIntervals).

29/30 Conway PParamsUpdate fields now render. Only `cppCostModels`
remains.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/PParams.hs:303-309`
  (`PoolVotingThresholds` 5-field record).
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/PParams.hs:393-405`
  (`DRepVotingThresholds` 10-field record).

## Changes

- Added `show_pparam_pool_voting_thresholds` rendering the
  5-field stock-derived record. Each `UnitInterval` field is shown
  via the bare `<num> % <den>` form (stripping the always-wrapping
  parens from `show_unit_interval`).
- Added `show_pparam_drep_voting_thresholds` rendering the
  10-field stock-derived record with the same pattern.
- Updated `show_conway_pparams_update` to wire both fields and
  removed them from the rejection list.
- Added `dumptofile_show_conway_gov_action_parameter_change_with_voting_thresholds`
  unit test setting both records with realistic mainnet-shape
  thresholds (1/2 and 2/3) and asserting the full rendered shape.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-tx-generator dumptofile` (54 tests, +1
  from R586)
- `cargo test -p yggdrasil-tx-generator` (236 lib tests + 5
  CLI/golden, +1 from R586 baseline)

## Remaining

- `cppCostModels` (final composite field — per-language cost-model
  arrays).
- Close upstream `bootstrapWitKeyHash` byte-parity for
  multi-witness sets.
- Full Haskell `Show (ByteString)` mnemonic-escape coverage.
- Capture upstream-binary comparison evidence once a runnable
  upstream binary environment is available.
