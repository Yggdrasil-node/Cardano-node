---
title: "Round 586 tx-generator DumpToFile PParamsUpdate Prices + OrdExUnits"
parent: Reference
---

# Round 586 tx-generator DumpToFile PParamsUpdate Prices + OrdExUnits

Date: 2026-05-20

## Scope

This round extends R585's interval coverage to render 3 composite
PParamsUpdate fields:

- `cppPrices` (combines `prMem` + `prSteps` NonNegativeIntervals into
  a `Prices` record).
- `cppMaxTxExUnits` (newtype `OrdExUnits` over `ExUnits`).
- `cppMaxBlockExUnits` (same shape).

27/30 Conway PParamsUpdate fields now render. The remaining 3
record fields (`CostModels`, `PoolVotingThresholds`,
`DRepVotingThresholds`) stay on field-name-bearing `TxGenError`.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/Plutus/ExUnits.hs:159-163`
  (`Prices` record stock-derived Show).
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/PParams.hs:413-415`
  (`OrdExUnits` newtype-Show delegating to ExUnits Show).

## Changes

- Added `show_pparam_prices` helper. Yggdrasil's
  `ProtocolParameterUpdate` stores `price_mem` and `price_step` as
  separate Option<UnitInterval> fields; the helper combines them into
  the upstream `SJust (Prices {prMem = <num> % <den>, prSteps = <num>
  % <den>})` form when both are Some, or `SNothing` when both are
  None. The mixed-Some/None case is caught earlier in the field
  rejection list with a clear pairing message.
- Added `show_pparam_ex_units` helper rendering `Option<&ExUnits>`
  as `SNothing` or `SJust (ExUnits {exUnitsMem = M, exUnitsSteps =
  S})`. Reuses R572's `show_alonzo_ex_units`.
- Added `strip_outer_parens` utility that strips one layer of outer
  parens — used in `show_pparam_prices` because `show_unit_interval`
  always wraps with parens for safety in constructor-argument
  position, but record fields at p=0 need the bare ratio form.
- Updated `show_conway_pparams_update` to wire the 3 fields and
  removed them from the field rejection list.
- Added 2 focused unit tests:
  - `dumptofile_show_conway_gov_action_parameter_change_with_prices_and_exunits`
    sets all 3 fields with realistic mainnet values and verifies the
    exact rendered shapes.
  - `dumptofile_show_conway_pparam_prices_rejects_unpaired` confirms
    the pairing enforcement reports the `price_mem and price_step
    must be set together` message.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-tx-generator dumptofile` (53 tests, +2
  from R585)
- `cargo test -p yggdrasil-tx-generator` (235 lib tests + 5
  CLI/golden, +2 from R585 baseline)

## Remaining (3 composite fields)

- `CostModels` record with per-language cost-model arrays.
- `PoolVotingThresholds` + `DRepVotingThresholds` records (UnitIntervals).

## Remaining (other)

- Close upstream `bootstrapWitKeyHash` byte-parity for
  multi-witness sets.
- Full Haskell `Show (ByteString)` mnemonic-escape coverage for
  byte parity.
- Capture upstream-binary comparison evidence once a runnable
  upstream binary environment is available.
