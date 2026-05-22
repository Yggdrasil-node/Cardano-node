---
title: "Round 787 dmq-node MempoolSeq signature store"
parent: Reference
---

# Round 787 dmq-node MempoolSeq signature store

Date: 2026-05-22

## Scope

dmq-node — ports the in-memory signature-mempool data structure the
DMQ `NodeKernel` holds for diffused signatures.

## What shipped

`crates/tools/dmq-node/src/mempool.rs` — new file, port of the pure
core of upstream `Ouroboros.Network.TxSubmission.Mempool.Simple` (the
mempool the DMQ `NodeKernel` field `mempool :: Mempool m SigId
(Sig crypto)` holds):

- `WithIndex<T>` — a mempool entry paired with its monotonic
  insertion index, mirror of upstream `data WithIndex tx`.
- `MempoolSeq<Id, Tx>` — the membership set plus the index-ordered
  entry sequence, mirror of upstream `data MempoolSeq txid tx`, with
  `empty` / `new` / `read` / `has_tx` / `lookup_tx` / `tx_ids_after`
  (mirrors of upstream `empty` / `new` / `read` and the
  `mempoolHasTx` / `mempoolLookupTx` / `mempoolTxIdsAfter` snapshot
  operations).

dmq-node carries its own copy (the R732 dmq-node-local decision — the
core `crates/consensus` mempool is concrete over ledger
transactions). The pure `MempoolSeq` is testable in isolation; the
upstream `Mempool` `StrictTVar` wrapper and the
`TxSubmissionMempoolReader` / `Writer` STM interfaces are the runtime
shell, landing with the dmq-node runtime sub-arc.

`lib.rs` gains `pub mod mempool;`.

4 unit tests cover the empty sentinel, indexed construction,
index-lookup, and the strictly-greater-than `tx_ids_after` query.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations (audit TSV rebuilt for the new file).
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 158 lib (+4) + 2 golden, all
  green.
