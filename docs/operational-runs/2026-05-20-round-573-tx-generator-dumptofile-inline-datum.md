---
title: "Round 573 tx-generator DumpToFile inline datums"
parent: Reference
---

# Round 573 tx-generator DumpToFile inline datums

Date: 2026-05-20

## Scope

This round lifts the `DatumOption::Inline(PlutusData)` boundary in
`show_babbage_datum`. Previously, any Babbage/Conway transaction with
an inline datum on an output would fail
`SubmitMode::DumpToFile` with `does not yet support inline datums`.
After this round, inline datums render as upstream `Datum
(BinaryData "<latin1-escaped-cbor>")` using R572's
`show_haskell_bytestring` over the PlutusData's canonical CBOR.

Reference scripts on `BabbageTxOut` and Plutus V1/V2/V3 script-witness
bytes inside the witness set remain explicit `TxGenError` boundaries
until their downstream mirrors land.

## Upstream references

- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/Plutus/Data.hs:140-211`
  (`BinaryData` newtype with `deriving newtype Show`, `Datum era` ADT
  with stock-derived Show).

## Changes

- Replaced the `Inline(_)` rejection arm in `show_babbage_datum` with
  proper rendering: `Datum (BinaryData <show_haskell_bytestring(pd.to_cbor_bytes())>)`.
- The single-arg `Datum` constructor wraps the `BinaryData` value at
  showsPrec 11, producing `(BinaryData "...")`; at p=0 inside the
  4-tuple this comes out as `Datum (BinaryData "...")`.
- The `BinaryData ShortByteString deriving newtype Show` instance
  uses the underlying `Show ShortByteString` (= `Show ByteString` with
  Latin1 interpretation), which R572's `show_haskell_bytestring`
  mirrors structurally.
- The SBS stored inside `BinaryData` is the canonical CBOR of the
  Plutus data — `dataToBinaryData (MkData (Memo _ sbs)) = BinaryData
  sbs`. Yggdrasil computes this via `PlutusData::to_cbor_bytes()`.
- Added 3 focused unit tests: NoDatum/DatumHash baseline,
  simple-integer inline datum (with a sanity assertion on the
  `Datum (BinaryData "...")` envelope), and nested-Constr inline
  datum.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-tx-generator dumptofile_babbage` (3 tests)
- `cargo test -p yggdrasil-tx-generator` (203 lib tests + 5
  CLI/golden, +3 from R572 baseline)

## Remaining

- Render reference scripts on `BabbageTxOut` (the
  `script_ref: Option<ScriptRef>` field).
- Render Plutus V1/V2/V3 script-witness bytes inside
  `show_alonzo_witness_set` as `MkAlonzoScript` /
  `PlutusScript (PlutusV{1,2,3} (PlutusBinary "..."))`.
- Render native scripts and bootstrap witnesses.
- Render Conway governance procedures
  (`ctbrVotingProcedures`, `ctbrProposalProcedures`,
  `ctbrCurrentTreasuryValue`, `ctbrTreasuryDonation`).
- Full Haskell `Show (ByteString)` mnemonic-escape coverage
  (`\NUL` ... `\DEL`) for byte parity.
- Capture upstream-binary comparison evidence once a runnable upstream
  binary environment is available.
