---
title: "Round 766 dmq-node NodeToNodeVersionData codec (dmq-node runtime sub-arc, slice 8)"
parent: Reference
---

# Round 766 dmq-node NodeToNodeVersionData codec (dmq-node runtime sub-arc, slice 8)

Date: 2026-05-22

## Scope

Slice 8 of the dmq-node runtime/diffusion sub-arc — the
`NodeToNodeVersionData` CBOR-term codec.

## What shipped

`crates/tools/dmq-node/src/node_to_node/version.rs`:

- `NodeToNodeVersionData::encode_term` / `decode_term` — mirror of
  upstream `nodeToNodeCodecCBORTerm`: a CBOR 4-element array
  `[networkMagic, diffusionMode, peerSharing, query]`, where
  `diffusionMode` is the boolean `true` for `InitiatorOnly` and
  `peerSharing` is the integer `0` / `1`. `decode_term` rejects an
  out-of-range network magic (`> u32::MAX`) and an unknown
  peer-sharing tag, mirroring upstream's bound checks.

This completes the full `NodeToNode/Version.hs` surface — the
version enum + its codec + JSON, and the version data + negotiation +
CBOR-term codec.

2 unit tests cover the version-data codec round-trip (with the `0x84`
4-array header check) and the out-of-range peer-sharing rejection.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 147 lib (+2 vs R765's 145) +
  2 golden, all green.

## Remaining (dmq-node runtime sub-arc)

- The `NodeToClientVersionData` + its CBOR-term codec; the NtN / NtC
  mux bundles; `Diffusion/*`; `NodeKernel`; `tracer.rs`; the `run()`
  loop replacing `RunError::DiffusionWiringDeferred`.
