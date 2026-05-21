---
title: "Round 703 tx-generator auxiliary_data blocker — precise narrowing (A4)"
parent: Reference
---

# Round 703 tx-generator auxiliary_data blocker — precise narrowing (A4)

Date: 2026-05-21

## Scope

Narrows the R702 `auxiliary_data` DumpToFile blocker from "the
whole `ShelleyTxAuxData` / `Metadatum` `Show` shape is unknown"
to the precise single remaining unknown, after reading the
upstream `Show` derivations in the vendored reference tree.

## Investigation findings

Reading the reference tree resolved most of the supposed
blocker:

- `ShelleyTxAuxData era = MkShelleyTxAuxData (MemoBytes
  (ShelleyTxAuxDataRaw era))`, stock `deriving Show`
  (`Shelley/TxAuxData.hs:74-76`).
- `Show (MemoBytes t) = "<show raw> (blake2b_256: SafeHash
  \"<mbHash>\")"` (`MemoBytes/Internal.hs:185-192`) — the same
  `… (blake2b_256: SafeHash "…")` pattern the tx-body renderers
  already emit.
- `Show ShelleyTxAuxDataRaw = "ShelleyTxAuxDataRaw
  {stadrMetadata = fromList [...]}"`, stock record `Show`.
- `Metadatum = Map | List | I Integer | B ByteArray | S Text`,
  stock `Show` (`Metadata.hs:52-58`).

The single remaining unknown is **`Show ByteArray`** for the
`Metadatum.B` case — a `primitive`-package (Hackage) detail.
The `primitive` package is not vendored in
`.reference-haskell-cardano-node/`, and no upstream golden or
`cardano-ledger` test vector exercises a `Metadatum.B` `Show`,
so the `B`-byte format genuinely cannot be determined here.

Also confirmed: `certificates` and `update` are provably-dead
defensive gates — `tx_generator/tx.rs::gen_tx` hard-codes
`certificates: None` / `update: None` for every era, so a
tx-generator-built tx can never carry them. They are not gaps.

## Changes (doc-only)

- `crates/tools/tx-generator/AGENTS.md` — replaced the R702
  "DumpToFile remaining-work blocker" entry with the precise
  R703 status: the narrowed `Show ByteArray` blocker (naming
  the missing artifact: the `primitive` package's `instance
  Show ByteArray`, or a golden exercising `Metadatum.B`), and
  the `gen_tx`-hardcoded-None confirmation for
  `certificates` / `update`.

No source change.

## Validation

- `cargo fmt --all -- --check` — green.
- `check-strict-mirror.py --fail-on-violation` — 0 violations.
- `cargo check-all` / `cargo lint` / `cargo test-all` —
  unaffected (doc-only edit).

## Remaining (A4) — blocked

- `auxiliary_data` rendering — blocked on the `primitive`
  package's `Show ByteArray` for `Metadatum.B`.
