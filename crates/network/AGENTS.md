---
name: network-crate-agent
description: Guidance for Cardano mini-protocol and peer networking work
---

Focus on typed protocol state machines, connection lifecycle, and exact wire-behavior boundaries.

## Scope
- Handshake, multiplexing, mini-protocol state machines, and peer lifecycle.
- Node-to-node and node-to-client protocol surfaces.

## Non-Negotiable Rules
- Handshake and multiplexing interfaces MUST remain independent from peer policy logic.
- Mini-protocols MUST be introduced incrementally, starting with ChainSync and BlockFetch.
- Testable state transitions MUST be preferred over implicit runtime behavior.
- Public protocol types, handshake surfaces, and state-machine functions MUST have Rustdocs where message flow or invariants are not self-evident.
- Naming MUST mirror the official node and Ouroboros network specs so protocol traces and docs line up cleanly.
- Wire behavior and protocol sequencing MUST be explained by reference to the official node and upstream Ouroboros network sources.

## Upstream References (add or update as needed)
- Networking repository root: <https://github.com/IntersectMBO/ouroboros-network/>
- Multiplexer package: <https://github.com/IntersectMBO/ouroboros-network/tree/main/network-mux/>
- Framework and handshake layer: <https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network-framework/>
- Protocol implementations: <https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network-protocols/>
- Shelley networking spec PDF: <https://ouroboros-network.cardano.intersectmbo.org/pdfs/network-spec/>

## Current Phase
- Multiplexer framing (SDU header encode/decode, MiniProtocolNum, MiniProtocolDir) is implemented and tested.
- Async bearer transport (Bearer trait, TcpBearer, Sdu framing) is implemented and tested over TCP loopback.
- Multiplexer/demuxer (`mux.rs`) is implemented: `start()` splits a TcpStream into reader/writer halves, spawns background demux (reads SDUs, dispatches by protocol number to per-protocol ingress channels) and mux (collects tagged payloads from protocol handles, frames as SDUs) tasks. Per-protocol `ProtocolHandle` provides `send()`/`recv()` for exchanging payloads through the mux. Tested: single-protocol round-trip, multi-protocol routing, bidirectional exchange, connection close detection, empty payloads, multiple messages, clean shutdown on handle drop.
- Handshake protocol types, state machine, and CBOR wire codec (ProposeVersions/AcceptVersion/Refuse/QueryReply) are complete.
- ChainSync, BlockFetch, KeepAlive, and TxSubmission2 have full message enums, validated state machines, transition tests, and CBOR wire codecs.
- Wire tags match upstream CDDL; payload types remain opaque (`Vec<u8>`) pending typed CBOR payloads.
- Next: Peer lifecycle (connection setup → handshake → protocol dispatch loop), typed protocol payloads, and SDU segmentation for large messages.
