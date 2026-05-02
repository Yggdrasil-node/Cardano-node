# Round 247 - Origin BlockFetch prefix preservation

Date: 2026-05-02

## Summary

R247 fixes a clean-preview replay failure where verified sync could skip
the first ChainSync-announced prefix when a batch started from
`Point::Origin`. The serial verified loop can collect multiple
roll-forward headers before fetching bodies. The old path normalized the
BlockFetch lower bound from `Origin` to the final announced upper bound,
so the first request fetched only the last header's block and silently
missed slots before it.

The fix keeps `normalize_blockfetch_range_points()` conservative for
ordinary callers, but routes verified pending roll-forwards through
`blockfetch_range_for_pending_forwards()`. When the current point is
Origin, that helper uses the first announced concrete ChainSync header as
the BlockFetch lower bound.

## Local Impact

- `node/src/sync.rs::sync_batch_verified_with_tentative()` now preserves
  the full first announced body range when a verified batch starts from
  Origin.
- Regression tests cover both Origin prefix handling and non-Origin lower
  bound preservation.
- `node/src/AGENTS.md` records the operational rule so future sync changes
  do not reintroduce the prefix skip.
- `crates/ledger/src/state.rs` now documents Byron genesis pseudo UTxO ids
  using the upstream `serializeCborHash txOutAddress` formula; the amount
  is not part of the pseudo transaction id.

## Failure Signature

The clean preview replay restored checkpoint `snapshot_84460.dat`, then
failed at Alonzo slot `86600` with an input missing from the UTxO set.
The missing lineage traced back to early preview Byron slots:

```text
slot 60  tx a8fa4293645facb2a0332f4dfc442dff3fc9ca021c95ee908df5d9605e3825be
slot 320 tx e3ca57e8f323265742a8f4e79ff9af884c9ff8719bd4f7788adaea4c33ba07b6
```

Before the fix, the local database contained slot `300` and later blocks,
but not slots `0` through `280`, so the slot-320 transaction's input
lineage was absent.

## Verification

Focused regression:

```text
cargo test -p yggdrasil-node blockfetch_range_ --lib
```

Bounded clean preview replay:

```text
timeout 180s cargo run -p yggdrasil-node -- run \
  --config tmp/preview-producer/config/preview-producer.json \
  --database-path tmp/preview-r247-origin-range-20260502T101721Z \
  --non-producing-node \
  --batch-size 16 \
  --max-concurrent-block-fetch-peers 1 \
  --checkpoint-interval-slots 1000000
```

Result: replay advanced to slot `101100` before timeout. Refscan
confirmed slots `0`, `20`, `40`, `60`, `300`, and `320` were present,
including the two transaction ids above, and the checkpoint at slot `300`
contained `a8fa4293645facb2a0332f4dfc442dff3fc9ca021c95ee908df5d9605e3825be#0`.

## Upstream References

- Preview environment configuration:
  <https://book.world.dev.cardano.org/env-preview.html>
- ChainSync protocol:
  <https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network-protocols/src/Ouroboros/Network/Protocol/ChainSync>
- BlockFetch protocol:
  <https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network-protocols/src/Ouroboros/Network/Protocol/BlockFetch>
- Byron genesis UTxO construction:
  <https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/byron/ledger/impl/src/Cardano/Chain/Genesis/UTxO.hs>
- Byron `fromTxOut` pseudo input construction:
  <https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/byron/ledger/impl/src/Cardano/Chain/UTxO/UTxO.hs>
