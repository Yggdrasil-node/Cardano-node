---
title: "Round 796 dmq-node inbound-V2 tickTimedTxs"
parent: Reference
---

# Round 796 dmq-node inbound-V2 tickTimedTxs

Date: 2026-05-22

## Scope

Continues the dmq-node inbound-V2 arc — the `tick_timed_txs` governor
state-mutation function.

## What shipped

`crates/tools/dmq-node/src/inbound_v2.rs`:

- `tick_timed_txs` — mirror of upstream `tickTimedTxs` (`State.hs`).
  Advances the governor's `timed_txs` timeouts to a given time:
  timed entries with a deadline strictly before `now` expire, their
  txids' reference counts are decremented (entries reaching zero are
  dropped, via `update_ref_counts`), and `buffered_txs` is restricted
  to the txids that still have a live reference count. The `now`
  entry and later entries are retained. The upstream
  `Map.splitLookup` maps to two `BTreeMap::range` queries.

1 unit test covers an expired entry (ref count → 0 → dropped from
counts, buffered, and timed maps) versus a still-future entry that
survives.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 174 lib (+1 vs R795's 173) +
  2 golden, all green.

## Remaining (dmq-node inbound-V2)

The `State.hs` `receivedTxIdsImpl` / `collectTxsImpl` state mutations
and the `Decision.hs` `pickTxsToDownload` / `makeDecisions`
orchestrator.
