---
title: "Round 806 dmq-node StakePools monitoring record"
parent: Reference
---

# Round 806 dmq-node StakePools monitoring record

Date: 2026-05-22

## Scope

Continues the dmq-node `run()` integration arc (Option A) — slice 2:
the `StakePools` stake-pool monitoring record from
`Diffusion/NodeKernel/Types.hs`.

## What shipped

`crates/tools/dmq-node/src/diffusion.rs`:

- `StakePools` — the stake-pool monitoring state the DMQ `NodeKernel`
  holds, mirror of upstream `data StakePools m`: the `stake_pools_var`
  per-pool stake-snapshot map (populated via the local-state-query
  client), and the `ledger_big_peers_var` / `ledger_peers_var` ledger-
  peer snapshots, with a `new` constructor.
- It reuses `yggdrasil_network::LedgerPeerSnapshot` for the
  ledger-peer fields rather than carrying a dmq-node-local copy.
- The upstream `withPoolValidationCtx` field is a rank-2 polymorphic
  closure — Rust cannot carry that as a struct field, so it is
  modelled as a method landing with the `NodeKernel` assembly (it
  also needs the kernel-level next-epoch / ocert-counter state).

2 unit tests cover the empty `new` state and recording a pool
snapshot.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 186 lib (+2 vs R805's 184) +
  2 golden, all green.

## Remaining (dmq-node run() integration — Option A)

The `NodeKernel` struct + `new_node_kernel` (composing the registry,
`MempoolSeq`, `SharedTxState`, `StakePools`, and the next-epoch /
ocert-counter vars, with the `with_pool_validation_ctx` method), the
`ntn_apps` / `ntc_apps` mux bundles, the `diffusion_arguments` /
`diffusion_applications` wiring, then the `run()` event loop.
