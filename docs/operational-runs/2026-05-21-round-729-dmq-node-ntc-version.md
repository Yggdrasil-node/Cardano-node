---
title: "Round 729 dmq-node NodeToClientVersion (dmq-node arc, slice 13)"
parent: Reference
---

# Round 729 dmq-node NodeToClientVersion (dmq-node arc, slice 13)

Date: 2026-05-21

## Scope

Slice 13 of the dmq-node arc — opens the `node_to_client` module
with the protocol-version type.

## What shipped

- `crates/tools/dmq-node/src/node_to_client.rs` — module-tree parent
  for the upstream `DMQ/NodeToClient/` directory.
- `crates/tools/dmq-node/src/node_to_client/version.rs` — strict
  mirror of `DMQ/NodeToClient/Version.hs`. Ports `NodeToClientVersion`
  (the `V1` enum), its CBOR-term codec, and its JSON rendering.
- `lib.rs` gains `pub mod node_to_client;`.

The codec mirrors upstream's distinguishing-bit scheme: the wire tag
is the logical tag with `nodeToClientVersionBit` (bit 12) OR-ed in —
`V1` encodes as `1 | (1 << 12) = 4097`. Decoding requires that bit to
be set, then clears it to resolve the logical tag. `to_json` and
`to_int` return the bare logical tag (`1`).

`NodeToClientVersionData`, `stdVersionDataNTC`, and the
version-negotiation instances depend on the `ouroboros-network-api`
`NetworkMagic` type and land with the diffusion sub-arc.

5 unit tests: int-tag round-trip, the distinguishing-bit encoding
(`4097`), CBOR codec round-trip, rejection of a tag missing the bit,
and the JSON tag.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 dev/test/check-strict-mirror.py --fail-on-violation` —
  0 violations (audit TSV rebuilt for the 2 new files).
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 80 lib (+5 vs R728's 75) +
  2 golden, all green.

## Remaining (dmq-node arc)

- `NodeToNodeVersionData` / `NodeToClientVersionData` + version
  negotiation; the NtN / NtC mini-protocols.
- The `SigSubmission` `codecSigSubmission` TxSubmission2 wrapper +
  protocol-limit tables; the rest of `validateSig`.
- `Diffusion/*` wiring (the `DiffusionWiringDeferred` carve-out).
