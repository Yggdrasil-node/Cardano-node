---
title: "Round 793 dmq-node inbound-V2 splitAcknowledgedTxIds"
parent: Reference
---

# Round 793 dmq-node inbound-V2 splitAcknowledgedTxIds

Date: 2026-05-22

## Scope

Continues the dmq-node inbound-V2 arc — the `split_acknowledged_tx_ids`
governor function.

## What shipped

`crates/tools/dmq-node/src/inbound_v2.rs`:

- `split_acknowledged_tx_ids` — mirror of upstream
  `splitAcknowledgedTxIds` (`State.hs`). Splits a peer's
  `unacknowledged_tx_ids` into the longest acknowledgeable prefix and
  the still-unacknowledged remainder, and computes how many new txids
  to request. A txid is acknowledgeable when it is not in-flight and
  is downloaded, unknown-to-the-peer, or already buffered. The
  request count is `min(maxUnacknowledged - unacked - requested +
  acked, maxNumToRequest - requested)`; the upstream `assert`s on the
  unacked / requested limits are `debug_assert!`s.

Reuses `policy::SigDecisionPolicy` (the dmq-node-local
`TxDecisionPolicy`).

2 unit tests cover the known-prefix split and the stop-at-an-in-flight
-txid case.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 171 lib (+2 vs R792's 169) +
  2 golden, all green.

## Remaining (dmq-node inbound-V2)

The remaining `State.hs` / `Decision.hs` governor functions —
`acknowledgeTxIds`, `receivedTxIdsImpl`, `collectTxsImpl`,
`filterActivePeers`, `pickTxsToDownload`, `makeDecisions`.
