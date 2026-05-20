---
title: "Round 571 tx-generator DumpToFile Mary multi-asset values"
parent: Reference
---

# Round 571 tx-generator DumpToFile Mary multi-asset values

Date: 2026-05-20

## Scope

This round lifts the multi-asset boundary in
`show_mary_value` so non-empty `MultiAsset` bundles render the
upstream `Show (MaryValue)` text instead of returning a `TxGenError`.
Because Mary, Alonzo, Babbage, and Conway transaction outputs all
forward to `show_mary_value`, this single change covers the
multi-asset case for every era's `tx_out` renderer.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/mary/impl/src/Cardano/Ledger/Mary/Value.hs`
- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/Hashes.hs` (`ScriptHash`, `SafeHash`)
- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/Coin.hs` (`Coin` via `Quiet`)

## Changes

- Rewrote `show_mary_value` in
  `crates/tools/tx-generator/src/script/core.rs` to render any `Value`
  variant, including non-empty `MultiAsset` bundles.
- Added `show_multi_asset_entries` helper that produces the
  upstream `fromList [(PolicyID {policyID = ScriptHash "<hex>"},fromList
  [("<asset-hex>",<qty>),...]),...]` inner body, matching the stock
  derived `Show` for `Map PolicyID (Map AssetName Integer)` and
  honoring upstream byte-lex ordering on both `PolicyID` (via
  `ScriptHash` Hash bytes) and `AssetName` (via `ShortByteString` byte
  ordering). `BTreeMap` iteration order in Rust matches `Data.Map
  toAscList` over the same comparator.
- Added four focused unit tests for the renderer: single-asset
  bundle, empty asset name, multi-policy byte-lex ordering check,
  and multi-asset-per-policy byte-lex ordering check.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-tx-generator dumptofile_mary_value`
  (5 tests including 4 new)
- `cargo test -p yggdrasil-tx-generator` (193 lib tests + 5
  CLI/golden, +4 from R570 baseline)

## Remaining

- Extend `gen_tx` so Mary/Alonzo/Babbage/Conway transactions can
  carry mint and multi-asset outputs; that will exercise this
  renderer end-to-end through the `DumpToFile` flow.
- Extend the renderer into Plutus-bearing Babbage/Conway shapes
  (inline datums, reference scripts, Plutus witness sets) in
  strict-mirror-sized slices.
- Capture upstream-binary comparison evidence once a runnable
  upstream binary environment is available.
