---
title: "Round 694 tx-generator DumpToFile renders required-signer hashes (A4)"
parent: Reference
---

# Round 694 tx-generator DumpToFile renders required-signer hashes (A4)

Date: 2026-05-21

## Scope

Extends the tx-generator `DumpToFile` `Show (Tx)` renderer to
render the tx-body required-signer-hash set
(`atbrReqSignerHashes` / `btbrReqSignerHashes` /
`ctbrReqSignerHashes`) for the Alonzo / Babbage / Conway era
renderers, instead of rejecting any tx that carries a non-empty
set.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/TxBody.hs`
  (`atbrReqSignerHashes :: !(Set (KeyHash 'Witness))`); the
  Babbage / Conway `TxBodyRaw` carry the analogous fields.
- `KeyHash` has a stock-derived record `Show` (`KeyHash
  {unKeyHash = "..."}`); `Data.Set` Show renders sorted.

## Changes

- Added `show_req_signer_hashes(Option<&[[u8; 28]]>)` — renders
  `fromList [KeyHash {unKeyHash = "..."}, …]` with the hashes
  sorted to match upstream `Set` ordering.
- `show_alonzo_tx_for_dump` / `show_babbage_tx_for_dump` /
  `show_conway_tx_for_dump` drop the
  `ensure_empty_or_absent(tx.body.required_signers, …)` gate and
  render the field value.

1 new focused unit test:
- `dumptofile_req_signer_hashes_render` — empty/absent → empty
  `fromList`; two out-of-order hashes → sorted output.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-tx-generator` (242 lib + 5 main,
  +1 new test vs R693 baseline of 241)

## Remaining (A4)

- Other `DumpToFile` tx-body fields still gated by
  `ensure_absent` / `ensure_empty_or_absent` (certificates,
  mint, collateral, reference inputs, auxiliary data, update).
