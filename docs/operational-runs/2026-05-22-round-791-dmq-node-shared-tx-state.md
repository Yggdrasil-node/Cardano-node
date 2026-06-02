---
title: "Round 791 dmq-node inbound-V2 SharedTxState"
parent: Reference
---

# Round 791 dmq-node inbound-V2 SharedTxState

Date: 2026-05-22

## Scope

Continues the dmq-node inbound-V2 arc — the `SharedTxState` record
(the inbound governor's state shared across all peers).

## What shipped

`crates/tools/dmq-node/src/inbound_v2.rs`:

- `SharedTxState<PeerAddr>` — the inbound tx-submission governor's
  cross-peer state, mirror of upstream
  `data SharedTxState peeraddr txid tx` (concrete over the DMQ
  `SigId` / `Sig`, generic over the peer-address key): the
  `peer_tx_states` map, the `inflight_txs` multiplicity map, the
  `buffered_txs` downloaded-tx map, the `reference_counts`, the
  `timed_txs` re-download-avoidance timeouts, the
  `in_submission_to_mempool_txs` counter map, and the `peer_rng`
  ordering-PRNG state.

`PartialEq` only — it holds `PeerTxState` (`PartialEq` via its `f64`
score). A hand-written `Default` avoids a spurious `PeerAddr: Default`
bound. The `peer_rng` field stands in for upstream's `StdGen`; the
actual peer-ordering RNG is governor logic landing later.

2 unit tests cover the empty default and peer registration / tx
buffering.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 167 lib (+2 vs R790's 165) +
  2 golden, all green.

## dmq-node inbound-V2 — state surface complete

`inbound_v2.rs` now has the full inbound-V2 state surface — the
foundational types, `TxDecision`, `PeerTxState`, and `SharedTxState`.
What remains is the governor decision / execution functions
(`makeDecision`, `acknowledgeTxIds`, `pickTxsToDownload`, ...).
