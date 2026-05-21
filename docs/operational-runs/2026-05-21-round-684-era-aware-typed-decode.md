---
title: "Round 684 Era-aware typed decode through TxValidationErrorInCardanoMode (A5 Phase-2.5)"
parent: Reference
---

# Round 684 Era-aware typed decode through TxValidationErrorInCardanoMode (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Wires the top-level `TxValidationErrorInCardanoMode` rejection
wrapper to the typed Conway predicate-failure tree — the final
A5 Phase-2.5 integration step. A Conway-era tx rejection now
decodes end-to-end into the typed `ConwayLedgerPredFailure`
tree built across R623-R683.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/API/Mempool.hs:107,157-159`
  (`newtype ApplyTxError <era>` — for Conway `ConwayApplyTxError
  (NonEmpty (ConwayLedgerPredFailure ConwayEra))`, `deriving
  newtype (EncCBOR, DecCBOR, …)` — the CBOR is the `NonEmpty`
  list of predicate failures).

## Changes

- Added `EraApplyTxError::decode_conway_failures` — decodes the
  Conway `ApplyTxError` raw CBOR (a non-empty CBOR array of
  `ConwayLedgerPredFailure` envelopes) into `Vec<ConwayLedgerPredFailure>`,
  routing each element through the typed tree via
  `ConwayLedgerPredFailure::from_cbor`. Rejects an empty
  NonEmpty.
- Added `TxValidationErrorInCardanoMode::typed_conway_failures`
  — returns `Some(decode result)` for the Conway variant,
  `None` for the other eras (whose `ShelleyLedgerPredFailure`
  payload is not yet CBOR-decodable).

1 new focused unit test:
- `tx_validation_error_typed_conway_failures_decodes` — a
  Conway `TxValidationErrorInCardanoMode` whose raw payload is
  a NonEmpty array of one `ConwayWdrlNotDelegatedToDRep`
  failure; asserts the typed decode and the `None` result for a
  Babbage-era rejection.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (351 lib + 4
  doctests + 1 main, +1 new test vs R683 baseline of 350)

## Remaining (A5 Phase-2.5+)

- `ShelleyLedgerPredFailure::from_cbor` — to extend the typed
  decode to the Shelley/Allegra/Mary/Alonzo/Babbage eras.
- `ContextError` tag 8 (`BabbageContextError`).
