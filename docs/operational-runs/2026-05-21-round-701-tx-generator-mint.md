---
title: "Round 701 tx-generator DumpToFile renders tx-body mint (A4)"
parent: Reference
---

# Round 701 tx-generator DumpToFile renders tx-body mint (A4)

Date: 2026-05-21

## Scope

Extends the tx-generator `DumpToFile` `Show (Tx)` renderer to
render the tx-body `mint` field (`atbrMint` / `btbrMint` /
`ctbrMint`) for the Mary / Alonzo / Babbage / Conway era
renderers, instead of rejecting any tx that carries a
non-empty mint.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/mary/impl/src/Cardano/Ledger/Mary/TxBody.hs`
  (`atbrMint :: !MultiAsset` — mint carries signed quantities;
  the Alonzo/Babbage/Conway `TxBodyRaw` carry the analogous
  fields). `MultiAsset` Show: `MultiAsset (fromList [(PolicyID
  {policyID = ScriptHash "..."},fromList [("<asset>",<qty>),
  …]), …])`.

## Changes

- Added `show_mint(Option<&MintAsset>)` — renders `MultiAsset
  (fromList [...])` over the `BTreeMap<PolicyId,
  BTreeMap<AssetName, i64>>` mint map (signed quantities,
  supporting burns).
- The Mary / Alonzo / Babbage / Conway renderers drop the
  `ensure_empty_mint` gate and render the field value; the
  now-unused `ensure_empty_mint` helper is removed.

1 new focused unit test:
- `dumptofile_mint_render` — empty/absent → `MultiAsset
  (fromList [])`; a one-policy/one-asset burn (negative
  quantity).

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-tx-generator` (249 lib + 5 main,
  +1 new test vs R700 baseline of 248)

## Remaining (A4)

- Other `DumpToFile` tx-body fields still gated by
  `ensure_absent` / `ensure_empty_or_absent` (certificates,
  auxiliary data, update).
