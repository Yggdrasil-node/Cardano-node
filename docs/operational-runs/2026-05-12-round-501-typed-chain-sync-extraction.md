---
title: 'R501: extract typed ChainSync API into node/src/sync/chain_sync.rs (sync.rs R-arc round 4/13)'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-12-round-501-typed-chain-sync-extraction/
---

# R501 — typed ChainSync API extraction (sync.rs R-arc, slice 4/13)

**Date:** 2026-05-12
**Predecessor:** [`R500`](2026-05-12-round-500-block-fetch-extraction.md).
**Slice scope:** single round — extract the typed ChainSync driver
API (single-step, multi-step, batch-apply, intersect, and
volatile-store apply helpers) from `node/src/sync.rs` into
`node/src/sync/chain_sync.rs`. Behavior-preserving.

## Slice scope

- 1 public enum (`TypedIntersectResult`).
- 10 public functions:
  - `sync_step`
  - `sync_step_decoded`
  - `sync_step_typed`
  - `sync_steps`
  - `sync_steps_typed`
  - `sync_until_typed`
  - `apply_typed_step_to_volatile`
  - `apply_typed_progress_to_volatile`
  - `typed_find_intersect`
  - `sync_batch_apply`
- 308 LOC moved.

## Mirror mapping

| Yggdrasil leaf | Upstream Haskell affinity |
|---|---|
| `node/src/sync/chain_sync.rs` | `Ouroboros.Consensus.MiniProtocol.ChainSync.Client` (client driver) + `Ouroboros.Network.Protocol.ChainSync.Client` (intersect semantics). The volatile-store apply path corresponds to upstream's `ChainDB.addBlockAsync` + `ChainSel.chainSelectionForBlock`. Yggdrasil collapses these into batch-shaped helper functions. |

Strict mirror: none. Explicit `## Naming parity` stanza per CLAUDE.md.

## Cross-module dependencies

Inbound (chain_sync.rs imports from sync sub-modules):

- `super::block_fetch::{fetch_range_blocks, fetch_range_blocks_decoded, fetch_range_blocks_typed, normalize_blockfetch_range_bytes, normalize_blockfetch_range_points, point_bytes_from_raw_header_or_tip}` — 6 `pub(super)` helpers from R500.
- `super::shelley_decoders::shelley_block_to_block` — already `pub`.
- `super::{DecodedSyncStep, SyncError, SyncProgress, SyncStep, TypedSyncProgress, TypedSyncStep}` — types still resident in `sync.rs` preamble.

Outbound (sync.rs re-exports chain_sync items):

- `pub use chain_sync::{TypedIntersectResult, apply_typed_progress_to_volatile, apply_typed_step_to_volatile, sync_batch_apply, sync_step, sync_step_decoded, sync_step_typed, sync_steps, sync_steps_typed, sync_until_typed, typed_find_intersect};` — 11 names preserved through `lib.rs`'s existing `pub use sync::{...}` block. No `lib.rs` edit.

Imports trimmed from `sync.rs` (no longer referenced after R501):

- `DecodedHeaderNextResponse`, `NextResponse`, `TypedIntersectResponse` (`yggdrasil_network`).
- `fetch_range_blocks`, `fetch_range_blocks_decoded`, `fetch_range_blocks_typed`, `normalize_blockfetch_range_bytes`, `point_bytes_from_raw_header_or_tip` (private `use block_fetch::{...}`).

`normalize_blockfetch_range_points`, `point_from_raw_header`, and
`fetch_range_blocks_multi_era_raw_decoded_excluding_lower` remain
imported in `sync.rs` because the residual Phase-37b verified pipeline
still calls them. **No cross-module promotions needed** — every item
chain_sync.rs reaches is already `pub`, `pub(super)`, or `pub(crate)`.

## Diff summary

| File | Before | After | Δ |
|---|---|---|---|
| `node/src/sync.rs` | 8,921 | 8,615 | −306 |
| `node/src/sync/chain_sync.rs` | 0 (new) | 361 | +361 |
| **Total** | 8,921 | 8,976 | +55 |

`+55` net is the module docstring + new `use` block.

## Verification gates

```
$ cargo fmt --all -- --check
(clean)

$ cargo check-all
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.42s

$ cargo lint
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 14.77s

$ cargo test-all | grep 'test result:' | awk ...
passing: 6224

$ python3 scripts/check-strict-mirror.py --fail-on-violation
strict-mirror: 0 violations (clean)
```

All four gates green. Test count unchanged (6,224 passing, 0 failing).
Strict-mirror drift-guard clean.

## Arc progress

| Round | Status | LOC carved | Residual `sync.rs` |
|---|---|---|---|
| R498 | ✅ shipped | 167 | 9,415 |
| R499 | ✅ shipped | 109 | 9,306 |
| R500 | ✅ shipped | 385 | 8,921 |
| R501 | ✅ shipped | 306 | 8,615 |
| R502 | pending | (target 188) | (target ~8,430) |
| … through R510 | pending | | (target ≤ 100) |

4/13 rounds shipped.

## Stop point — next round candidate

**R502 — KeepAlive + Phase 33 managed sync service.** Move
`keepalive_heartbeat`, `run_sync_service`, `SyncServiceConfig`, and
`SyncServiceOutcome` into `node/src/sync/service.rs`. Estimated 188
LOC. Awaits explicit `proceed`.

## References

- Plan: [`2026-05-12-round-498-plan-sync-rs-split-arc.md`](2026-05-12-round-498-plan-sync-rs-split-arc.md)
- Predecessor: [`R500`](2026-05-12-round-500-block-fetch-extraction.md)
- Upstream affinity:
  - `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/MiniProtocol/ChainSync/Client.hs`
  - `.reference-haskell-cardano-node/deps/ouroboros-network/ouroboros-network-protocols/src/Ouroboros/Network/Protocol/ChainSync/Client.hs`
