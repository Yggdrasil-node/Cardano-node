---
title: 'R500: extract BlockFetch primitives into node/src/sync/block_fetch.rs (sync.rs R-arc round 3/13)'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-12-round-500-block-fetch-extraction/
---

# R500 — BlockFetch primitives extraction (sync.rs R-arc, slice 3/13)

**Date:** 2026-05-12
**Predecessor:** [`R499`](2026-05-12-round-499-shelley-decoders-extraction.md).
**Slice scope:** single round — extract BlockFetch range fetchers,
range normalization helpers, and ChainSync header-point extraction
from `node/src/sync.rs` into `node/src/sync/block_fetch.rs`.
Behavior-preserving.

## Slice scope

- 9 functions moved (~390 LOC):
  - `map_blockfetch_error`
  - `fetch_range_blocks`
  - `fetch_range_blocks_typed`
  - `fetch_range_blocks_multi_era_raw_decoded` (`pub(crate)`)
  - `fetch_range_blocks_multi_era_raw_decoded_excluding_lower`
  - `fetch_range_blocks_decoded`
  - `normalize_blockfetch_range_points`
  - `normalize_blockfetch_range_bytes`
  - `point_from_raw_header` (with 7 nested closures)
  - `point_bytes_from_raw_header_or_tip`

Public surface preserved: `fetch_range_blocks_multi_era_raw_decoded`
remains `pub(crate)` (consumed by `blockfetch_worker.rs` via
`crate::sync::fetch_range_blocks_multi_era_raw_decoded`). All others
become `pub(super)` so `sync.rs` can re-import via private `use
block_fetch::{...};` for residual call sites.

## Mirror mapping

| Yggdrasil leaf | Upstream Haskell affinity |
|---|---|
| `node/src/sync/block_fetch.rs` | `Ouroboros.Network.BlockFetch.Client` + `Ouroboros.Consensus.MiniProtocol.BlockFetch.Client`. The Origin-lower-bound normalization (`[Origin, upper] → [upper, upper]`) mirrors upstream `BlockFetch.Server`'s inability to resolve `Point::Origin`. `point_from_raw_header` encodes the R211 lesson: Byron EBB hash prefix `0x82 0x00`, main `0x82 0x01`. |

The new leaf carries an explicit `## Naming parity` stanza ending
`**Strict mirror:** none.` — Yggdrasil's adapter layer between the
mini-protocol client and multiple ChainSync envelope flavors is a
synthesis.

## Cross-module dependencies

Inbound (block_fetch.rs reaches into sync.rs):

- `decode_multi_era_block_ledger` — Phase-35 helper (R504 target);
  promoted to `pub(super)`.
- `drop_raw_range_lower_boundary` — same situation; promoted to
  `pub(super)`.
- `MultiEraBlock` — already `pub`; accessed via `super::MultiEraBlock`.

Outbound (sync.rs re-imports block_fetch.rs items):

- `fetch_range_blocks`, `fetch_range_blocks_decoded`,
  `fetch_range_blocks_typed`,
  `fetch_range_blocks_multi_era_raw_decoded_excluding_lower`,
  `normalize_blockfetch_range_bytes`,
  `normalize_blockfetch_range_points`,
  `point_bytes_from_raw_header_or_tip`,
  `point_from_raw_header` — 8 private `pub(super)` re-imports.
- `fetch_range_blocks_multi_era_raw_decoded` — `pub(crate)` re-export.

`map_blockfetch_error` is used only inside block_fetch.rs (by
`fetch_range_blocks_typed` and `fetch_range_blocks_decoded`); kept
`pub(super)` for symmetry but not imported into the parent.

2 inbound promotions — within the skill's "1–6 promote inline"
budget. No public-API change (`lib.rs` unchanged).

`ChainRange` import removed from `sync.rs` (no longer used there).

## Diff summary

| File | Before | After | Δ |
|---|---|---|---|
| `node/src/sync.rs` | 9,306 | 8,921 | −385 |
| `node/src/sync/block_fetch.rs` | 0 (new) | 449 | +449 |
| **Total** | 9,306 | 9,370 | +64 |

`+64` net is the module docstring + new `use` block.

## Verification gates

```
$ cargo fmt --all -- --check
(clean)

$ cargo check-all
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.79s

$ cargo lint
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.27s

$ cargo test-all | grep 'test result:' | awk ...
passing: 6224

$ python3 dev/test/check-strict-mirror.py --fail-on-violation
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
| R501 | pending | (target 322) | (target ~8,600) |
| … through R510 | pending | | (target ≤ 100) |

3/13 rounds shipped.

## Stop point — next round candidate

**R501 — typed ChainSync API.** Move `sync_step*`, `sync_steps*`,
`sync_until_typed`, `apply_typed_step_to_volatile`,
`apply_typed_progress_to_volatile`, `typed_find_intersect`, and
`sync_batch_apply` into `node/src/sync/chain_sync.rs`. Estimated
322 LOC. Awaits explicit `proceed`.

## References

- Plan: [`2026-05-12-round-498-plan-sync-rs-split-arc.md`](2026-05-12-round-498-plan-sync-rs-split-arc.md)
- Predecessor: [`R499`](2026-05-12-round-499-shelley-decoders-extraction.md)
- Upstream affinity:
  - `.reference-haskell-cardano-node/deps/ouroboros-network/ouroboros-network/src/Ouroboros/Network/BlockFetch/Client.hs`
  - `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/MiniProtocol/BlockFetch/Client.hs`
