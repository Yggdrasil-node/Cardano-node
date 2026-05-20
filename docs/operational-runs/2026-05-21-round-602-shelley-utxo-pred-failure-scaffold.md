---
title: "Round 602 ShelleyUtxoPredFailure scaffold (A5 Phase-2.5)"
parent: Reference
---

# Round 602 ShelleyUtxoPredFailure scaffold (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Opens the `ShelleyUtxoPredFailure` scaffold — the nested UTXO
sub-rule that sits under `ShelleyUtxowPredFailure::UtxoFailure`
(tag 4). Ships the 11-variant enum with typed decoders for the
Mismatch-payload tags (1/2/4) and the no-payload tag (3). The
remaining 7 variants (0/5/6/7/8/9/10) carry raw inner CBOR
pending more elaborate payload decoders.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxo.hs:166-189`
  (`data ShelleyUtxoPredFailure era` 11-variant ADT).
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxo.hs:226-260`
  (`encCBOR` / `decCBOR` via upstream's `Sum` constructor —
  CBOR list with Word8 tag at index 0, payload from index 1).

## Changes

- Added `ShelleyUtxoPredFailure` 11-variant enum:
  - Tag 0 `BadInputsUTxO(Vec<u8>)` — pending TxIn decoder.
  - Tag 1 `ExpiredUTxO(Mismatch<u64>)` — typed (SlotNo).
  - Tag 2 `MaxTxSizeUTxO(Mismatch<u32>)` — typed (Word32, with
    decode-time narrowing range check).
  - Tag 3 `InputSetEmptyUTxO` — no payload (1-element CBOR
    array envelope).
  - Tag 4 `FeeTooSmallUTxO(Mismatch<u64>)` — typed (Coin).
  - Tag 5 `ValueNotConservedUTxO(Vec<u8>)` — pending era-specific
    Value decoder.
  - Tag 6 `OutputTooSmallUTxO(Vec<u8>)` — pending TxOut decoder.
  - Tag 7 `UpdateFailure(Vec<u8>)` — pending PPUP sub-rule.
  - Tag 8 `WrongNetwork(Vec<u8>)` — 3-element [tag, network, set];
    pending Addr decoder.
  - Tag 9 `WrongNetworkWithdrawal(Vec<u8>)` — 3-element;
    pending AccountAddress NonEmptySet decoder.
  - Tag 10 `OutputBootAddrAttrsTooBig(Vec<u8>)` — pending TxOut
    decoder.
- Added `tag()`, `constructor()`, Display impl matching upstream
  stock-derived Show. Display routes Mismatch payloads through
  the existing `Mismatch<T>` Display (R597). Tag-4 (Coin) wraps
  values in `CoinShow` for Quiet-Show `Coin <n>` output. Tag-3
  (no payload) renders bare.
- Added `from_cbor` decoder that walks the outer CBOR array, reads
  the Word8 tag, dispatches per-variant. Length-1 for tag 3,
  length-2 for tags 0/1/2/4/5/6/7/10, length-3 for tags 8/9
  (range-checked at 1..=3 with stricter per-tag length enforcement
  on tag 3).
- Added shared `decode_mismatch_u64` helper for the two
  `Mismatch<u64>` variants (tags 1 and 4); decodes the inner
  2-element array and the supplied/expected unsigned values.

6 focused unit tests:
- `_input_set_empty_decodes_tag3` (no-payload variant).
- `_expired_utxo_decodes_tag1` (Mismatch SlotNo).
- `_max_tx_size_decodes_tag2` (Mismatch Word32 with narrowing).
- `_fee_too_small_decodes_tag4` (Mismatch Coin + CoinShow Display).
- `_routes_tag0_to_raw_variant` (BadInputsUTxO raw routing).
- `_unknown_tag_rejects` (tag 99 rejection).

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (183 lib + 4
  doctests + 1 main, +6 new tests vs R601 baseline of 177)

## Remaining (A5 Phase-2.5+)

- `NonEmptySet TxIn` decoder for tag 0 (each TxIn is a
  `[txid: bytes(32), index: Word16]` 2-element array).
- Era-specific `Mismatch<Value era>` decoder for tag 5 (Shelley
  uses Coin, Mary+ uses MultiAsset).
- `NonEmpty (TxOut era)` decoder for tags 6 and 10
  (era-specific TxOut wire format).
- `ShelleyPpupPredFailure` sub-rule decoder for tag 7.
- Network + `NonEmptySet Addr` decoders for tag 8.
- Network + `NonEmptySet AccountAddress` decoders for tag 9.
- Wire the typed `ShelleyUtxoPredFailure` into
  `ShelleyUtxowPredFailure::UtxoFailure(Vec<u8>)`.
- Wire the typed `ShelleyUtxowPredFailure` (and via it the
  decoded `ShelleyUtxoPredFailure`) into
  `ShelleyLedgerPredFailure::UtxowFailure(Vec<u8>)`.
- Mirror the predicate-failure tree for Allegra/Mary/Alonzo/
  Babbage/Conway eras.
