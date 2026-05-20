---
title: "Round 585 tx-generator DumpToFile PParamsUpdate interval fields"
parent: Reference
---

# Round 585 tx-generator DumpToFile PParamsUpdate interval fields

Date: 2026-05-20

## Scope

This round extends R584's scalar coverage to render 8 more
PParamsUpdate fields:

- 4 EpochInterval fields render as `SJust (EpochInterval <n>)`.
- 4 ratio interval fields (UnitInterval + NonNegativeInterval)
  render as `SJust (<num> % <den>)`.

24/30 Conway PParamsUpdate fields now render. The remaining 6
composite fields (`Prices`, `OrdExUnits` x2, `CostModels`,
`PoolVotingThresholds`, `DRepVotingThresholds`) stay on
field-name-bearing `TxGenError`.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-base/cardano-slotting/src/Cardano/Slotting/Slot.hs:128-133`
  (`EpochInterval` Show via Quiet).
- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/BaseTypes.hs:296-298,470-482,545-558`
  (`BoundedRatio`, `NonNegativeInterval`, `UnitInterval` newtype-Show
  chain).

## Changes

- Added `show_pparam_epoch_interval` generic helper rendering
  `Option<N: Into<u64>>` as `SJust (EpochInterval <n>)`. Quiet
  derives Show by suppressing record syntax but keeping the
  `EpochInterval` constructor name; SJust wraps single-arg
  constructor in parens at showsPrec 11.
- Added `show_pparam_ratio_interval` helper rendering
  `Option<UnitInterval>` as `SJust (<num> % <den>)`. Both
  `UnitInterval` and `NonNegativeInterval` use `deriving newtype
  Show` through `BoundedRatio Word64`, delegating to `Show (Ratio
  Word64)` which emits `<num> % <den>` (parens added by ratioPrec
  > 7 inside SJust).
- Updated `show_conway_pparams_update` to use the helpers for 8
  fields:
  - EpochInterval: `cppEMax`, `cppCommitteeMaxTermLength`,
    `cppGovActionLifetime`, `cppDRepActivity`.
  - Ratio interval: `cppA0`, `cppRho`, `cppTau`,
    `cppMinFeeRefScriptCostPerByte`.
- Removed these 8 fields from the field-name rejection list.
- Added `dumptofile_show_conway_gov_action_parameter_change_with_interval_fields`
  unit test setting all 8 supported interval fields.
- Moved the rejection regression test from `cppA0` (now renders)
  to `cppCostModels` (still on `CostModels` boundary).

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-tx-generator dumptofile` (51 tests, +1
  from R584)
- `cargo test -p yggdrasil-tx-generator` (233 lib tests + 5
  CLI/golden, +1 from R584 baseline)

## Remaining (6 composite fields + Shelley-only)

- `Prices` record (1 field: `cppPrices` — combines `prMem` +
  `prSteps` NonNegativeIntervals).
- `OrdExUnits` (2 fields: `cppMaxTxExUnits`, `cppMaxBlockExUnits`).
- `CostModels` (1 field).
- `PoolVotingThresholds` + `DRepVotingThresholds` (2 records of
  UnitIntervals).
- 4 Shelley-era-only yggdrasil fields (`d`, `extra_entropy`,
  `min_utxo_value`, `protocol_version`) cannot render against
  Conway PParamsUpdate at all.

## Remaining (other)

- Close upstream `bootstrapWitKeyHash` byte-parity for
  multi-witness sets.
- Full Haskell `Show (ByteString)` mnemonic-escape coverage.
- Capture upstream-binary comparison evidence once a runnable
  upstream binary environment is available.
