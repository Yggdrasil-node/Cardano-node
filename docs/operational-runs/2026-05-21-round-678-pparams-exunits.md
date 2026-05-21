---
title: "Round 678 Typed PParamsUpdate ExUnits parameters (A5 Phase-2.5)"
parent: Reference
---

# Round 678 Typed PParamsUpdate ExUnits parameters (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Types the execution-unit protocol parameters within a
`PParamsUpdate` — `maxTxExUnits` / `maxBlockExUnits` (ids 20/21)
and `prices` (id 19).

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/PParams.hs`
  (`maxTxExUnits` / `maxBlockExUnits` are `ExUnits` 2-element
  arrays `[mem, steps]`; `prices` is `Prices { prMem ::
  NonNegativeInterval, prSteps :: NonNegativeInterval }` — a
  2-element array of tag-30 rationals).

## Changes

- Added `PParamValue::ExUnits(ExUnits)` and
  `PParamValue::ExUnitPrices { mem: UnitInterval, step:
  UnitInterval }`.
- `PParamsUpdate::from_decoder` dispatches on the parameter id:
  ids 20/21 decode via `ExUnits::from_decoder`; id 19 decodes
  the 2-element `[memPrice, stepPrice]` rational pair.
- Display: ExUnits renders the upstream `WrapExUnits {...}`
  shape; prices render as `Prices {prMem = <r>, prSteps = <r>}`.

1 new focused unit test:
- `pparams_update_decodes_exunits_and_prices` — a
  `ParameterChange` setting `maxTxExUnits` and `prices`,
  asserting the typed `ExUnits` budget and the rational price
  pair.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (345 lib + 4
  doctests + 1 main, +1 new test vs R677 baseline of 344)

## Remaining (A5 Phase-2.5+)

- `PParamsUpdate` cost-model and pool/DRep voting-threshold
  parameters.
- `CollectError::BadTranslation` (`ContextError`).
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
