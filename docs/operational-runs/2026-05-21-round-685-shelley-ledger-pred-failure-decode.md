---
title: "Round 685 ShelleyLedgerPredFailure::from_cbor + Shelley-family typed decode (A5 Phase-2.5)"
parent: Reference
---

# Round 685 ShelleyLedgerPredFailure::from_cbor + Shelley-family typed decode (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Adds `ShelleyLedgerPredFailure::from_cbor` and wires the
Shelley/Allegra/Mary/Alonzo/Babbage eras into the top-level
typed-decode path — every era's tx rejection now decodes
end-to-end into its typed predicate-failure tree.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Ledger.hs`
  (`ShelleyLedgerPredFailure` CBOR `Sum` envelope `[tag,
  payload]`, tags 0-3: UtxowFailure / DelegsFailure /
  WithdrawalsNotInRewardsLEDGER / IncompleteWithdrawalsLEDGER).
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/API/Mempool.hs:157-159`
  (`ApplyTxError ShelleyEra = ShelleyApplyTxError (NonEmpty
  (ShelleyLedgerPredFailure ShelleyEra))`).

## Changes

- Added `ShelleyLedgerPredFailure::from_cbor` — decodes the
  2-element `[tag, payload]` envelope and routes each tag to
  its already-typed payload decoder (`ShelleyUtxowPredFailure`,
  `ShelleyDelegsPredFailure`, `Withdrawals`,
  `IncompleteWithdrawals`).
- Added `EraApplyTxError::decode_shelley_failures` — decodes a
  Shelley-family `ApplyTxError` payload (`NonEmpty
  (ShelleyLedgerPredFailure)`) into `Vec<ShelleyLedgerPredFailure>`.
- Added `TxValidationErrorInCardanoMode::typed_shelley_failures`
  — returns `Some(decode result)` for the
  Shelley/Allegra/Mary/Alonzo/Babbage variants, `None` for
  Conway.

1 new focused unit test:
- `tx_validation_error_typed_shelley_failures_decodes` — a
  Babbage `TxValidationErrorInCardanoMode` whose raw payload is
  a NonEmpty array of one `ShelleyWithdrawalsMissingAccounts`
  failure; asserts the typed decode and the `None` result for
  Conway.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (352 lib + 4
  doctests + 1 main, +1 new test vs R684 baseline of 351)

## Remaining (A5 Phase-2.5+)

- `ContextError` tag 8 (`BabbageContextError`).
- Deeper Shelley DELEGS → DELPL → DELEG/POOL payload decoders
  for the pre-Conway eras.
