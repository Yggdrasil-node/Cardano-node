---
title: "Round 815 dmq-node NtN mux bundle"
parent: Reference
---

# Round 815 dmq-node NtN mux bundle

Date: 2026-05-22

## Scope

Continues the dmq-node `run()` integration arc (Option A) — slice 10:
the node-to-node mux mini-protocol bundle.

## What shipped

`crates/tools/dmq-node/src/node_to_node.rs`:

- `dmq_ntn_bundle` — the DMQ node-to-node mux mini-protocol bundle
  (`OuroborosBundle`), mirror of the DMQ NtN protocol assignment:
  warm-tier `SigSubmission` (11) and `KeepAlive` (12), established-tier
  `PeerSharing` (13), no hot tier (dmq-node runs no block sync).
- `dmq_descriptor` — a private helper building a `MiniProtocolDescriptor`
  with the standard eager start and the default ingress-queue limit.

`crates/network`'s `MiniProtocolDescriptor` is pure data, so the
bundle is a clean pure-data slice; the descriptor → driver-task
conversion is part of the `run()` event loop.

1 unit test covers the tier assignment.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 202 lib (+1 vs R814's 201) +
  2 golden, all green.

## Remaining (dmq-node run() integration — Option A)

The `dmq_ntc_bundle` (node-to-client mux bundle), and the `run()`
event loop assembling the `crates/network` diffusion components with
the `NodeKernel` and the per-protocol driver runners.
