---
title: "Round 808 dmq-node peer-sharing policy foundations"
parent: Reference
---

# Round 808 dmq-node peer-sharing policy foundations

Date: 2026-05-22

## Scope

Continues the dmq-node `run()` integration arc (Option A) — slice 3:
the self-contained peer-sharing policy foundations of
`Ouroboros.Network.PeerSharing`, which the `NodeKernel`'s
`PeerSharingAPI` builds on.

## What shipped

`crates/tools/dmq-node/src/peer_sharing.rs` — new file:

- `PeerSharingAmount` — the count of peers requested in / returned by
  one `PeerSharing` exchange, mirror of upstream
  `newtype PeerSharingAmount = PeerSharingAmount Word8`.
- `PS_POLICY_PEER_SHARE_STICKY_TIME` — the peer-pick salt-rotation
  interval (823 s), mirror of upstream
  `ps_POLICY_PEER_SHARE_STICKY_TIME`.
- `PS_POLICY_PEER_SHARE_MAX_PEERS` — the max peers per `PeerSharing`
  reply (10), mirror of upstream `ps_POLICY_PEER_SHARE_MAX_PEERS`.

dmq-node-local (R732 decision). `lib.rs` gains
`pub mod peer_sharing;`.

2 unit tests cover the `PeerSharingAmount` newtype and the policy
constants.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations (audit TSV rebuilt for the new file).
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 188 lib (+2 vs R806's 186) +
  2 golden, all green.

## Remaining (dmq-node run() integration — Option A)

`PeerSharingAPI` (using these constants), the `FetchClientRegistry`
sync infrastructure, the `NodeKernel` struct + `new_node_kernel`, the
`ntn_apps` / `ntc_apps` mux bundles, and the `run()` event loop.
