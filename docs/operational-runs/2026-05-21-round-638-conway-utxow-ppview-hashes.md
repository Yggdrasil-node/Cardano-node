---
title: "Round 638 Typed Conway UTXOW PPViewHashesDontMatch variant (A5 Phase-2.5)"
parent: Reference
---

# Round 638 Typed Conway UTXOW PPViewHashesDontMatch variant (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Types Conway UTXOW tag 13 (`PPViewHashesDontMatch`) by adding
the `StrictMaybeScriptIntegrityHash` type. After R638, **16 of
19 Conway UTXOW variants carry typed payloads.**

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/Utxow.hs:93-94,237`
  (`PPViewHashesDontMatch (Mismatch RelEQ (StrictMaybe
  ScriptIntegrityHash))`; encoder `Sum PPViewHashesDontMatch 13
  !> ToGroup mm`).
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/TxBody.hs:125`
  (`type ScriptIntegrityHash = SafeHash
  EraIndependentScriptIntegrity` — 32-byte SafeHash).

## Changes

- Added `StrictMaybeScriptIntegrityHash(Option<[u8; 32]>)` —
  decodes a `StrictMaybe ScriptIntegrityHash` from a CBOR list
  (0-element = SNothing, 1-element = SJust 32-byte hash).
  Display: `SNothing` / `SJust (SafeHash "<hex>")`.
- Refactored `ConwayUtxowPredFailure::PPViewHashesDontMatch(Vec<u8>)`
  → `PPViewHashesDontMatch(Mismatch<StrictMaybeScriptIntegrityHash>)`.
  `from_cbor` decodes the 3-element ToGroup-flattened envelope
  `[13, supplied SMaybe, expected SMaybe]`. Display routes
  through the generic `Mismatch` Display.

1 new focused unit test:
- `_pp_view_hashes_dont_match_tag13` — Mismatch with supplied
  SJust + expected SNothing, asserting the full nested Display.

Lint cleanup: removed a duplicated doc comment at the insertion
anchor.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (292 lib + 4
  doctests + 1 main, +1 new test vs R637 baseline of 291)

## Remaining (A5 Phase-2.5+)

- Conway UTXOW raw variants: tag 10 (MissingRedeemers —
  `NonEmpty (PlutusPurpose AsItem, ScriptHash)`), tag 18
  (ScriptIntegrityHashMismatch — Mismatch + StrictMaybe
  ByteString).
- Conway UTXO raw variants (tags 6/13/15/21).
- Conway UTXOS `CollectErrors`.
- Conway GOV raw variants (18 governance-specific decoders).
- Typed Byron bootstrap parse.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
