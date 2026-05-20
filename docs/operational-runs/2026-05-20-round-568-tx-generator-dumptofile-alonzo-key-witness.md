---
title: "Round 568 tx-generator DumpToFile Alonzo key-witnessed"
parent: Reference
---

# Round 568 tx-generator DumpToFile Alonzo key-witnessed

Date: 2026-05-20

## Scope

This round extends `Benchmarking.Script.Core.submitInEra`
`SubmitMode::DumpToFile` coverage from Shelley/Mary into Alonzo
key-witnessed transaction streams. It deliberately keeps Plutus-bearing
Alonzo-family witness sets on an explicit `TxGenError` boundary until
the nested witness `Show` shape is mirrored and compared.

## Upstream references

- `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/Script/Core.hs`
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/Tx.hs`
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/TxBody.hs`
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/alonzo/impl/src/Cardano/Ledger/Alonzo/TxWits.hs`
- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/MemoBytes/Internal.hs`

## Changes

- Added Alonzo `Show(Tx)` rendering for key-witnessed transactions,
  including `AlonzoTxBodyRaw`, upstream field names, body hash,
  `AlonzoTxWitsRaw`, empty `TxDats` / `Redeemers` memo wrappers,
  and `IsValid` rendering.
- Kept script-integrity hashes, collateral, required signers, auxiliary
  data, mint, and non-vkey witness sets as explicit `TxGenError`
  boundaries.
- Added a script-core `SubmitMode::DumpToFile` test for an Alonzo
  `SplitN` stream.

## Validation

- `cargo fmt --all -- --check`
- `cargo test -p yggdrasil-tx-generator dumptofile`

## Remaining

- Extend renderer into Plutus-bearing Alonzo-family transaction shapes.
- Capture upstream-binary comparison evidence once a runnable upstream
  binary environment is available.
