---
title: "Round 642 MultiAsset MaryValue decoder closes Conway UTXO (A5 Phase-2.5)"
parent: Reference
---

# Round 642 MultiAsset MaryValue decoder closes Conway UTXO (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Adds the Mary-era multi-asset value decoder and wires Conway
UTXO tags 6 (`ValueNotConservedUTxO`) and 15
(`CollateralContainsNonADA`). **After R642, all 23 Conway UTXO
variants carry typed payloads — the Conway UTXO sub-rule is
fully typed.**

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/mary/impl/src/Cardano/Ledger/Mary/Value.hs:108-156,181-182,342-353`
  (`MaryValue`, `MultiAsset`, `PolicyID`, `AssetName`; CBOR
  encoder — bare integer for ADA-only, 2-array `[coin, ma]`
  otherwise; `MultiAsset` is a nested map `{PolicyID:
  {AssetName: amount}}`).
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/Utxo.hs:88-89,121,319,328`
  (`ValueNotConservedUTxO (Mismatch RelEQ (Value era))` encoded
  via ToGroup consumed-first; `CollateralContainsNonADA (Value
  era)`).

## Changes

- Added `PolicyId([u8; 28])` — minting-policy identifier.
  Display: `PolicyID {policyID = ScriptHash "<hex>"}`.
- Added `AssetName(Vec<u8>)` — native-asset name. Display: the
  quoted hex of the bytes (matching upstream `Show AssetName =
  show . assetNameToBytesAsHex`).
- Added `MultiAsset` — `Vec<(PolicyId, Vec<(AssetName, i64)>)>`,
  decodes the nested CBOR map. Display: `MultiAsset (fromList
  [(<PolicyID>,fromList [(<AssetName>,<amount>)]),...])`.
- Added `MaryValue { coin: u64, assets: MultiAsset }` —
  `from_decoder` peeks the major type: bare integer → ADA-only
  value; 2-element array → `[coin, multiasset]`. Display:
  `MaryValue (Coin <n>) (<MultiAsset>)`.
- Refactored `ConwayUtxoPredFailure::ValueNotConservedUTxO(Vec<u8>)`
  → `ValueNotConservedUTxO(Mismatch<MaryValue>)` (ToGroup
  flattened, consumed-first per upstream comment) and
  `CollateralContainsNonADA(Vec<u8>)` →
  `CollateralContainsNonADA(MaryValue)`.

3 new tests + 1 replaced:
- `_value_not_conserved_tag6` — Mismatch with bare-coin
  consumed + multi-asset produced.
- `_value_not_conserved_ada_only_tag6` — replaces R637's
  `_routes_pending_to_raw_tag6`; both sides bare-coin.
- `_collateral_contains_non_ada_tag15` — MaryValue with a
  policy bundle.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (299 lib + 4
  doctests + 1 main, +2 net new tests vs R641 baseline of 297 —
  added 3, replaced 1)

## Conway predicate-failure tree status

- LEDGER: 9/9 typed (root + UTXOW/CERTS/GOV sub-rules).
- UTXOW: 17/19 typed (only tag 10 MissingRedeemers raw).
- UTXO: **23/23 typed** (closed by R642).
- UTXOS: 1/2 typed (only CollectErrors raw).
- CERTS → CERT: DELEG 8/8, POOL 6/6, GOVCERT 6/6 typed.
- GOV: 1/19 typed (governance-specific decoders pending).

## Remaining (A5 Phase-2.5+)

- Conway UTXOW tag 10 (MissingRedeemers — PlutusPurpose AsItem).
- Conway UTXOS `CollectErrors`.
- Conway GOV raw variants (18 governance-specific decoders).
- Typed Byron bootstrap parse.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
