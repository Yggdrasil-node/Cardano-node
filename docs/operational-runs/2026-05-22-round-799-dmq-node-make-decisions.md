---
title: "Round 799 dmq-node inbound-V2 makeDecisions orchestrator"
parent: Reference
---

# Round 799 dmq-node inbound-V2 makeDecisions orchestrator

Date: 2026-05-22

## Scope

Continues the dmq-node inbound-V2 arc — the `make_decisions`
orchestrator and the `order_by_rejections` peer ordering.

## What shipped

`crates/tools/dmq-node/src/inbound_v2.rs`:

- `order_by_rejections` — mirror of upstream `orderByRejections`
  (`Decision.hs`). Orders peers by `score` (lower / more-useful
  first), with a salted `peeraddr` hash as the tie-breaker.
- `make_decisions` — mirror of upstream `makeDecisions`
  (`Decision.hs`): draws a salt from the governor PRNG (advancing
  it), orders the peers, runs `pick_txs_to_download`, and collects
  the per-peer decisions into a map. The `peer_rng` step is a
  yggdrasil-side splitmix increment — not byte-identical to
  upstream's `StdGen`; the salt only randomises tie-breaking and has
  no wire effect.
- `hash_with_salt` — the salted-hash tie-breaker helper.

2 unit tests cover score-ordering and the orchestrator
(rng-advance + per-peer decision).

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 179 lib (+2 vs R798's 177) +
  2 golden, all green.

## dmq-node inbound-V2 — decision logic complete

The inbound-V2 governor's decision path is complete: the full state
surface plus `filter_active_peers` → `make_decisions` →
`pick_txs_to_download` → `pick_peer_step` → `acknowledge_tx_ids` →
`split_acknowledged_tx_ids`, with `update_ref_counts` /
`tick_timed_txs`. The remaining `State.hs` surface is the
`receivedTxIdsImpl` / `collectTxsImpl` state mutations (the
governor's inbound-message handlers).
