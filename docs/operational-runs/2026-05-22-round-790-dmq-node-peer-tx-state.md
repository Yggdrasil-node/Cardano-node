---
title: "Round 790 dmq-node inbound-V2 PeerTxState"
parent: Reference
---

# Round 790 dmq-node inbound-V2 PeerTxState

Date: 2026-05-22

## Scope

Continues the dmq-node inbound-V2 arc — the `PeerTxState` per-peer
governor state record.

## What shipped

`crates/tools/dmq-node/src/inbound_v2.rs`:

- `PeerTxState` — the inbound tx-submission governor's per-peer
  state, mirror of upstream `data PeerTxState txid tx` (concrete over
  the DMQ `SigId` / `Sig`): the ordered `unacknowledged_tx_ids`, the
  `available_tx_ids` size map, the in-flight request tracking
  (`requested_tx_ids_inflight`, `requested_txs_inflight_size`,
  `requested_txs_inflight`), the `unknown_txs` set, the `score` decay
  metric with its `score_ts` timestamp, and the `downloaded_txs` /
  `to_mempool_txs` maps.

`PartialEq` only — upstream derives `Eq` but `score` is `f64`.
`Default` is a fresh peer's empty state. Upstream's `Time` timestamp
is modelled as a `Duration` since the monotonic origin.

2 unit tests cover the empty default and the offered / in-flight
tracking fields.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 165 lib (+2 vs R789's 163) +
  2 golden, all green.

## Remaining (dmq-node inbound-V2)

`SharedTxState` (the shared state across all peers) and the governor
decision functions.
