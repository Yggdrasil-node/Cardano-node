---
title: 'R498: extract SyncError into node/src/sync/error.rs (sync.rs R-arc round 1/13)'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-12-round-498-sync-error-extraction/
---

# R498 — `SyncError` extraction (sync.rs R-arc, slice 1/13)

**Date:** 2026-05-12
**Predecessor:** [`R498-R510 plan`](2026-05-12-round-498-plan-sync-rs-split-arc.md).
**Slice scope:** single round — extract `SyncError` enum + `impl SyncError` from `node/src/sync.rs` into `node/src/sync/error.rs`. Behavior-preserving. No public-API change.

## Slice scope

- 1 enum (`SyncError`, 13 variants).
- 1 impl block (`SyncError::is_peer_attributable`).
- 167 LOC moved (lines 69–235 of pre-R498 `sync.rs`).
- 3 unused imports trimmed from residual `sync.rs` (`PeerError`,
  `ChainSyncClientError`, `KeepAliveClientError` — those types only
  appeared inside `SyncError` variant declarations).

`DIJKSTRA_MAJOR_PROTOCOL_VERSION` remained in `sync.rs`; it is
consumed by the verified-pipeline slice (Phase 37b) and will move
with R508.

## Mirror mapping

| Yggdrasil leaf | Upstream Haskell affinity |
|---|---|
| `node/src/sync/error.rs` | none (synthesis). Wraps protocol-client errors from `Ouroboros.Network.{ChainSync,BlockFetch,KeepAlive}.Client` plus `Ouroboros.Consensus` validation errors. `is_peer_attributable` mirrors `Ouroboros.Consensus.Storage.ChainDB.API.Types.InvalidBlockPunishment` peer-attribution semantics. |

`error.rs` carries an explicit `## Naming parity` stanza ending
`**Strict mirror:** none.` per AGENTS.md.

## Cross-module dependencies

`SyncError` was already publicly re-exported via `node/src/lib.rs`'s
`pub use sync::{... SyncError ...}` block. R498 preserves that
surface by adding `pub use error::SyncError;` inside `sync.rs`. **No
`lib.rs` edit needed.** No external caller observes the
re-organization.

Internal callers (`node/src/blockfetch_worker.rs`, `node/src/runtime.rs`,
etc.) reference `crate::sync::SyncError`, which resolves through the
same re-export. Verified clean by `cargo check-all`.

## Diff summary

| File | Before | After | Δ |
|---|---|---|---|
| `node/src/sync.rs` | 9,579 | 9,415 | −164 |
| `node/src/sync/error.rs` | 0 (new) | 195 | +195 |
| **Total** | 9,579 | 9,610 | +31 |

`+31` net is the module-header docstring on the new file. No
production logic added.

## Verification gates

```
$ cargo fmt --all -- --check
(clean)

$ cargo check-all
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.45s

$ cargo lint
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 18.11s

$ cargo test-all | grep 'test result:' | awk ...
passing: 6224

$ python3 dev/test/check-strict-mirror.py --fail-on-violation
strict-mirror: 0 violations (clean)
```

All four gates green. Strict-mirror drift-guard clean.

## Arc progress

| Round | Status | LOC carved | Residual `sync.rs` |
|---|---|---|---|
| R498 | ✅ shipped | 167 | 9,415 |
| R499 | pending | (target 240) | (target 9,175) |
| R500 | pending | (target 400) | (target 8,775) |
| … through R510 | pending | (target ~7,000 cum.) | (target ≤ 100) |

13/13 rounds remaining; 1/13 shipped.

## Stop point — next round candidate

**R499 — Shelley decoders.** Move
`compute_tx_id`, `shelley_block_to_block(+_with_spans)`,
`decode_shelley_blocks`, `decode_shelley_header`, and `decode_point`
into `node/src/sync/shelley_decoders.rs`. Estimated 240 LOC.
Awaits explicit `proceed` per the continuous-agent-loop pattern.

## References

- Plan: [`2026-05-12-round-498-plan-sync-rs-split-arc.md`](2026-05-12-round-498-plan-sync-rs-split-arc.md)
- Predecessor: [`R497`](2026-05-11-round-497-to-raw-tx-bytes-fidelity.md) (db-analyser HasAnalysis arc closure)
- Skill: `docs/AGENTS.md`
- Upstream affinity:
  - `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Storage/ChainDB/API/Types/InvalidBlockPunishment.hs`
  - `.reference-haskell-cardano-node/deps/ouroboros-network/ouroboros-network/src/Ouroboros/Network/Protocol/ChainSync/Client.hs`
