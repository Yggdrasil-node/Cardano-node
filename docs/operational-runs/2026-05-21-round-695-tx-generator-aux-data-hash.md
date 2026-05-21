---
title: "Round 695 tx-generator DumpToFile renders tx-body aux-data hash (A4)"
parent: Reference
---

# Round 695 tx-generator DumpToFile renders tx-body aux-data hash (A4)

Date: 2026-05-21

## Scope

Extends the tx-generator `DumpToFile` `Show (Tx)` renderer to
render the tx-body `StrictMaybe TxAuxDataHash` field
(`stbrAuxDataHash` / `atbrAuxDataHash` / `btbrAuxDataHash` /
`ctbrAuxDataHash`) across all six era renderers, instead of
rejecting any tx that sets it.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/Hashes.hs:252-255`
  (`newtype TxAuxDataHash = TxAuxDataHash { unTxAuxDataHash ::
  SafeHash EraIndependentTxAuxData }`, `deriving (Show, …)` —
  stock-derived record Show over a `SafeHash`).

## Changes

- Added `show_strict_maybe_aux_data_hash(Option<[u8; 32]>)` —
  renders `SNothing` / `SJust (TxAuxDataHash {unTxAuxDataHash =
  SafeHash "..."})`.
- All six era renderers (`show_shelley_tx_for_dump` …
  `show_conway_tx_for_dump`) drop the
  `ensure_absent(tx.body.auxiliary_data_hash, …)` gate and
  render the field value.

1 new focused unit test:
- `dumptofile_aux_data_hash_render` — `SNothing` + the set
  `SJust (TxAuxDataHash {…})` form.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-tx-generator` (243 lib + 5 main,
  +1 new test vs R694 baseline of 242)

## Remaining (A4)

- Other `DumpToFile` tx-body fields still gated by
  `ensure_absent` / `ensure_empty_or_absent` (certificates,
  mint, collateral, reference inputs, script-integrity hash,
  auxiliary data, update).
