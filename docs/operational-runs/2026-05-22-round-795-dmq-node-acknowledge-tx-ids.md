---
title: "Round 795 dmq-node inbound-V2 acknowledgeTxIds"
parent: Reference
---

# Round 795 dmq-node inbound-V2 acknowledgeTxIds

Date: 2026-05-22

## Scope

Continues the dmq-node inbound-V2 arc — the `acknowledge_tx_ids`
governor function.

## What shipped

`crates/tools/dmq-node/src/inbound_v2.rs`:

- `acknowledge_tx_ids` — mirror of upstream `acknowledgeTxIds`
  (`State.hs`). Acknowledges the longest prefix of a peer's
  unacknowledged txids and returns `(tx_ids_to_acknowledge,
  tx_ids_to_request, txs_to_mempool, ref_count_diff, updated_peer)`:
  it selects the downloaded acknowledged txs that can now go to the
  mempool, splits downloaded txs into still-live and acknowledged,
  scores late downloads, restricts `available_tx_ids` / `unknown_txs`
  to the live set, builds the per-txid reference-count diff, and
  produces the updated `PeerTxState`. Txids are only acknowledged
  when new ones can also be requested (a zero-txid `MsgRequestTxIds`
  is a protocol error), so a zero request count yields the no-op
  `(0, 0, ...)` result.

1 unit test covers the known-prefix acknowledgement: the ack count,
the reference-count diff increments, the live-set restriction, and
the updated in-flight request count.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 173 lib (+1 vs R794's 172) +
  2 golden, all green.

## Remaining (dmq-node inbound-V2)

The `State.hs` state-mutation functions (`receivedTxIdsImpl`,
`collectTxsImpl`, `tickTimedTxs`) and the `Decision.hs`
`pickTxsToDownload` / `makeDecisions` orchestrator.
