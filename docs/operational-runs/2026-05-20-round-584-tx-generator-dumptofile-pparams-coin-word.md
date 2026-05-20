---
title: "Round 584 tx-generator DumpToFile PParamsUpdate Coin + Word fields"
parent: Reference
---

# Round 584 tx-generator DumpToFile PParamsUpdate Coin + Word fields

Date: 2026-05-20

## Scope

This round extends R583's empty-PParamsUpdate path to render the 16
scalar PParamsUpdate fields:

- 8 Coin-family fields (CompactForm Coin / CoinPerByte): render as
  `SJust (CompactCoin {unCompactCoin = <n>})`.
- 8 plain Word fields (Word16 / Word32 / Word64): render as `SJust
  <n>`.

The remaining 14 fields (interval-family + composite records) stay
on field-name-bearing `TxGenError`.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/Coin.hs:146-209`
  (`CompactForm Coin = CompactCoin {unCompactCoin :: Word64}`,
  `CoinPerByte` newtype-Show delegating to CompactForm Coin).

## Changes

- Added `show_pparam_compact_coin` generic helper rendering
  `Option<N: Into<u64>>` as `SJust (CompactCoin {unCompactCoin =
  <n>})` (matching the inner CompactCoin record wrapped in parens at
  showsPrec 11 inside `SJust`).
- Added `show_pparam_word` generic helper rendering
  `Option<N: Display>` as `SJust <n>` (primitive numeric Show at
  showsPrec 0 without constructor wrapping).
- Updated `show_conway_pparams_update` to use the helpers for 16
  fields:
  - Coin-family: `cppTxFeePerByte`, `cppTxFeeFixed`,
    `cppKeyDeposit`, `cppPoolDeposit`, `cppMinPoolCost`,
    `cppCoinsPerUTxOByte`, `cppGovActionDeposit`, `cppDRepDeposit`.
  - Word: `cppMaxBBSize`, `cppMaxTxSize`, `cppMaxBHSize`,
    `cppNOpt`, `cppMaxValSize`, `cppCollateralPercentage`,
    `cppMaxCollateralInputs`, `cppCommitteeMinSize`.
- Removed these 16 fields from the field-name rejection list.
- Added `dumptofile_show_conway_gov_action_parameter_change_with_coin_fields`
  unit test setting all 16 supported fields and verifying their
  rendered shape.
- Moved the rejection regression test from `cppKeyDeposit` (now
  renders) to `cppA0` (still on `NonNegativeInterval` boundary).

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-tx-generator dumptofile` (50 tests, +1
  from R583)
- `cargo test -p yggdrasil-tx-generator` (232 lib tests + 5
  CLI/golden, +1 from R583 baseline)

## Remaining (14 fields)

- `EpochInterval` (4 fields: `cppEMax`,
  `cppCommitteeMaxTermLength`, `cppGovActionLifetime`,
  `cppDRepActivity`).
- `NonNegativeInterval` (2 fields: `cppA0`,
  `cppMinFeeRefScriptCostPerByte`).
- `UnitInterval` (2 fields: `cppRho`, `cppTau`).
- `Prices` (1 field: `cppPrices` — combines `prMem` + `prSteps`).
- `OrdExUnits` (2 fields: `cppMaxTxExUnits`, `cppMaxBlockExUnits`).
- `PoolVotingThresholds` + `DRepVotingThresholds` (2 records).
- `CostModels` (1 record with per-language cost-model arrays).
- 4 Shelley-era-only fields (`d`, `extra_entropy`, `min_utxo_value`,
  `protocol_version`) cannot render against Conway PParamsUpdate at
  all.

## Remaining (other)

- Close upstream `bootstrapWitKeyHash` byte-parity for
  multi-witness sets.
- Full Haskell `Show (ByteString)` mnemonic-escape coverage.
- Capture upstream-binary comparison evidence once a runnable
  upstream binary environment is available.
