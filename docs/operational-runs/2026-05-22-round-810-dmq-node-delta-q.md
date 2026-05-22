---
title: "Round 810 dmq-node DeltaQ Distribution"
parent: Reference
---

# Round 810 dmq-node DeltaQ Distribution

Date: 2026-05-22

## Scope

Continues the dmq-node `run()` integration arc (Option A) — slice 5:
the `Distribution` leaf of `Ouroboros.Network.DeltaQ`, the foundation
the keepalive latency model (`GSV` / `PeerGSV`) builds on.

## What shipped

`crates/tools/dmq-node/src/delta_q.rs` — new file:

- `Distribution<N>` — an (improper) probability distribution, mirror
  of upstream `data Distribution n`. Like upstream, it currently
  covers only the degenerate case — a value taken with probability 1.
- `degenerate_distribution` — the constructor, mirror of upstream
  `degenerateDistribution`.
- `Distribution::convolve` — the convolution of two distributions
  (for degenerate distributions, the values add), mirror of upstream
  `convolveDistribution` / the `Semigroup` instance.

dmq-node-local (R732 decision). `lib.rs` gains `pub mod delta_q;`.

3 unit tests cover the constructor, convolution-adds, and the
zero-distribution convolution identity.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations (audit TSV rebuilt for the new file).
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 193 lib (+3 vs R809's 190) +
  2 golden, all green.

## Remaining (dmq-node run() integration — Option A)

The DeltaQ `GSV` triple and `PeerGSV` (building on `Distribution`),
the `KeepAliveRegistry`, the `FetchClientRegistry` sync infrastructure,
the `NodeKernel` struct, the `ntn_apps` / `ntc_apps` mux bundles, and
the `run()` event loop.
