---
title: "Round 674 Typed CollectError NoRedeemer (A5 Phase-2.5)"
parent: Reference
---

# Round 674 Typed CollectError NoRedeemer (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Types the `CollectError::NoRedeemer` variant (tag 0) to carry a
`ConwayPlutusPurposeItem`. After R674, 3 of 4 `CollectError`
variants are fully typed — only `BadTranslation` (`ContextError`)
remains raw.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/Plutus/Context.hs:344-375`
  (`NoRedeemer (PlutusPurpose AsItem era)`; CBOR `Sum` tag 0).

## Changes

- Refactored `CollectError::NoRedeemer(Vec<u8>)` →
  `NoRedeemer(ConwayPlutusPurposeItem)` — the `PlutusPurpose
  AsItem` form already has a typed carrier (R655).
- `CollectError::from_decoder` special-cases tag 0: decodes the
  payload via `ConwayPlutusPurposeItem::from_decoder`; tag 3
  `BadTranslation` keeps its raw byte-range capture.
- Display: `NoRedeemer (<ConwayPlutusPurposeItem>)`.

1 new focused unit test:
- `conway_utxos_pred_failure_collect_errors_no_redeemer` — a
  `CollectErrors` carrying a `NoRedeemer` whose purpose item is
  a `ConwayMinting` PolicyID.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (342 lib + 4
  doctests + 1 main, +1 new test vs R673 baseline of 341)

## Remaining (A5 Phase-2.5+)

- `CollectError::BadTranslation` (`ContextError` — era-specific
  Plutus script-context translation error).
- `PParamsUpdate` per-parameter typed values (~30 protocol
  parameters).
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
