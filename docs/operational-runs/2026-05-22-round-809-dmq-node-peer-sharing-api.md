---
title: "Round 809 dmq-node PeerSharingAPI"
parent: Reference
---

# Round 809 dmq-node PeerSharingAPI

Date: 2026-05-22

## Scope

Continues the dmq-node `run()` integration arc (Option A) — slice 4:
`PublicPeerSelectionState` and the `PeerSharingAPI` record.

## What shipped

`crates/tools/dmq-node/src/peer_sharing.rs`:

- `PublicPeerSelectionState<PeerAddr>` — the set of peer addresses
  this node will share via the `PeerSharing` protocol, mirror of
  upstream `newtype PublicPeerSelectionState peeraddr`, with `empty`
  (mirror of `emptyPublicPeerSelectionState`).
- `PeerSharingAPI<PeerAddr>` — the peer-sharing API the `NodeKernel`
  holds, mirror of upstream `data PeerSharingAPI addr s m`: the
  shared public-state var, the peer-pick PRNG state (`u64` seed), the
  salt-rotation deadline, and the sticky-time / max-peers policy.
- `new_peer_sharing_api` — the constructor, mirror of upstream
  `newPeerSharingAPI`.

`PublicPeerSelectionState` is a trivial newtype not exported by
`crates/network`, so it is ported dmq-node-local here.

2 unit tests cover the empty public state and the `PeerSharingAPI`
constructor (policy carried, PRNG seeded, the state var shared).

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 190 lib (+2 vs R808's 188) +
  2 golden, all green.

## Remaining (dmq-node run() integration — Option A)

The `FetchClientRegistry` sync infrastructure, the `NodeKernel`
struct + `new_node_kernel`, the `ntn_apps` / `ntc_apps` mux bundles,
and the `run()` event loop.
