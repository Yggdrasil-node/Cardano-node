---
title: 'R482: ImmutableStore::iter_after streaming iterator'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-11-round-482-immutable-store-iter-after/
---

# R482 — `ImmutableStore::iter_after` streaming iterator

**Date:** 2026-05-11
**Predecessor:** [`R481 db-analyser HasAnalysis arc closeout`](2026-05-11-round-481-db-analyser-hasanalysis-arc-closure.md).
**Scope:** single-round bounded follow-on — add a streaming
counterpart to `ImmutableStore::suffix_after` that yields blocks
one at a time rather than materializing the full chain in a
`Vec<Block>`.

## Slice scope

The R475-R481 arc shipped `db-analyser`'s end-to-end dispatch
with `ImmutableStore::suffix_after(&Point::Origin)` returning a
fully-materialized `Vec<Block>`. For multi-terabyte forensic
chains (Cardano mainnet is ~100M blocks × ~50-100 KB each ≈
5-10 TB), holding the entire chain in memory is infeasible.

R482 adds a streaming counterpart:

```rust
fn iter_after<'a>(
    &'a self,
    point: &Point,
) -> Result<Box<dyn Iterator<Item = Block> + 'a>, StorageError>;
```

- **Default impl** delegates to `suffix_after().into_iter()` —
  existing implementations stay correct without override.
- **`InMemoryImmutable` override** yields cloned blocks via
  `self.blocks[start..].iter().cloned()` — no intermediate `Vec`.
- **`FileImmutable` override** walks `self.chain[start..]` and
  clones blocks from `self.index` on-demand — no intermediate
  `Vec`.
- **`db-analyser`'s `lib.rs::run`** now calls `iter_after` rather
  than `suffix_after`, so the runner consumes blocks lazily.

The point-not-found error semantics are preserved exactly:
backends return the error up-front (before the iterator yields
any items), matching the existing `suffix_after` contract.

## Design decision: `Box<dyn Iterator<Item = Block> + 'a>`

Trait methods that return iterators have two common shapes:
- `impl Iterator<...> + 'a` — supported by RPIT-in-trait, but
  constrains every implementor to the same concrete type.
- `Box<dyn Iterator<...> + 'a>` — allocations-per-call but
  flexible.

The Rust port chooses `Box<dyn>` because:
- Different `ImmutableStore` backends yield via different
  upstream iterators (`Vec::iter().cloned()` vs.
  `slice::iter().filter_map`); RPIT would require they all
  unify to one type.
- A one-time `Box` allocation per chain walk is negligible
  alongside the per-block clone work.
- The boxed iterator can be passed around / composed with other
  iterator adapters (`take`, `filter`, etc.) without further
  wrapping.

## Refactor: `resolve_suffix_start` helper

Both `InMemoryImmutable` and `FileImmutable` now share the same
3-case point-resolution logic:
1. `Point::Origin` → start at index 0.
2. `Point::BlockPoint(slot, hash)` outside the covered range →
   start at 0 (before range) or `chain.len()` (after range).
3. `Point::BlockPoint(slot, hash)` inside the range with a
   matching hash → start at `pos + 1`; with no matching hash →
   `Err(PointNotFound)`.

Extracted as a private `resolve_suffix_start(&self, point) ->
Result<usize, StorageError>` method on each impl. `suffix_after`
collects from `chain[start..]`; `iter_after` returns the
iterator over the same slice. The two methods are guaranteed to
yield byte-identical sequences (verified by the new
`*_iter_and_suffix_yield_same_sequence` tests).

## `db-analyser` wire-up

`crates/tools/db-analyser/src/lib.rs::run` now reads:

```rust
let store = FileImmutable::open(&config.db_dir).map_err(RunError::Storage)?;
let blocks = store.iter_after(&Point::Origin).map_err(RunError::Storage)?;
let outcome = analysis::runner::run_analysis(config, blocks).map_err(RunError::Analysis)?;
```

The `run_analysis` function already accepts `IntoIterator<Item =
Block>`, so the change is mechanical — the storage call changes
from `suffix_after` to `iter_after` and the result type changes
from `Vec<Block>` to `Box<dyn Iterator<Item = Block>>`. Both
satisfy `IntoIterator<Item = Block>`.

## Tests delivered (+10 cases)

`crates/storage/tests/integration.rs`:

| Test | Coverage |
|------|----------|
| `in_memory_iter_after_streams_full_chain_from_origin` | 3-block chain yielded via iter |
| `in_memory_iter_after_skips_to_point` | Skip past first block |
| `in_memory_iter_after_empty_when_past_tip` | Point past tip → empty iterator |
| `in_memory_iter_after_rejects_unknown_inside_range` | Unknown hash inside range → `PointNotFound` |
| `in_memory_iter_after_empty_store_yields_empty` | Empty store → empty iterator |
| `in_memory_iter_and_suffix_yield_same_sequence` | Equivalence with `suffix_after` |
| `file_immutable_iter_after_streams_chain` | 4-block chain via FileImmutable |
| `file_immutable_iter_after_skips_to_point` | Skip past mid-chain |
| `file_immutable_iter_after_persists_across_reopen` | Iter works after store reopen |
| `file_immutable_iter_and_suffix_yield_same_sequence` | Equivalence with `suffix_after` |

## Verification log

```
cargo fmt --all -- --check                                  clean
cargo check-all                                              clean
cargo lint                                                   clean
cargo test-all                                               6,166 → 6,176
python3 dev/test/check-strict-mirror.py --fail-on-violation   0 violations
python3 dev/test/check-parity-matrix.py                       clean
```

## Future work

The R482 implementation cleanly separates "what blocks to yield"
(the iterator) from "where the blocks come from" (the storage
backend). Two follow-on optimizations stay deferred:

- **On-disk-streaming `FileImmutable`**: current impl loads
  every block into `self.index: HashMap<HeaderHash, Block>` at
  open time, so memory pressure is just shifted from
  `suffix_after`'s temporary `Vec` to the persistent `index`.
  A revision that lazy-loads CBOR records from disk on-demand
  would close the multi-terabyte gap fully; gated on a
  chunked-log on-disk format design (separate arc).
- **Error-bearing iterator**: `iter_after` currently yields
  `Item = Block` and returns `Result<_, StorageError>` only
  up-front. An iterator that yields `Item = Result<Block,
  StorageError>` per-item would let an on-disk-streaming impl
  surface per-block decode errors. Deferred — current shape
  matches the in-memory implementations' contract.

## References

- Plan: this round is the first bullet under R481's "Future work"
  section ("Streaming chain iterator").
- Upstream: `Ouroboros.Consensus.Storage.ImmutableDB.API.Iterator`
  (the upstream Haskell shape db-analyser consumes).
- Yggdrasil callers: `crates/tools/db-analyser/src/lib.rs::run`
  (R481 → R482).
