---
title: "Round 658 Era-tolerant TxOut decode (A5 Phase-2.5)"
parent: Reference
---

# Round 658 Era-tolerant TxOut decode (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Makes `ShelleyTxOut::from_decoder` era-tolerant — it now accepts
every on-wire TxOut shape so a predicate-failure carrier holding
outputs from any era (Conway carriers hold Babbage/Conway
map-form outputs) decodes correctly.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/TxOut.hs`
  (Alonzo 3-array TxOut `[address, value, datum_hash]`).
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/babbage/impl/src/Cardano/Ledger/Babbage/TxOut.hs`
  (Babbage CBOR-map TxOut `{0: address, 1: value, 2: datum,
  3: script_ref}`).
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/mary/impl/src/Cardano/Ledger/Mary/Value.hs:342-353`
  (`MaryValue` — bare integer for ADA-only, 2-array `[coin, ma]`
  otherwise).

## Changes

- Added `read_txout_value_lovelace` — reads a TxOut value field
  and returns its lovelace component, accepting the Shelley
  bare-`Coin` form and the Mary+ `MaryValue` form (bare integer
  or `[coin, multiasset]` array, via `MaryValue::from_decoder`).
- Extended `ShelleyTxOut::from_decoder` — peeks the CBOR major
  type and dispatches:
  - major 4 array of len 2 → Shelley/Mary `[addr, value]`.
  - major 4 array of len 3 → Alonzo `[addr, value, datum_hash]`
    (the trailing datum hash is consumed via `skip()`).
  - major 5 map → Babbage/Conway `{0: addr, 1: value, ...}` via
    the new `from_map_decoder` (keys 2/3 datum/script_ref are
    consumed but not stored).
- The struct keeps the headline `{ addr, coin }` shape; Display
  is unchanged (`(<Addr>, Coin <n>)`).

2 new focused unit tests:
- `_decodes_alonzo_three_array` — 3-array TxOut with a trailing
  datum hash.
- `_decodes_babbage_map_form` — map-form TxOut with a
  `[coin, multiasset]` value.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (325 lib + 4
  doctests + 1 main, +2 new tests vs R657 baseline of 323)

## Remaining (A5 Phase-2.5+)

- Deepest leaf payloads: `TxCert`, `PParamsUpdate`,
  `Constitution`, `ContextError`.
- TxOut multi-asset / datum / script-ref fields surfaced in
  Display (currently only lovelace is rendered).
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
