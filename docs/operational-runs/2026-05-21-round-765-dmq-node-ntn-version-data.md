---
title: "Round 765 dmq-node NodeToNodeVersionData + negotiation (dmq-node runtime sub-arc, slice 7)"
parent: Reference
---

# Round 765 dmq-node NodeToNodeVersionData + negotiation (dmq-node runtime sub-arc, slice 7)

Date: 2026-05-21

## Scope

Slice 7 of the dmq-node runtime/diffusion sub-arc — the NtN
version-data type and the handshake version-negotiation logic.

## What shipped

`crates/tools/dmq-node/src/node_to_node/version.rs`:

- `DiffusionMode` (`InitiatorOnly` / `InitiatorAndResponder`, `Ord`)
  and `PeerSharing` (`Disabled` / `Enabled`) — the supporting types.
- `NodeToNodeVersionData` — `network_magic` / `diffusion_mode` /
  `peer_sharing` / `query`, mirror of upstream
  `data NodeToNodeVersionData` (`NodeToNode/Version.hs`). Reuses the
  crate's existing `types::NetworkMagic`.
- `NodeToNodeVersionData::accept` — mirror of upstream
  `instance Acceptable NodeToNodeVersionData`: the network magic must
  match (a mismatch refuses); the accepted diffusion mode is the more
  restrictive (`min`); peer sharing is agreed only when the accepted
  mode is `InitiatorAndResponder` and both peers enabled it; `query`
  is the OR of the two.

4 unit tests cover the negotiation: magic-mismatch refusal,
diffusion-mode `min`, the peer-sharing both-enabled rule, and the
`query` OR.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 145 lib (+4 vs R764's 141) +
  2 golden, all green.

## Remaining (dmq-node runtime sub-arc)

- The `NodeToNodeVersionData` / `NodeToClientVersionData` CBOR-term
  codecs; the NtN / NtC mux bundles; `Diffusion/*`; `NodeKernel`;
  `tracer.rs`; the `run()` loop.
