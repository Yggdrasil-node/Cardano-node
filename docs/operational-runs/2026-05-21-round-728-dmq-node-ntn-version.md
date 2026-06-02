---
title: "Round 728 dmq-node NodeToNodeVersion (dmq-node arc, slice 12)"
parent: Reference
---

# Round 728 dmq-node NodeToNodeVersion (dmq-node arc, slice 12)

Date: 2026-05-21

## Scope

Slice 12 of the dmq-node arc — opens the `node_to_node` module with
the protocol-version type.

## What shipped

- `crates/tools/dmq-node/src/node_to_node.rs` — module-tree parent
  for the upstream `DMQ/NodeToNode/` directory.
- `crates/tools/dmq-node/src/node_to_node/version.rs` — strict mirror
  of `DMQ/NodeToNode/Version.hs`. Ports `NodeToNodeVersion` (the
  `V1`/`V2` enum), its integer-tag mapping, the CBOR-term codec
  (`encode`/`decode`, mirror of `nodeToNodeVersionCodec`), and the
  JSON rendering (`to_json`, mirror of `instance ToJSON`).
- `lib.rs` gains `pub mod node_to_node;`.

`NodeToNodeVersionData` plus the `Acceptable` / `Queryable`
version-negotiation instances depend on the `ouroboros-network-api`
`NetworkMagic` / `DiffusionMode` / `PeerSharing` types and land with
the diffusion sub-arc.

5 unit tests: int-tag round-trip + unknown-tag `None`, `ALL`
ordering, CBOR codec round-trip, unknown-tag decode rejection, and
the JSON tag.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations (audit TSV rebuilt for the 2 new files).
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 75 lib (+5 vs R727's 70) +
  2 golden, all green.

## Remaining (dmq-node arc)

- `NodeToNodeVersionData` + version negotiation; `NodeToClient`
  version + protocols.
- The `SigSubmission` `codecSigSubmission` TxSubmission2 wrapper +
  protocol-limit tables; the rest of `validateSig`.
- `Diffusion/*` wiring (the `DiffusionWiringDeferred` carve-out).
