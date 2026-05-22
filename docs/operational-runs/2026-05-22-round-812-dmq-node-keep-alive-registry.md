---
title: "Round 812 dmq-node KeepAliveRegistry"
parent: Reference
---

# Round 812 dmq-node KeepAliveRegistry

Date: 2026-05-22

## Scope

Continues the dmq-node `run()` integration arc (Option A) — slice 7:
the `KeepAliveRegistry` from `Ouroboros.Network.KeepAlive.Registry`.

## What shipped

`crates/tools/dmq-node/src/keep_alive.rs` — new file:

- `KeepAliveRegistry<Peer>` — the registry of per-peer keepalive
  state, mirror of upstream `data KeepAliveRegistry peer m`: the
  `dq_registry` of `PeerGsv` latency measurements, the `keep_registry`
  of block-fetch teardown handles, and the `dying_registry` of
  peers being torn down.
- `new_keep_alive_registry` — the constructor (`newKeepAliveRegistry`).
- `KeepAliveRegistry::read_peer_gsvs` — the `PeerGsv`s of the
  currently-hot peers (the `dq_registry`/`keep_registry`
  intersection), mirror of upstream `readPeerGSVs`.

Upstream's `keep_registry` value is `(ThreadId, TMVar ())` — the
fetch client's cancellation target and exit signal. dmq-node runs no
block-fetch clients, so that registry is never populated; its value
is modelled as the unit type, faithful to dmq-node's degenerate
`FetchClientRegistry () ()` instantiation.

dmq-node-local (R732 decision). `lib.rs` gains `pub mod keep_alive;`.

2 unit tests cover the empty registry and the `read_peer_gsvs`
intersection semantics.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations (audit TSV rebuilt for the new file).
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 197 lib (+2 vs R811's 195) +
  2 golden, all green.

## Remaining (dmq-node run() integration — Option A)

The `FetchClientRegistry` (composing the keepalive registry and the
sync brackets), the `NodeKernel` struct, the `ntn_apps` / `ntc_apps`
mux bundles, and the `run()` event loop.
