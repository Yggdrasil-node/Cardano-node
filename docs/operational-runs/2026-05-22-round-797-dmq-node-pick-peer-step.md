---
title: "Round 797 dmq-node inbound-V2 pickTxsToDownload per-peer step"
parent: Reference
---

# Round 797 dmq-node inbound-V2 pickTxsToDownload per-peer step

Date: 2026-05-22

## Scope

Continues the dmq-node inbound-V2 arc — the per-peer step of the
`pickTxsToDownload` decision logic.

## What shipped

`crates/tools/dmq-node/src/inbound_v2.rs`:

- `PickTxsState` — the accumulator threaded across peers by the
  `pick_txs_to_download` fold, mirror of upstream `data St`
  (`st_inflight` / `st_acknowledged` /
  `st_in_submission_to_mempool_txs`).
- `pick_peer_step` — the per-peer `accumFn` of upstream
  `pickTxsToDownload` (`Decision.hs`). It picks a prefix of the
  peer's available txids (skipping buffered / in-flight / unknown /
  in-submission txids) until the per-peer in-flight size limit or the
  per-txid multiplicity limit stops it, records the new requests on
  the peer, calls `acknowledge_tx_ids`, threads the accumulator, and
  produces the `TxDecision`. Upstream's short-circuiting
  `foldWithState` is inlined as a `break`-on-limit loop.

2 unit tests cover picking available txs under the limits and
skipping buffered / in-flight txids.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 176 lib (+2 vs R796's 174) +
  2 golden, all green.

## Remaining (dmq-node inbound-V2)

The `pick_txs_to_download` fold-over-peers wrapper and its `gn`
finalization, the `makeDecisions` orchestrator, and the `State.hs`
`receivedTxIdsImpl` / `collectTxsImpl` state mutations.
