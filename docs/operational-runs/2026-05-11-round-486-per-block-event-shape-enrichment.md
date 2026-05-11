---
title: 'R486: per-block event-shape enrichment for CountTxOutputs + ShowBlockHeaderSize'
layout: default
parent: Operational runs
permalink: /operational-runs/2026-05-11-round-486-per-block-event-shape-enrichment/
---

# R486 — Per-block event-shape enrichment

**Date:** 2026-05-11
**Predecessor:** R485 (`CheckNoThunksEvery` permanent carve-out).
**Scope:** single-round bounded — align two shipped handlers'
per-block event shapes with upstream `Analysis.hs` traces.

## Slice scope

Audit of upstream's `Analysis.hs` event shapes surfaced two
divergences in the R479-R480 shipped handlers:

| Upstream event | Yggdrasil R479-R480 shape | R486 shape |
|----------------|---------------------------|------------|
| `CountTxOutputsEvent(blockNo, slot, cumulative, count)` | `(slot, count)` | `(slot, block_no, cumulative, count)` |
| `HeaderSizeEvent(blockNo, slot, headerSize, blockSize)` | `(slot, header_size)` | `(slot, block_no, header_size, block_size)` |

Both R486 changes preserve the existing aggregate fields (`total`
on `CountTxOutputs`; `max_size` on `ShowBlockHeaderSize`) so
callers that only consume the aggregates are unaffected.

## Implementation notes

- **`CountTxOutputs.per_block`**: emits the running `cumulative`
  total *after* applying each block. Tracking cumulative at the
  handler level (rather than in the stdout renderer) lets
  programmatic callers (tests, downstream tools) consume the
  per-block running totals directly without reproducing the
  reduction.

- **`ShowBlockHeaderSize.per_block`**: `block_size` is derived
  from `Block::raw_cbor.as_ref().map(|b| b.len()).unwrap_or(0)`.
  When `raw_cbor` is populated (production chain walk),
  `block_size` reflects the wire bytes. Programmatically
  constructed blocks emit 0 — matching upstream's behavior on
  the (rare) case where `GetBlockSize` has no source.

## `lib.rs::render_outcome` updates

The stdout-shape renderer now emits per-row:

- `CountTxOutputs`: `slot=N block_no=M cumulative_tx_outputs=C tx_outputs=K`
- `ShowBlockHeaderSize`: `slot=N block_no=M header_size=H block_size=B`

Aggregate-row formats (`total_tx_outputs=K`, `max_header_size=M`)
unchanged.

## Tests delivered (+3 cases)

| Test | Coverage |
|------|----------|
| `analysis_count_tx_outputs_emits_block_no_and_cumulative` | 3-block chain, asserts the 4-tuple shape + cumulative=0 across empty-tx blocks |
| `analysis_show_block_header_size_emits_block_no` | 2-block chain, asserts the 4-tuple shape (block_size=0 with no raw_cbor) |
| `analysis_show_block_header_size_emits_block_size_from_raw_cbor` | Populates `Block::raw_cbor` via `Arc<[u8]>` of length 1024; asserts `block_size=1024` in the emitted row |

Existing tests refactored to the new 4-tuple shape:

- `analysis_count_tx_outputs_empty_blocks_yields_zero`
- `analysis_show_block_header_size_tracks_max`
- `analysis_show_block_header_size_treats_missing_as_zero`

## Verification log

```
cargo fmt --all -- --check                                  clean
cargo check-all                                              clean
cargo lint                                                   clean
cargo test-all                                               6,178 → 6,181
python3 scripts/check-strict-mirror.py --fail-on-violation   0 violations
python3 scripts/check-parity-matrix.py                       clean
```

## Carve-outs surviving R486

- `ShowSlotBlockNo`, `CountBlocks`, `ShowBlockTxsSize`,
  `ShowEBBs`, `OnlyValidation` already match their upstream event
  shapes. `CountBlocks` emits a `first`/`last` enrichment beyond
  upstream's `CountedBlocksEvent(counted)`-only — this is
  Yggdrasil-side enrichment (not a divergence to remove).
- The full byte-equivalent stdout soak vs upstream binary remains
  a follow-on; this round closes the per-block event-shape gap.

## Stop point

The 4 R479 handlers + 3 R480 handlers now emit per-block events
whose field set matches upstream `Analysis.hs`. The remaining
gap to full byte-equivalent stdout is the *formatting* (e.g.
upstream uses tracer events serialized through Haskell's
`Show` instances; Yggdrasil emits `key=value` lines). A formal
soak round can pin the exact stdout string format.
