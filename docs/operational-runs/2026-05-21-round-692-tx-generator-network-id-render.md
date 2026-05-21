---
title: "Round 692 tx-generator DumpToFile renders tx-body network id (A4)"
parent: Reference
---

# Round 692 tx-generator DumpToFile renders tx-body network id (A4)

Date: 2026-05-21

## Scope

Extends the tx-generator `DumpToFile` `Show (Tx)` renderer to
render the tx-body `StrictMaybe Network` field
(`atbrTxNetworkId` / `btbrNetworkId` / `ctbrNetworkId`) for the
Alonzo / Babbage / Conway eras, instead of rejecting any tx
that sets it.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/TxBody.hs`
  (`atbrTxNetworkId :: !(StrictMaybe Network)`); the Babbage /
  Conway `TxBodyRaw` carry the analogous `btbrNetworkId` /
  `ctbrNetworkId`.
- `Network` has a stock-derived nullary `Show` (`Testnet` /
  `Mainnet`), so a set value renders `SJust Testnet` with no
  inner parens.

## Changes

- Added `show_strict_maybe_network(Option<u8>)` — renders
  `SNothing` / `SJust Testnet` / `SJust Mainnet`, reusing the
  existing `show_network` id→name mapping.
- `show_alonzo_tx_for_dump` / `show_babbage_tx_for_dump` /
  `show_conway_tx_for_dump` now render the field value: dropped
  the `ensure_absent(tx.body.network_id, …)` gate and replaced
  the hard-coded `…NetworkId = SNothing` literal with the typed
  render.

1 new focused unit test:
- `dumptofile_strict_maybe_network_renders` — `SNothing` /
  `SJust Testnet` / `SJust Mainnet` plus the invalid-id
  rejection.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-tx-generator` (240 lib + 5 main,
  +1 new test vs R691 baseline of 239)

## Remaining (A4)

- Other `DumpToFile` tx-body fields still gated by
  `ensure_absent` / `ensure_empty_or_absent` (certificates,
  withdrawals, mint, collateral, reference inputs, auxiliary
  data, update).
