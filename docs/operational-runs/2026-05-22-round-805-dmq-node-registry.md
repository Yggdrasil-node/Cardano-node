---
title: "Round 805 dmq-node inbound-V2 channel registry"
parent: Reference
---

# Round 805 dmq-node inbound-V2 channel registry

Date: 2026-05-22

## Scope

Opens the dmq-node `run()` integration arc (Option A, per the
`COMPLETION_ROADMAP.md` A4 dmq-node entry / the R804 parity-plan) —
slice 1: the `Inbound/V2/Registry.hs` channel registry.

## What shipped

`crates/tools/dmq-node/src/registry.rs` — new file, port of the
registry types of upstream
`Ouroboros.Network.TxSubmission.Inbound.V2.Registry`:

- `TxDecisionChannel` — a one-slot mailbox carrying a `TxDecision`
  from the inbound-V2 governor to one peer's `SigSubmission` client
  (mirror of upstream's per-peer `StrictMVar m (TxDecision ...)`).
- `TxChannels<PeerAddr>` — the per-peer decision-channel registry,
  with `register` / `channel` / `unregister`, mirror of upstream
  `newtype TxChannels`.
- `TxChannelsVar` / `new_tx_channels_var` — the registry behind a
  shared lock (the `NodeKernel`'s `sigChannelVar`).
- `TxMempoolSem` / `new` / `acquire` — the mempool-access semaphore
  (one permit), mirror of upstream `newtype TxMempoolSem` /
  `newTxMempoolSem`.

dmq-node-local (R732 decision). `lib.rs` gains `pub mod registry;`.

3 unit tests cover register/lookup/unregister, the empty
`TxChannelsVar`, and the semaphore's exclusive acquire/release.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations (audit TSV rebuilt for the new file).
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 184 lib (+3 vs R801's 181) +
  2 golden, all green.

## Remaining (dmq-node run() integration — Option A)

The `NodeKernel` struct + `new_node_kernel`, the `ntn_apps` /
`ntc_apps` mux bundles, the `diffusion_arguments` /
`diffusion_applications` wiring, then the `run()` event loop.
