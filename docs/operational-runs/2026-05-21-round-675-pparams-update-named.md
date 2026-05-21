---
title: "Round 675 Named PParamsUpdate parameters (A5 Phase-2.5)"
parent: Reference
---

# Round 675 Named PParamsUpdate parameters (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Renders each updated protocol parameter in a `PParamsUpdate` by
its upstream Conway name rather than a bare integer CBOR-map
key.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/PParams.hs`
  (Conway `PParamsUpdate` CBOR-map key assignment — 0 minFeeA,
  1 minFeeB, …, 33 minFeeRefScriptCostPerByte).

## Changes

- Added `PParamsUpdate::param_name` — maps a CBOR `PParamsUpdate`
  map key to its upstream Conway protocol-parameter name
  (`minFeeA` / `keyDeposit` / `costModels` / `drepActivity` /
  `minFeeRefScriptCostPerByte` / …); returns `None` for ids
  outside the known 0-33 range.
- `PParamsUpdate::Display` now renders `(<param-name>,<raw-cbor
  N bytes>)` for known ids and `(param-<id>,<raw-cbor N bytes>)`
  for unknown ids.

1 new test + 1 updated:
- New `pparams_update_renders_named_parameters` — exercises a
  named parameter (33) and an unknown id (99), plus the
  `param_name` lookup directly.
- `_malformed_proposal_parameter_change` updated for the named
  `minFeeA` render.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (343 lib + 4
  doctests + 1 main, +1 new test vs R674 baseline of 342)

## Remaining (A5 Phase-2.5+)

- `PParamsUpdate` per-parameter typed values (~30 protocol
  parameters — Coin / interval / ExUnits / cost-model decoders).
- `CollectError::BadTranslation` (`ContextError`).
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
