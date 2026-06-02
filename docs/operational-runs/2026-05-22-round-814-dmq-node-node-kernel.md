---
title: "Round 814 dmq-node NodeKernel"
parent: Reference
---

# Round 814 dmq-node NodeKernel

Date: 2026-05-22

## Scope

Continues the dmq-node `run()` integration arc (Option A) — slice 9:
the `NodeKernel` struct, composing the eight registry / state
components ported in R805-R813.

## What shipped

`crates/tools/dmq-node/src/diffusion.rs`:

- `NodeKernel<NtnAddr, ConnId>` — the DMQ node's shared runtime
  state, mirror of upstream `data NodeKernel crypto ntnAddr m`:
  the keepalive registry (`fetch_client_registry`), the peer-sharing
  registry + API, the signature `mempool`, the inbound-V2
  `sig_channel_var` / `sig_mempool_sem` / `sig_shared_tx_state_var`,
  the `stake_pools` monitoring state, and the `next_epoch_var`.
- `new_node_kernel` — the constructor, mirror of upstream
  `newNodeKernel`: empty registries, an empty mempool, a
  PRNG-seeded inbound-V2 state, and the default peer-share policy.

Generic over the node-to-node peer address and the connection-id
key; concrete over the DMQ `SigId` / `Sig`.

2 unit tests cover the empty-kernel construction (empty mempool /
channels, seeded PRNG) and the carried peer-share policy.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 201 lib (+2 vs R813's 199) +
  2 golden, all green.

## Remaining (dmq-node run() integration — Option A)

The `ntn_apps` / `ntc_apps` mux mini-protocol application bundles
(wiring the ported drivers and the `NodeKernel`), and the `run()`
event loop assembling the diffusion components.
