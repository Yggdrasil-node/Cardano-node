---
title: "Round 677 Typed rational PParamsUpdate values (A5 Phase-2.5)"
parent: Reference
---

# Round 677 Typed rational PParamsUpdate values (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Types the rational protocol parameters within a `PParamsUpdate`
(`a0`, `rho`, `tau`, `minFeeRefScriptCostPerByte`) — they
decode to a typed `UnitInterval`.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/PParams.hs`
  (the `a0` / `rho` / `tau` / `minFeeRefScriptCostPerByte`
  parameters are `BoundedRatio` rationals — CBOR tag-30
  `#6.30([num, den])`).

## Changes

- Added the `PParamValue::Rational(UnitInterval)` variant.
- `PParamsUpdate::from_decoder` now decodes a CBOR major-type-6
  (tag) value as a tag-30 `UnitInterval` (the only tagged value
  type that appears in a Conway `PParamsUpdate` map); the
  structured array/map parameters still capture raw.
- Display: rational parameters render as `<num> % <den>`.

1 new focused unit test:
- `pparams_update_decodes_rational_value` — a `ParameterChange`
  setting `a0` (id 9) to a tag-30 rational `3 % 1000`.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (344 lib + 4
  doctests + 1 main, +1 new test vs R676 baseline of 343)

## Remaining (A5 Phase-2.5+)

- `PParamsUpdate` structured parameter values (cost models,
  ExUnits prices, max-ExUnits, pool/DRep voting thresholds).
- `CollectError::BadTranslation` (`ContextError`).
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
