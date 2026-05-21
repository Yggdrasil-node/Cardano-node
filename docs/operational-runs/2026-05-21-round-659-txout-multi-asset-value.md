---
title: "Round 659 Surface TxOut multi-asset value in Display (A5 Phase-2.5)"
parent: Reference
---

# Round 659 Surface TxOut multi-asset value in Display (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Changes `ShelleyTxOut` to carry the full `MaryValue` instead of
only the lovelace amount, so a TxOut's native-asset bundle
renders in Display — matching upstream `viewCompactTxOut`'s
`(Addr, Value)` tuple.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/TxOut.hs`
  (`Show ShelleyTxOut = show . viewCompactTxOut`;
  `viewCompactTxOut :: TxOut -> (Addr, Value era)` — for Mary+
  `Value = MaryValue`, so the tuple's second element is the full
  multi-asset value).
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/mary/impl/src/Cardano/Ledger/Mary/Value.hs:181-182`
  (`data MaryValue = MaryValue Coin MultiAsset`).

## Changes

- `ShelleyTxOut` now carries `value: MaryValue` in place of
  `coin: u64`. A Shelley bare-`Coin` output lifts into a
  `MaryValue` with an empty asset bundle.
- `from_decoder` / `from_map_decoder` read the value via
  `MaryValue::from_decoder` directly (the R658
  `read_txout_value_lovelace` helper is removed — `MaryValue`
  already accepts both the bare-integer and `[coin, multiasset]`
  forms).
- Display renders `(<Addr>, <MaryValue>)` — e.g. `(<Addr>,
  MaryValue (Coin <n>) (MultiAsset (fromList [...])))`.
- Added `Ord`/`PartialOrd`/`Hash` derives to `MaryValue`,
  `MultiAsset`, `PolicyId`, and `AssetName` so `ShelleyTxOut`
  keeps its full derive set.
- Updated 6 `.coin` field accesses (now `.value.coin`) and 4
  Display-string assertions across the existing TxOut /
  `BabbageOutputTooSmallUTxO` / `ScriptsNotPaidUTxO` /
  `OutputTooSmallUTxO` tests.

1 new focused unit test:
- `shelley_tx_out_surfaces_multi_asset_value` — a Babbage-map
  TxOut whose value carries a one-policy / one-asset bundle,
  asserting the `MultiAsset` renders in Display.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (326 lib + 4
  doctests + 1 main, +1 net new test vs R658 baseline of 325)

## Remaining (A5 Phase-2.5+)

- Deepest leaf payloads: `TxCert`, `PParamsUpdate`,
  `Constitution`, `ContextError`.
- TxOut datum / script-reference fields surfaced in Display.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
