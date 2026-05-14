# Round 283 — `era_tag` wiring + new `lsq_era_index` constants

**Date:** 2026-05-09
**Phase:** C (tech-debt purge)
**Predecessor:** R282 (`docs/operational-runs/2026-05-09-round-282-block-producer-serde-field.md`)
**Plan:** `~/.claude/plans/playful-tickling-plum.md`

## Scope

1. Drop the stale `#[allow(dead_code)]` on `node/src/sync.rs::mod era_tag`.
   Investigation showed the constants ARE used (all 8 era tags
   referenced by the multi-era envelope decoder match arms at lines
   3749, 3763, 3771, 3787, 3802, 3987+). The allow was leftover from
   when the constants were authored ahead of the decoder wiring.

2. Eliminate magic numbers in `node/src/local_server.rs::dispatch_upstream_query`
   `GetCurrentPParams` era dispatch by introducing a new named-constant
   module `lsq_era_index`. The LSQ-protocol era ordinal is distinct from
   the wire-format `era_tag` (Byron is one ordinal here vs two on the
   wire), so a separate constant module is correct. The new module
   matches upstream `Ouroboros.Consensus.Cardano.Block::CardanoEras`
   indexing.

## Investigation

### sync.rs `era_tag` allow

`grep "era_tag::" node/src/sync.rs` returned 9 hits — all 8 named
constants (BYRON_EBB, BYRON_MAIN, SHELLEY, ALLEGRA, MARY, ALONZO,
BABBAGE, CONWAY) are used by the multi-era block decoder match arms.
The `#[allow(dead_code)]` on the module declaration is stale.

### local_server.rs era magic numbers

The `GetCurrentPParams` dispatch at `node/src/local_server.rs:725-746`
uses bare integer match arms (`1..=3 => encode_shelley_pparams_for_lsq`,
`4 => encode_alonzo_pparams_for_lsq`, etc.) operating on the LSQ
protocol's `era_index` field. The mapping (Shelley=1, Allegra=2,
Mary=3, Alonzo=4, Babbage=5, Conway=6) is documented in
`effective_era_index_for_lsq`'s docstring but isn't expressed as
named constants.

The LSQ ordinal collapses Byron into a single value (0) while the
on-wire `era_tag` in sync.rs splits Byron into BYRON_EBB (0) and
BYRON_MAIN (1). The two numbering schemes are incompatible — sync.rs's
SHELLEY=2 vs local_server.rs's SHELLEY=1. So reusing `era_tag::*` is
incorrect; a separate `lsq_era_index::*` constant module is the right
fix.

## Resolution

### sync.rs

Removed the stale `#[allow(dead_code)]` on the `mod era_tag`
declaration. No other changes needed; the constants stay used.

### local_server.rs

Added a new constant module `lsq_era_index` near the file head
(alongside `mod sessions` / `mod accept`):

```rust
/// LSQ-protocol era ordinal used by `QueryHardFork::GetCurrentEra` and
/// the `[era_index, era_specific_query]` envelope.
///
/// This is distinct from the on-wire `era_tag` in `node/src/sync.rs`
/// (which numbers Byron-EBB and Byron-Main separately) because the LSQ
/// protocol collapses Byron into a single ordinal. The mapping here
/// matches upstream `Ouroboros.Consensus.Cardano.Block::CardanoEras`
/// indexing.
mod lsq_era_index {
    pub const BYRON: u32 = 0;
    pub const SHELLEY: u32 = 1;
    pub const ALLEGRA: u32 = 2;
    pub const MARY: u32 = 3;
    pub const ALONZO: u32 = 4;
    pub const BABBAGE: u32 = 5;
    pub const CONWAY: u32 = 6;
}
```

Updated `effective_era_index_for_lsq` to use the constants in its
PV-major→era mapping. Updated the `GetCurrentPParams` dispatch to use
`if i == lsq_era_index::SHELLEY || i == lsq_era_index::ALLEGRA ||
i == lsq_era_index::MARY` for the Shelley-PP-shape arm and named-
constant comparisons for Alonzo / Babbage / Conway arms.

The match arms now read like English: "if era index equals Shelley /
Allegra / Mary, encode with the Shelley PP shape" instead of `1..=3 =>`.

## Production `#[allow(dead_code)]` site count

| Site | Pre-R283 | Post-R283 | Round |
|---|---|---|---|
| `block_producer.rs::TextEnvelope::description` | 0 | 0 | R282 ✅ |
| `sync.rs::mod era_tag` | 1 | 0 | R283 ✅ |
| `reconnecting.rs::_runstate_impl_marker` | 1 | 1 | R286 |
| `peer_management.rs` × 5 (Phase 6 scaffolding) | 5 | 5 | R285 |
| `shelley.rs::mk_txout` test helper | 1 | 1 | R286 |
| **TOTAL production** | 8 | 7 | |

## Verification gates

```text
cargo fmt --all -- --check          clean
cargo check-all                     clean (Finished `dev` profile in 6.30s)
cargo lint                          clean (Finished `dev` profile in 10.59s)
cargo test-all                      4855 passed; 0 failed (baseline preserved)
```

## Diff stat

```text
node/src/sync.rs                  -1 line  (removed `#[allow(dead_code)]`)
node/src/local_server.rs         +27 lines (lsq_era_index module + match
                                            updates in
                                            effective_era_index_for_lsq +
                                            GetCurrentPParams dispatch)
docs/operational-runs/2026-05-09-round-283-... (new)
```

## Stop point — Phase C in progress

| Round | Site | Status |
|---|---|---|
| R282 | `block_producer.rs::description` | ✅ closed |
| **R283** | `sync.rs::mod era_tag` + `local_server.rs` LSQ era index | ✅ closed |
| R284 | `local_server.rs:713` LSQ TODO | next |
| R285 | `peer_management.rs` Phase 6 wiring | pending |
| R286 | `reconnecting.rs` marker + `shelley.rs` test helper | pending |
| R287 | `code-audit.md` + `REFACTOR_BLUEPRINT.md` re-grade | pending |

## References

- Plan: `~/.claude/plans/playful-tickling-plum.md`
- Predecessor: R282 (`docs/operational-runs/2026-05-09-round-282-block-producer-serde-field.md`)
- Upstream `CardanoEras` indexing:
  `.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-cardano/src/Ouroboros/Consensus/Cardano/Block.hs`
