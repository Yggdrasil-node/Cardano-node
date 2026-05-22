---
title: "Round 798 dmq-node inbound-V2 pickTxsToDownload"
parent: Reference
---

# Round 798 dmq-node inbound-V2 pickTxsToDownload

Date: 2026-05-22

## Scope

Continues the dmq-node inbound-V2 arc — the `pick_txs_to_download`
fold-over-peers wrapper completing the `pickTxsToDownload` decision
logic.

## What shipped

`crates/tools/dmq-node/src/inbound_v2.rs`:

- `pick_txs_to_download` — mirror of upstream `pickTxsToDownload`
  (`Decision.hs`). Folds the peers (the upstream `mapAccumR`,
  right-to-left, decisions restored to input order), threading the
  `PickTxsState` accumulator through `pick_peer_step`, then runs the
  `gn` finalization: the per-peer updated states, the
  reference-count subtraction (via `update_ref_counts`), the
  buffered-tx restriction to the live set, and the
  in-submission-to-mempool counter update. Fully-empty decisions are
  excluded from the result.

1 unit test covers a busy peer getting a non-empty decision, an idle
(at-capacity) peer's empty decision being dropped, and the shared
state retaining both peers and recording the request.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 177 lib (+1 vs R797's 176) +
  2 golden, all green.

## Remaining (dmq-node inbound-V2)

The `Decision.hs` `makeDecisions` orchestrator and the `State.hs`
`receivedTxIdsImpl` / `collectTxsImpl` state mutations.
