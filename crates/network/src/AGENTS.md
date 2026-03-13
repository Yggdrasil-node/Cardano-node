---
name: network-src-subagent
description: Guidance for concrete network transport, mux, and client-driver implementation work
---

Focus on implementation details for bearer I/O, mux/demux behavior, protocol driver ergonomics, and wire-level safety properties.

## Scope
- `bearer.rs`, `multiplexer.rs`, `mux.rs`, `peer.rs`, and typed client drivers.
- CBOR message boundary handling, segmentation/reassembly, and protocol-handle composition.

## Non-Negotiable Rules
- Keep wire framing deterministic and byte-accurate.
- Do not leak protocol business logic from `protocols/` state machines into transport primitives.
- Preserve strict separation between raw transport (`ProtocolHandle`) and higher-level message orchestration (`MessageChannel`, client drivers).
- Any receive-path buffering or boundary detection changes MUST ship with regression tests for partial/incremental payload delivery.
- Public transport and driver APIs MUST include Rustdocs when behavior is non-obvious.

## Upstream References (add or update as needed)
- Multiplexer implementation: <https://github.com/IntersectMBO/ouroboros-network/tree/main/network-mux>
- Network framework: <https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network-framework>
- Mini-protocol implementations: <https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network-protocols>

## Current Phase
- TCP bearer and SDU framing are implemented and tested.
- Mux/demux routing is implemented with per-protocol handles.
- Large-message SDU segmentation/reassembly is implemented via `MAX_SEGMENT_SIZE` + `MessageChannel`.
- Typed ChainSync, BlockFetch, KeepAlive, and TxSubmission client drivers are in place. TxSubmission now uses typed ledger `TxId` values for request/advertise flows, provides typed reply helpers for both `Vec<Tx>` and `Vec<MultiEraSubmittedTx>`, and maintains an outstanding/requestable TxId FIFO so invalid acknowledgements and transaction requests are rejected before replying while preserving raw wire bodies.
- Next: typed protocol payload decoding (replace remaining opaque `Vec<u8>` payloads where practical).
