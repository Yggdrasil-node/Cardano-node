---
title: "Round 792 dmq-node inbound-V2 RefCountDiff + emptyTxDecision"
parent: Reference
---

# Round 792 dmq-node inbound-V2 RefCountDiff + emptyTxDecision

Date: 2026-05-22

## Scope

Continues the dmq-node inbound-V2 arc — the reference-count update
logic and the empty-decision value.

## What shipped

`crates/tools/dmq-node/src/inbound_v2.rs`:

- `TxDecision::empty` (and a `Default` derive) — the all-zero
  decision, mirror of upstream `emptyTxDecision`.
- `RefCountDiff` — a set of per-txid reference-count decrements,
  mirror of upstream `newtype RefCountDiff txid` (`State.hs`).
- `update_ref_counts` — applies a `RefCountDiff` to a
  reference-count map, mirror of upstream `updateRefCounts`: each
  entry is decremented by the matching diff amount, an entry whose
  count reaches zero is removed, entries absent from the diff are
  carried through, entries present only in the diff are ignored. The
  upstream `assert (x >= y)` underflow check is a `debug_assert!`.

3 unit tests cover the empty decision and the
decrement / carry-through / drop-at-zero cases of `update_ref_counts`.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 169 lib (+2 vs R791's 167) +
  2 golden, all green.

## Remaining (dmq-node inbound-V2)

The interconnected governor decision / state functions of
`Decision.hs` (`makeDecisions`, `filterActivePeers`,
`pickTxsToDownload`) and `State.hs` (`splitAcknowledgedTxIds`,
`acknowledgeTxIds`, `receivedTxIdsImpl`, `collectTxsImpl`).
