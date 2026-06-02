---
title: "Round 811 dmq-node DeltaQ GSV / PeerGSV"
parent: Reference
---

# Round 811 dmq-node DeltaQ GSV / PeerGSV

Date: 2026-05-22

## Scope

Continues the dmq-node `run()` integration arc (Option A) — slice 6:
the DeltaQ `Gsv` / `PeerGsv` latency model.

## What shipped

`crates/tools/dmq-node/src/delta_q.rs`:

- `Gsv` — the G/S/V latency model for one link direction, mirror of
  upstream `data GSV`: `G` minimum latency, `S` per-byte rate, `V`
  variance distribution. Upstream's `S` is a general
  `SizeInBytes -> DiffTime` function; every `GSV` is ballistic in
  practice, so the port models `S` as the linear per-byte rate.
- `Gsv::convolve` — per-component composition of two link segments,
  mirror of upstream `instance Semigroup GSV`.
- `ballistic_gsv` — the linear-model constructor (`ballisticGSV`).
- `PeerGsv` — the measured GSV for a peer, both directions, mirror of
  upstream `data PeerGSV`.
- `default_gsv` — the pre-measurement default (`G` 500 ms, `S` 2 µs/
  byte, `V` degenerate-zero), mirror of upstream `defaultGSV`.

Upstream's `GSV` / `PeerGSV` are spelled `Gsv` / `PeerGsv` — Rust's
`upper_case_acronyms` lint requires the mixed-case form.

3 unit tests cover the `default_gsv` constants and `Gsv` convolution.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 195 lib (+2 vs R810's 193) +
  2 golden, all green.

## Remaining (dmq-node run() integration — Option A)

The `KeepAliveRegistry` (holding `PeerGsv` per peer), the
`FetchClientRegistry` sync infrastructure, the `NodeKernel` struct,
the `ntn_apps` / `ntc_apps` mux bundles, and the `run()` event loop.
