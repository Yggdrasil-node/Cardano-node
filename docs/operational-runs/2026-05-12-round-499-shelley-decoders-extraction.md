---
title: 'R499: extract Shelley decoders into node/src/sync/shelley_decoders.rs (sync.rs R-arc round 2/13)'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-12-round-499-shelley-decoders-extraction/
---

# R499 — Shelley decoders extraction (sync.rs R-arc, slice 2/13)

**Date:** 2026-05-12
**Predecessor:** [`R498`](2026-05-12-round-498-sync-error-extraction.md).
**Slice scope:** single round — extract Shelley-era decoders and the
`compute_tx_id` helper from `node/src/sync.rs` into
`node/src/sync/shelley_decoders.rs`. Behavior-preserving. No public-API
change.

## Slice scope

- 1 private helper (`compute_tx_id`, promoted to `pub(super)`).
- 5 public decoder functions:
  - `shelley_block_to_block`
  - `shelley_block_to_block_with_spans`
  - `decode_shelley_blocks`
  - `decode_shelley_header`
  - `decode_point`
- 114 LOC moved from sync.rs.

`apply_raw_header_hash_override` (still in `sync.rs`, Phase 35 territory
slated for R504) was promoted to `pub(super)` so the new leaf can
reach it via `use super::apply_raw_header_hash_override;`. **2
cross-module promotions total** — well within the skill's "1–6 promote
inline" budget.

## Mirror mapping

| Yggdrasil leaf | Upstream Haskell affinity |
|---|---|
| `node/src/sync/shelley_decoders.rs` | `Ouroboros.Consensus.Shelley.Ledger.Block` — `decodeShelleyBlock`, `decodeShelleyHeader`. Plus byte-span preserving conversion that is byte-exact-fee-correct per `Cardano.Ledger.Shelley.Tx.minfee`. |

The new leaf carries an explicit `## Naming parity` stanza ending
`**Strict mirror:** none.` per AGENTS.md (Yggdrasil's adapter layer
between the upstream Shelley typed surface and the storage `Block`
wrapper is a synthesis — upstream splits the same concerns across
multiple files).

## Cross-module dependencies

- `compute_tx_id` is used at 4 call sites still resident in `sync.rs`
  (Phase 35 multi-era decoders + Phase 40 mempool eviction).
  `pub(super)` lets those keep working via `use shelley_decoders::compute_tx_id;`
  in `sync.rs`'s preamble.
- `apply_raw_header_hash_override` (sync.rs:3870) used by
  `shelley_block_to_block` — promoted to `pub(super)`, accessed via
  `use super::apply_raw_header_hash_override;` in the new leaf.
- Public surface preserved through `pub use shelley_decoders::{...};`
  in `sync.rs`. `lib.rs` unchanged.

## Diff summary

| File | Before | After | Δ |
|---|---|---|---|
| `node/src/sync.rs` | 9,415 | 9,306 | −109 |
| `node/src/sync/shelley_decoders.rs` | 0 (new) | 149 | +149 |
| **Total** | 9,415 | 9,455 | +40 |

`+40` net is the module docstring + new `use` block. Body of moved
functions copied verbatim (one fmt cosmetic: `shelley_block_to_block_with_spans`
collapsed to single-line signature after the import simplification).

## Verification gates

```
$ cargo fmt --all -- --check
(clean)

$ cargo check-all
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.74s

$ cargo lint
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.74s

$ cargo test-all | grep 'test result:' | awk ...
passing: 6224

$ python3 dev/test/check-strict-mirror.py --fail-on-violation
strict-mirror: 0 violations (clean)
```

All four gates green. Test count unchanged from R498 (6,224 passing,
0 failing). Strict-mirror drift-guard clean.

## Arc progress

| Round | Status | LOC carved | Residual `sync.rs` |
|---|---|---|---|
| R498 | ✅ shipped | 167 | 9,415 |
| R499 | ✅ shipped | 109 | 9,306 |
| R500 | pending | (target 400) | (target ~8,900) |
| … through R510 | pending | | (target ≤ 100) |

2/13 rounds shipped.

## Stop point — next round candidate

**R500 — BlockFetch fetch primitives.** Move `map_blockfetch_error`,
`fetch_range_blocks*` (raw / typed / multi-era / decoded variants),
`normalize_blockfetch_range_*`, `point_from_raw_header`, and
`point_bytes_from_raw_header_or_tip` into
`node/src/sync/block_fetch.rs`. Estimated 400 LOC. Awaits explicit
`proceed`.

## References

- Plan: [`2026-05-12-round-498-plan-sync-rs-split-arc.md`](2026-05-12-round-498-plan-sync-rs-split-arc.md)
- Predecessor: [`R498`](2026-05-12-round-498-sync-error-extraction.md)
- Skill: `docs/AGENTS.md`
- Upstream affinity:
  - `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-shelley/src/Ouroboros/Consensus/Shelley/Ledger/Block.hs`
  - `.reference-haskell-cardano-node/deps/cardano-ledger/eras/shelley/impl/src/Cardano/Ledger/Shelley/Tx.hs` (minfee byte-preservation rationale)
