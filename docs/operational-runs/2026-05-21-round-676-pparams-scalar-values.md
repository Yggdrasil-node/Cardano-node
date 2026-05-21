---
title: "Round 676 Typed scalar PParamsUpdate values (A5 Phase-2.5)"
parent: Reference
---

# Round 676 Typed scalar PParamsUpdate values (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Types the integer-valued protocol parameters within a
`PParamsUpdate` — they decode to a typed `Word` rather than a
raw byte capture.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/PParams.hs`
  (Conway `PParamsUpdate` — most parameters are `Coin` /
  `Word16` / `Word32` / `EpochInterval`, all CBOR unsigned
  integers; `a0`/`rho`/`tau`/`minFeeRefScriptCostPerByte` are
  rationals; cost models / prices / ExUnits / voting thresholds
  are structured).

## Changes

- Added `PParamValue` enum — `Word(u64)` for integer-valued
  parameters, `Raw(Vec<u8>)` for the rational / structured
  parameters. Display: the typed integer, or a `<raw-cbor N
  bytes>` marker.
- `PParamsUpdate::from_decoder` now peeks each value's CBOR
  major type: a plain unsigned (major 0) decodes to
  `PParamValue::Word`; everything else is captured raw.
- `PParamsUpdate.updates` is now `Vec<(u64, PParamValue)>`.
- Display renders `(<param-name>,<value>)` — integer parameters
  show the typed value inline.

1 test updated:
- `pparams_update_renders_named_parameters` — asserts the
  integer parameters now render their typed `Word` value (`5`,
  `6`) rather than a `<raw-cbor>` marker.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (343 lib + 4
  doctests + 1 main)

## Remaining (A5 Phase-2.5+)

- `PParamsUpdate` rational / structured parameter values (`a0`,
  `rho`, cost models, ExUnits, voting thresholds).
- `CollectError::BadTranslation` (`ContextError`).
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
