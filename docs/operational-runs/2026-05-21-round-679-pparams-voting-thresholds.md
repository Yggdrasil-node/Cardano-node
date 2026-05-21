---
title: "Round 679 Typed PParamsUpdate voting-threshold parameters (A5 Phase-2.5)"
parent: Reference
---

# Round 679 Typed PParamsUpdate voting-threshold parameters (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Types the governance voting-threshold protocol parameters
within a `PParamsUpdate` — `poolVotingThresholds` (id 25) and
`drepVotingThresholds` (id 26).

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/PParams.hs:303-372`
  (`PoolVotingThresholds` — a 5-`UnitInterval` record encoded as
  a flat `encodeListLen 5` array; `DRepVotingThresholds` — a
  10-`UnitInterval` record encoded as a flat array).

## Changes

- Added the `PParamValue::VotingThresholds(Vec<UnitInterval>)`
  variant.
- `PParamsUpdate::from_decoder` dispatches ids 25/26: decodes
  the fixed-length CBOR array of tag-30 rationals.
- Display: `[<r>,<r>,...]`.

1 new focused unit test:
- `pparams_update_decodes_voting_thresholds` — a
  `ParameterChange` setting `poolVotingThresholds` to a
  5-element rational array.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (346 lib + 4
  doctests + 1 main, +1 new test vs R678 baseline of 345)

## Remaining (A5 Phase-2.5+)

- `PParamsUpdate` `costModels` parameter (the per-language
  Plutus cost-model map).
- `CollectError::BadTranslation` (`ContextError`).
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
