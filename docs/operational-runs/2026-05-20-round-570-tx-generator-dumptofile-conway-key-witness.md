---
title: "Round 570 tx-generator DumpToFile Conway key-witnessed"
parent: Reference
---

# Round 570 tx-generator DumpToFile Conway key-witnessed

Date: 2026-05-20

## Scope

This round extends `Benchmarking.Script.Core.submitInEra`
`SubmitMode::DumpToFile` coverage from Babbage into Conway
key-witnessed transaction streams — the final eras-without-Plutus
slice of the renderer. After this round the `show_tx_for_dump`
dispatch is exhaustive across every `MultiEraSubmittedTx` variant
(Shelley, Allegra, Mary, Alonzo, Babbage, Conway). Inline datums,
reference scripts, non-empty governance procedures, non-zero
treasury donations, and Plutus-bearing witness sets remain on
explicit `TxGenError` boundaries until their downstream mirrors land.

## Upstream references

- `.reference-haskell-cardano-node/bench/tx-generator/src/Cardano/Benchmarking/Script/Core.hs`
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Tx.hs`
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/TxBody.hs`
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/TxWits.hs`
- `.reference-haskell-cardano-node/deps/cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Governance/Procedures.hs`
- `.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-data/src/Data/OSet/Strict.hs`

## Changes

- Added `show_conway_tx_for_dump` and the `ensure_empty_voting_procedures`
  boundary helper in `crates/tools/tx-generator/src/script/core.rs`.
- Wired the `MultiEraSubmittedTx::Conway` arm of `show_tx_for_dump`
  to the new renderer and dropped the catch-all `_ => Err(...)` arm —
  the dispatch is now exhaustive across all six current variants,
  which makes a missed Dijkstra arm a compile error rather than a
  runtime error when that era lands.
- Updated the file-level doc comment from "Shelley-through-Babbage"
  to "Shelley-through-Conway".
- Rendered the upstream 19-field `ConwayTxBodyRaw` record with the
  exact Conway field renames: `ctbrSpendInputs` (vs Babbage's
  `btbrInputs`), combined `ctbrVldt :: ValidityInterval`, `ctbrCerts`
  carried as an `OSet {osSSeq = StrictSeq ..., osSet = ...}` (Conway
  moved off `StrictSeq`), dropped `btbrUpdate` (Conway protocol-
  parameter updates moved to governance), and four added governance
  fields: `ctbrVotingProcedures = VotingProcedures {unVotingProcedures
  = fromList []}`, `ctbrProposalProcedures = OSet {...}`,
  `ctbrCurrentTreasuryValue = SNothing`, `ctbrTreasuryDonation = Coin 0`.
- Reused `show_babbage_tx_out_list` for outputs (Conway shares
  `BabbageTxOut`) and `show_alonzo_witness_set` for witnesses (Conway
  `type TxWits ConwayEra = AlonzoTxWits ConwayEra`).
- Emitted the `ShelleyTx ShelleyBasedEraConway (AlonzoTx ...)`
  envelope and `MkConwayTxBody` body prefix.
- Added `dumptofile_submit_generates_conway_haskell_show_transaction`
  asserting each of the four Conway-only governance keys plus the
  `ctbrSpendInputs` rename, the `OSet` certs shape, the `ctbrVldt`
  field name, the `Sized` outputs prefix, the `NoDatum,SNothing`
  tuple suffix, and the `AlonzoTxWitsRaw` witness shell.

## Validation

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test -p yggdrasil-tx-generator dumptofile` (6 tests including
  the new Conway case)
- `cargo test -p yggdrasil-tx-generator` (189 lib tests + 5 CLI/golden,
  +1 from R569 baseline)

## Remaining

- Extend the renderer into Plutus-bearing Babbage/Conway transaction
  shapes (inline datums, reference scripts, Plutus witness sets,
  non-empty governance procedures).
- Capture upstream-binary comparison evidence once a runnable upstream
  binary environment is available.
