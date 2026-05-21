---
title: "Round 767 dmq-node NodeToClientVersionData (dmq-node runtime sub-arc, slice 9)"
parent: Reference
---

# Round 767 dmq-node NodeToClientVersionData (dmq-node runtime sub-arc, slice 9)

Date: 2026-05-22

## Scope

Slice 9 of the dmq-node runtime/diffusion sub-arc — the NtC
version-data type, negotiation, and CBOR-term codec.

## What shipped

`crates/tools/dmq-node/src/node_to_client/version.rs`:

- `NodeToClientVersionData` — `network_magic` / `query`, mirror of
  upstream `data NodeToClientVersionData` (simpler than its NtN
  sibling — no diffusion mode or peer sharing).
- `NodeToClientVersionData::standard` — `stdVersionDataNTC` (`query`
  defaults to `false`).
- `accept` — mirror of `instance Acceptable`: the network magic must
  match (a mismatch refuses); `query` is the OR of the two.
- `encode_term` / `decode_term` — `nodeToClientCodecCBORTerm`: a CBOR
  2-element array `[networkMagic, query]`; an out-of-range magic is
  rejected.
- `to_json` — `{ "NetworkMagic": …, "Query": … }`.

This completes the full `NodeToClient/Version.hs` surface.

4 unit tests cover `standard`, the negotiation (magic match + `query`
OR + mismatch refusal), the CBOR-term codec round-trip, and the JSON
shape.

## Validation

- `cargo fmt --all -- --check` — green.
- `python3 scripts/check-strict-mirror.py --fail-on-violation` —
  0 violations.
- `cargo check-all` — green.
- `cargo lint` — green.
- `cargo test -p yggdrasil-dmq-node` — 151 lib (+4 vs R766's 147) +
  2 golden, all green.

## Remaining (dmq-node runtime sub-arc)

- The NtN / NtC mux protocol bundles; `Diffusion/*`; `NodeKernel`;
  `tracer.rs`; the `run()` loop replacing
  `RunError::DiffusionWiringDeferred`.
