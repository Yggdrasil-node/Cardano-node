---
title: "Round 569 tx-generator DumpToFile Babbage key-witnessed"
parent: Reference
---

# Round 569 tx-generator DumpToFile Babbage key-witnessed

Date: 2026-05-20

## Scope

This round extends `Benchmarking.Script.Core.submitInEra`
`SubmitMode::DumpToFile` coverage from Alonzo into Babbage
key-witnessed transaction streams. It deliberately keeps Plutus-
bearing Babbage witness sets, inline datums, and reference scripts on
explicit `TxGenError` boundaries until their downstream mirrors land.

## Upstream references

- `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/Script/Core.hs`
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/babbage/impl/src/Cardano/Ledger/Babbage/Tx.hs`
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/babbage/impl/src/Cardano/Ledger/Babbage/TxBody.hs`
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/babbage/impl/src/Cardano/Ledger/Babbage/TxOut.hs`
- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-binary/src/Cardano/Ledger/Binary/Decoding/Sized.hs`
- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/Plutus/Data.hs`
- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-core/src/Cardano/Ledger/MemoBytes/Internal.hs`

## Changes

- Added `show_babbage_tx_for_dump`, `show_babbage_tx_out_list`,
  `show_babbage_tx_out`, `show_babbage_datum`, and
  `show_babbage_script_ref` helpers in
  `crates/tools/tx-generator/src/script/core.rs`.
- Wired the `MultiEraSubmittedTx::Babbage` arm of `show_tx_for_dump`
  to the new renderer; widened the unsupported-era fallback error
  message and the file-level doc comment from "Shelley-through-Alonzo"
  to "Shelley-through-Babbage".
- Rendered Babbage outputs as `Sized {sizedValue = (addr, val,
  datum, refScript), sizedSize = N}` 4-tuples, with `sizedSize`
  computed from `BabbageTxOut::to_cbor_bytes().len()` so the canonical
  post-Alonzo map encoding matches upstream `mkSized`.
- Reused `show_alonzo_witness_set` because Babbage's `TxWits` is the
  Alonzo `AlonzoTxWits` type unchanged; the envelope renders as
  `ShelleyTx ShelleyBasedEraBabbage (AlonzoTx {...})`.
- Kept inline datums, reference scripts, certificates, withdrawals,
  updates, aux-data hashes, non-empty mints, script integrity hashes,
  collateral inputs, required signers, network ID, collateral return,
  total collateral, reference inputs, and envelope-level aux data on
  explicit `TxGenError` boundaries using the upstream `btbr*` field
  names.
- Added a script-core `SubmitMode::DumpToFile` test for a Babbage
  `SplitN` stream asserting the Babbage-only record keys
  (`btbrCollateralInputs`, `btbrReferenceInputs`, `btbrCollateralReturn`,
  `btbrTotalCollateral`), the `Sized {sizedValue = (` outputs prefix,
  the `(... ,NoDatum,SNothing), sizedSize = ` tuple suffix, the
  `AlonzoTxWitsRaw` witness shell, and the `IsValid True` flag.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-tx-generator dumptofile`

## Remaining

- Extend renderer into Plutus-bearing Babbage / Conway transaction
  shapes, with inline datum / reference script support.
- Capture upstream-binary comparison evidence once a runnable upstream
  binary environment is available.
