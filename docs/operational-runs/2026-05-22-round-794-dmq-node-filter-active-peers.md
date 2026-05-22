---
title: "Round 794 dmq-node inbound-V2 filterActivePeers"
parent: Reference
---

# Round 794 dmq-node inbound-V2 filterActivePeers

Date: 2026-05-22

## Scope

Continues the dmq-node inbound-V2 arc — the `filter_active_peers`
governor function.

## What shipped

`crates/tools/dmq-node/src/inbound_v2.rs`:

- `filter_active_peers` — mirror of upstream `filterActivePeers`
  (`Decision.hs`). Filters the governor's `peer_tx_states` to the
  peers that can currently either request more txids (no request in
  flight, the unacknowledged count under the limit, request capacity
  available via `split_acknowledged_tx_ids`) or download a tx (under
  the per-peer in-flight size limit, with at least one requestable
  available txid not already in-flight / unknown / buffered / over
  the inflight-multiplicity limit / in submission to the mempool).

Now portable since `split_acknowledged_tx_ids` (R793) is in place.

1 unit test covers an active peer being kept and an idle peer being
filtered out.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 172 lib (+1 vs R793's 171) +
  2 golden, all green.

## Remaining (dmq-node inbound-V2)

The remaining `State.hs` / `Decision.hs` governor functions —
`acknowledgeTxIds`, `receivedTxIdsImpl`, `collectTxsImpl`,
`pickTxsToDownload`, `makeDecisions`.
