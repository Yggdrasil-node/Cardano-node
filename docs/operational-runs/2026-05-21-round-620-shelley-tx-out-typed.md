---
title: "Round 620 Typed Shelley TxOut parser (A5 Phase-2.5)"
parent: Reference
---

# Round 620 Typed Shelley TxOut parser (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Replaces the era-opaque `RawTxOut(Vec<u8>)` carrier with the typed
`ShelleyTxOut { addr: Addr, coin: u64 }` matching upstream
`data ShelleyTxOut era = TxOutCompact !CompactAddr !(CompactForm
(Value era))` (Shelley/Allegra/Mary 2-array wire format). The
`NonEmptyTxOut` carrier used by `ShelleyUtxoPredFailure` tags
6/10 now holds typed Shelley-era outputs end-to-end.

The Alonzo 3-array TxOut and Babbage map-form TxOut shapes remain
pending for the wider per-era predicate-failure tree (Allegra
inherits Shelley TxOut; Mary inherits Shelley TxOut with the
Value swapped to MultiAsset; Alonzo adds an optional datum hash;
Babbage+ moves to a CBOR map). `ShelleyUtxoPredFailure` is era-
tagged at the outer envelope so its variant payloads inherit the
Shelley-era TxOut shape.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/TxOut.hs:56-78,121-122,131-148`
  (`data ShelleyTxOut era = TxOutCompact !CompactAddr !(CompactForm
  (Value era))`, `Show ShelleyTxOut = show . viewCompactTxOut`
  rendering as the Haskell tuple `(<Addr>, <Value>)`).

## Changes

- Renamed `RawTxOut(Vec<u8>)` → `ShelleyTxOut { addr: Addr, coin:
  u64 }` struct with typed fields. `from_decoder` reads the
  canonical 2-element CBOR array `[bytes(addr), coin]`:
  - The address bytes are decoded through R607's
    `Addr::from_decoder` (which captures the raw Cardano address
    bytes — Shelley/Bootstrap typed split pending).
  - The coin is a CBOR unsigned (Word64).
  - Length enforcement: exact 2-array required.
- Display matches upstream `show . viewCompactTxOut` Haskell-tuple
  form: `(<Addr>, Coin <n>)`. Inner Addr renders through R607's
  hex marker; coin renders through R615's `CoinShow` (Quiet-Show
  `Coin <n>`).
- `NonEmptyTxOut::entries` retyped from `Vec<RawTxOut>` to
  `Vec<ShelleyTxOut>`. `from_decoder` simplified — no longer
  needs the original `&[u8]` source for byte-range capture, since
  each entry decodes structurally.
- Updated `ShelleyUtxoPredFailure::from_cbor` tag-6/10 call sites
  to use the simplified single-arg `NonEmptyTxOut::from_decoder`
  signature.
- Removed the now-unused `skip_single_datum` CBOR datum-walker
  helper that was only used by `RawTxOut::from_decoder`.
- Updated R609's `_output_too_small_decodes_tag6` test to assert
  the typed `ShelleyTxOut` fields (addr length, addr header byte,
  coin value) and the typed Display shape `(Addr ..., Coin N)`.
- Added a focused `shelley_tx_out_typed_round_trip` test that
  decodes a 2-array TxOut directly and verifies the Addr +
  Coin typed payload plus the tuple Display.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (228 lib + 4
  doctests + 1 main, +1 net new test vs R619 baseline of 227)

## Remaining (A5 Phase-2.5+)

- Full typed `Addr` Show parse (Shelley vs Bootstrap variant
  split, PaymentCredential + StakeReference) — currently rendered
  as raw hex.
- Alonzo / Babbage / Conway per-era TxOut shapes (3-array with
  optional datum hash for Alonzo; CBOR map for Babbage+).
- Mirror the per-era predicate-failure tree for Allegra / Mary /
  Alonzo / Babbage / Conway (new enum trees with their own
  per-era variant additions; Conway adds 4+ governance-specific
  variants on top of the Babbage set).
