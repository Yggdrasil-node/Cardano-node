---
title: "Round 639 Typed Conway UTXOW ScriptIntegrityHashMismatch variant (A5 Phase-2.5)"
parent: Reference
---

# Round 639 Typed Conway UTXOW ScriptIntegrityHashMismatch variant (A5 Phase-2.5)

Date: 2026-05-21

## Scope

Types Conway UTXOW tag 18 (`ScriptIntegrityHashMismatch`) by
adding the `StrictMaybeBytes` type. After R639, **17 of 19
Conway UTXOW variants carry typed payloads** — only tag 10
(MissingRedeemers, the `PlutusPurpose AsItem` form) remains raw.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/Utxow.hs:106-109,242`
  (`ScriptIntegrityHashMismatch (Mismatch RelEQ (StrictMaybe
  ScriptIntegrityHash)) (StrictMaybe ByteString)`; encoder `Sum
  ScriptIntegrityHashMismatch 18 !> To x !> To y`).
- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/BaseTypes.hs:847-852`
  (`EncCBOR (Mismatch r a)` — `Rec Mismatch !> To supplied !>
  To expected`, a nested 2-element record array).

## Changes

- Added `StrictMaybeBytes(Option<Vec<u8>>)` — decodes a
  `StrictMaybe ByteString` from a CBOR list (0-element =
  SNothing, 1-element = SJust bytes). Display: `SNothing` /
  `SJust <bytestring N bytes>` (the bytes render as a hex
  marker — cardano-submit-api does not carry the full
  mnemonic-escape ByteString Show helper).
- Refactored `ConwayUtxowPredFailure::ScriptIntegrityHashMismatch(Vec<u8>)`
  → struct variant `{ mismatch:
  Mismatch<StrictMaybeScriptIntegrityHash>, provided:
  StrictMaybeBytes }`.
- `from_cbor` decodes the 3-element envelope `[18, Mismatch
  2-array, StrictMaybe ByteString]`. The inner Mismatch is a
  nested 2-element array `[supplied, expected]` (the `To`
  encoding — not ToGroup-flattened, distinct from tag 13's
  ToGroup form).
- Display: `ScriptIntegrityHashMismatch (<Mismatch>)
  (<StrictMaybeBytes>)`.

1 new focused unit test:
- `_script_integrity_hash_mismatch_tag18` — nested Mismatch
  (supplied SJust + expected SNothing) + provided SJust bytes,
  asserting the full nested Display.

Lint cleanup: removed a duplicated doc comment at the insertion
anchor.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-cardano-submit-api` (293 lib + 4
  doctests + 1 main, +1 new test vs R638 baseline of 292)

## Remaining (A5 Phase-2.5+)

- Conway UTXOW tag 10 (`MissingRedeemers` — `NonEmpty
  (PlutusPurpose AsItem, ScriptHash)`; the AsItem form carries
  the actual TxIn/PolicyID/TxCert/AccountAddress/Voter/
  ProposalProcedure item, which requires the per-item typed
  decoders).
- Conway UTXO raw variants (tags 6/13/15/21).
- Conway UTXOS `CollectErrors`.
- Conway GOV raw variants (18 governance-specific decoders).
- Typed Byron bootstrap parse.
- Era-aware top-level wiring through `TxValidationErrorInCardanoMode`.
