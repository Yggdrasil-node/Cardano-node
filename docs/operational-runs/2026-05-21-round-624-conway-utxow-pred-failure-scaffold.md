---
title: "Round 624 ConwayUtxowPredFailure scaffold (A5 Phase-2.5)"
parent: Reference
---

# Round 624 ConwayUtxowPredFailure scaffold (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Adds the `ConwayUtxowPredFailure` 19-variant scaffold (the
largest sub-rule under Conway LEDGER) and wires
`ConwayLedgerPredFailure::ConwayUtxowFailure(Vec<u8>)` to the
typed enum. After R624, **12 of 19 Conway UTXOW variants carry
typed payloads** by reusing existing Shelley-path carriers; the
7 remaining variants keep raw inner CBOR pending Conway-specific
nested-rule + Plutus-purpose + DataHash + ScriptIntegrityHash
decoders.

The full Conway LEDGER → UTXOW chain now renders typed
end-to-end for 12 of the 19 Conway UTXOW variant paths.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/Utxow.hs:56-110,222-270`
  (variant ADT + CBOR encoder/decoder; tags 0-18).

## Changes

- Added `ConwayUtxowPredFailure` 19-variant enum mirroring
  upstream:
  - **Typed (12 variants):**
    - Tag 1 `InvalidWitnessesUTXOW(NonEmptyVKey)` (R601 reuse).
    - Tag 2 `MissingVKeyWitnessesUTXOW(NonEmptySetKeyHash)`
      (R600 reuse).
    - Tags 3/4/9/16/17 `*(NonEmptySetScriptHash)` (R599 reuse —
      shared decode branch for the 5 NonEmptySet-ScriptHash
      variants).
    - Tags 5/6 `*(TxAuxDataHash)` (R598 reuse).
    - Tag 7 `ConflictingMetadataHash(Mismatch<TxAuxDataHash>)` —
      new `Mismatch<TxAuxDataHash>` with `ToGroup`-flattened
      wire encoding (supplied-then-expected per upstream
      `EncCBORGroup (Mismatch r a)`).
    - Tag 8 `InvalidMetadata` — no payload.
    - Tag 14 `UnspendableUTxONoDatumHash(NonEmptySetTxIn)`
      (R603 reuse).
  - **Raw (7 variants):**
    - Tag 0 (UTXO sub-rule — pending Conway UTXO decoder).
    - Tag 10 (MissingRedeemers — `NonEmpty (PlutusPurpose AsItem
      era, ScriptHash)`).
    - Tag 11/12 (MissingRequiredDatums / NotAllowed —
      `NonEmptySet DataHash + Set DataHash`).
    - Tag 13 (PPViewHashesDontMatch — `Mismatch (StrictMaybe
      ScriptIntegrityHash)`).
    - Tag 15 (ExtraRedeemers — `NonEmpty (PlutusPurpose AsIx)`).
    - Tag 18 (ScriptIntegrityHashMismatch — `Mismatch + StrictMaybe
      ByteString`).
- `from_cbor` enforces exact envelope length per variant (1 for
  tag 8 no-payload; 2 for most tags; 3 for tags 7/11/12/13; 4
  for nothing yet but range allows). Unknown tags reject.
- Display routes typed payloads through their typed Display; raw
  variants emit `<Constructor> <raw-cbor N bytes>`.
- Refactored `ConwayLedgerPredFailure::ConwayUtxowFailure(Vec<u8>)`
  → `ConwayUtxowFailure(ConwayUtxowPredFailure)`. LEDGER
  `from_cbor` dispatcher: tag 1 now decodes through the typed
  sub-rule; tags 2/3 retain the raw payload pending Conway CERTS
  / GOV decoders. Display routes the typed nested payload.
- Updated R623's `_utxow_raw_routing_tag1` test (now `_typed_routing_tag1`) to assert
  end-to-end LEDGER → UTXOW Display for the typed scaffold
  (chosen inner: tag-8 `InvalidMetadata` — simplest no-payload
  variant).

6 new focused unit tests:
- `_invalid_metadata_decodes_tag8` — no-payload typed variant.
- `_missing_tx_body_metadata_hash_decodes_tag5` — TxAuxDataHash
  typed.
- `_conflicting_metadata_hash_decodes_tag7` — Mismatch
  TxAuxDataHash via ToGroup-flattened.
- `_missing_script_witnesses_decodes_tag3` — NonEmptySetScriptHash
  with tag-258 tolerance.
- `_routes_pending_to_raw_tag10` — confirms tag 10
  MissingRedeemers still routes through raw payload.
- `_unknown_tag_rejects` — tag 99 rejection.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (243 lib + 4
  doctests + 1 main, +6 new tests vs R623 baseline of 237)

## Remaining (A5 Phase-2.5+)

- Conway UTXOW raw variants: tag 0 (nested Conway UTXO), tag 10
  (MissingRedeemers — `NonEmpty (PlutusPurpose AsItem, ScriptHash)`),
  tag 11/12 (`NonEmptySet DataHash + Set DataHash`), tag 13
  (`Mismatch (StrictMaybe ScriptIntegrityHash)`), tag 15
  (`NonEmpty (PlutusPurpose AsIx)`), tag 18 (`Mismatch +
  StrictMaybe ByteString`).
- Conway CERTS sub-rule (LEDGER tag 2) — replaces Shelley's
  DELEGS; dispatches into CERT → POOL/DELEG/GOVCERT.
- Conway GOV sub-rule (LEDGER tag 3) — new for governance
  actions (proposal procedures, voting, etc.).
- Conway UTXO sub-rule decoder (referenced by UTXOW tag 0) —
  itself a chain of Conway/Babbage/Alonzo/Shelley UTXO predicate
  failures.
- Typed Byron bootstrap parse (uncommon legacy era).
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
