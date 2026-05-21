---
title: "Round 680 Typed PParamsUpdate costModels parameter (A5 Phase-2.5)"
parent: Reference
---

# Round 680 Typed PParamsUpdate costModels parameter (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Types the `costModels` protocol parameter within a
`PParamsUpdate` (id 18) — the last raw `PParamValue` variant.
**Every Conway `PParamsUpdate` parameter value now decodes to a
typed `PParamValue`.**

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/Plutus/CostModels.hs`
  (`CostModels` — a `Map Language [Int64]`; CBOR map keyed by
  the Word8 language id, each value the flat cost-integer
  array).

## Changes

- Added the `PParamValue::CostModels(Vec<(u64, Vec<i64>)>)`
  variant — per-language `(language-id, cost-integer-array)`
  entries.
- `PParamsUpdate::from_decoder` dispatches id 18: decodes the
  `{language-id: [cost-int, ...]}` CBOR map.
- Display: `CostModels (fromList [(PlutusV1,<N costs>),...])` —
  surfaces each language and its cost-array length.

1 new focused unit test:
- `pparams_update_decodes_cost_models` — a `ParameterChange`
  setting `costModels` for two languages, asserting the typed
  per-language cost arrays.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (347 lib + 4
  doctests + 1 main, +1 new test vs R679 baseline of 346)

## Remaining (A5 Phase-2.5+)

- `CollectError::BadTranslation` (`ContextError` — era-specific
  Plutus script-context translation error).
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
