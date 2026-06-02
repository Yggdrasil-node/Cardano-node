---
title: "Round 813 dmq-node PeerSharingRegistry"
parent: Reference
---

# Round 813 dmq-node PeerSharingRegistry

Date: 2026-05-22

## Scope

Continues the dmq-node `run()` integration arc (Option A) — slice 8:
the `PeerSharingRegistry` / `PeerSharingController`, the last
unported `NodeKernel` field type.

## What shipped

`crates/tools/dmq-node/src/peer_sharing.rs`:

- `PeerSharingRequest<Peer>` — a pending peer-sharing request (an
  amount plus a result slot), modelling upstream's
  `(PeerSharingAmount, MVar [peer])` controller payload.
- `PeerSharingController<Peer>` — a depth-1 request mailbox for one
  peer's peer-sharing exchange, mirror of upstream `newtype
  PeerSharingController peer m` (the `StrictTMVar` mailbox modelled
  as a `Mutex`-guarded optional slot).
- `PeerSharingRegistry<Peer>` — the per-peer controller registry,
  mirror of upstream `newtype PeerSharingRegistry peer m`, with
  `new_peer_sharing_registry` (`newPeerSharingRegistry`).

This also resolved the `FetchClientRegistry` knot: `BlockFetch.
ClientRegistry` re-exports `module KeepAlive`, and dmq-node uses the
`NodeKernel`'s registry field exclusively via `bracketKeepAliveClient`
— so dmq-node's `fetch_client_registry` is functionally the R812
`KeepAliveRegistry`, and no separate degenerate `FetchClientRegistry`
struct port is needed.

2 unit tests cover the registry registration and the controller
request mailbox.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 199 lib (+4 vs R812's 197) +
  2 golden, all green.

## Remaining (dmq-node run() integration — Option A)

All `NodeKernel` field types are now ported. Next: the `NodeKernel`
struct + `new_node_kernel`, then the `ntn_apps` / `ntc_apps` mux
bundles, and the `run()` event loop.
