---
title: "Round 800 dmq-node inbound-V2 receivedTxIdsImpl"
parent: Reference
---

# Round 800 dmq-node inbound-V2 receivedTxIdsImpl

Date: 2026-05-22

## Scope

Continues the dmq-node inbound-V2 arc — the `received_tx_ids_impl`
inbound-message handler.

## What shipped

`crates/tools/dmq-node/src/inbound_v2.rs`:

- `received_tx_ids_impl` — mirror of upstream `receivedTxIdsImpl`
  (`State.hs`). Records the txids a peer sends in a `MsgReplyTxIds`:
  txids already in the mempool are buffered as `None` (no download
  needed), the rest are added to the peer's `available_tx_ids`
  (unless already unacknowledged or buffered), all received txids are
  appended to `unacknowledged_tx_ids` with their reference counts
  incremented, and `req_no` outstanding txid requests are cleared.
  The `mempool_has_tx` callback mirrors upstream's `mempoolHasTx`.

1 unit test covers an in-mempool txid being buffered, a fresh txid
being offered for download, the unacknowledged/ref-count updates, and
the cleared in-flight request count.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 180 lib (+1 vs R799's 179) +
  2 golden, all green.

## Remaining (dmq-node inbound-V2)

The `State.hs` `collectTxsImpl` handler (recording received txs and
the advertised-vs-received size check).
