---
title: "Round 681 Typed CollectError BadTranslation / ContextError (A5 Phase-2.5)"
parent: Reference
---

# Round 681 Typed CollectError BadTranslation / ContextError (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Adds the `ContextError` 8-variant scaffold and types
`CollectError::BadTranslation` (tag 3) — the last raw
`CollectError` variant. **All four `CollectError` variants now
carry typed payloads.**

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/TxInfo.hs:175-256`
  (`data ConwayContextError era` 8-variant ADT — CBOR `Sum`
  tags 8-15: BabbageContextError, CertificateNotSupported,
  PlutusPurposeNotSupported, CurrentTreasuryFieldNotSupported,
  VotingProceduresFieldNotSupported,
  ProposalProceduresFieldNotSupported,
  TreasuryDonationFieldNotSupported,
  ReferenceInputsNotDisjointFromInputs).

## Changes

- Added the `ContextError` 8-variant enum. Tags 9/10/11/14/15
  carry typed payloads — `CertificateNotSupported` (`TxCert`),
  `PlutusPurposeNotSupported` (`ConwayPlutusPurposeItem`),
  `CurrentTreasuryFieldNotSupported` / `TreasuryDonationField-
  NotSupported` (`Coin`), `ReferenceInputsNotDisjointFromInputs`
  (`NonEmptyTxIn`). Tags 8/12/13 (`BabbageContextError`,
  voting / proposal procedure fields) keep raw inner CBOR.
- Refactored `CollectError::BadTranslation(Vec<u8>)` →
  `BadTranslation(ContextError)`. `CollectError::from_decoder`
  routes tag 3 through `ContextError::from_decoder`.

1 new focused unit test:
- `conway_utxos_pred_failure_collect_errors_bad_translation` —
  a `CollectErrors` carrying a `BadTranslation` whose
  `ContextError` is `CurrentTreasuryFieldNotSupported (Coin
  1000)`.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (348 lib + 4
  doctests + 1 main, +1 new test vs R680 baseline of 347)

## Remaining (A5 Phase-2.5+)

- `ContextError` raw variants (tag 8 BabbageContextError, 12/13
  voting / proposal procedure fields).
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
