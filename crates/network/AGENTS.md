---
name: network-crate-agent
description: Guidance for Cardano mini-protocol and peer networking work
---

Focus on typed protocol state machines, connection lifecycle, and exact wire-behavior boundaries.

## Scope
- Handshake, multiplexing, mini-protocol state machines, and peer lifecycle.
- Peer candidate resolution and bootstrap-target ordering helpers that feed runtime peer policy.
- Node-to-node and node-to-client protocol surfaces.

##  Rules *Non-Negotiable*
- Handshake and multiplexing interfaces MUST remain independent from peer policy logic.
- Simple reusable peer candidate ordering and address resolution helpers belong here, but full governor-style policy should remain a separate layer from transport and handshake primitives.
- Mini-protocols MUST be introduced incrementally, starting with ChainSync and BlockFetch.
- Testable state transitions MUST be preferred over implicit runtime behavior.
- Public protocol types, handshake surfaces, and state-machine functions MUST have Rustdocs where message flow or invariants are not self-evident.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Naming MUST mirror the official node and Ouroboros network specs so protocol traces and docs line up cleanly.
- Wire behavior and protocol sequencing MUST be explained by reference to the official node and upstream Ouroboros network sources.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant AGENTS.md file.

## Official Upstream References *Always research referances and add or update links as needed*
- Networking repository root: <https://github.com/IntersectMBO/ouroboros-network/>
- Multiplexer package: <https://github.com/IntersectMBO/ouroboros-network/tree/main/network-mux/>
- Framework and handshake layer: <https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network-framework/>
- Protocol implementations: <https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network-protocols/>
- Shelley networking spec PDF: <https://ouroboros-network.cardano.intersectmbo.org/pdfs/network-spec/>

## Current Phase
- Multiplexer framing (SDU header encode/decode, MiniProtocolNum, MiniProtocolDir) is implemented and tested.
- Async bearer transport (Bearer trait, TcpBearer, Sdu framing) is implemented and tested over TCP loopback.
- Multiplexer/demuxer (`mux.rs`) is implemented: `start()` splits a TcpStream into reader/writer halves, spawns background demux (reads SDUs, dispatches by protocol number to per-protocol ingress channels) and mux (collects tagged payloads from protocol handles, frames as SDUs) tasks. Per-protocol `ProtocolHandle` provides `send()`/`recv()` for exchanging payloads through the mux. Tested: single-protocol round-trip, multi-protocol routing, bidirectional exchange, connection close detection, empty payloads, multiple messages, clean shutdown on handle drop.
- SDU segmentation is implemented for large protocol payloads: mux writer splits payloads into `MAX_SEGMENT_SIZE` chunks, and `MessageChannel` reassembles segmented SDUs into complete CBOR messages using self-delimiting CBOR item-length detection.
- Peer connection lifecycle (`peer.rs`) is implemented: `connect()` (initiator) and `accept()` (responder) establish TCP, start the mux, run the Handshake mini-protocol (ProposeVersions / AcceptVersion / Refuse), and return a `PeerConnection` with negotiated version + data-protocol handles. Version negotiation selects the highest common version matching network magic. Tested: happy-path connect/accept, data exchange after handshake, handshake refusal on magic mismatch, highest-version selection.
- Handshake protocol types, state machine, and CBOR wire codec (ProposeVersions/AcceptVersion/Refuse/QueryReply) are complete.
- ChainSync, BlockFetch, KeepAlive, and TxSubmission2 have full message enums, validated state machines, transition tests, and CBOR wire codecs.
- Wire tags match upstream CDDL. ChainSync and BlockFetch now expose typed point/range helpers (`request_next_typed`, `find_intersect_points`, `request_range_points`) backed by ledger `Point` CBOR. ChainSync also exposes generic decoded-header support via `request_next_decoded_header::<H>()`. BlockFetch now exposes both per-item decode helpers (`recv_block_decoded::<B>()`, `recv_block_with()`, `recv_block_raw_with()`) and batch collection helpers (`request_range_collect*`) for raw, decoded, and raw+decoded flows. TxSubmission now uses ledger `TxId` for advertised/requested identifiers, exposes typed reply helpers for both storage `Tx` wrappers and ledger `MultiEraSubmittedTx` values, and tracks the outstanding/requestable TxId FIFO so invalid `ack`, blocking-mode, and request sequences are rejected locally while transaction bodies on the wire remain raw bytes.
- Upstream planning anchor: `cardano-node` `TopologyP2P` and `ouroboros-network` `Diffusion.Topology` model local root groups with `hotValency`, `warmValency`, `diffusionMode`, and trustability, while `PublicRootPeers` distinguishes configured public roots, bootstrap peers, ledger peers, and big ledger peers.
- Next implementation order:
	1. Align topology types with upstream P2P fields and semantics.
	2. Add root-set provider layers for local roots, public roots, bootstrap peers, and later DNS-refreshed public roots.
	3. Add a peer registry with peer source and peer status concepts aligned with upstream `PeerSource` and cold or warm or hot state.
	4. Only then add governor-style promotion, demotion, peer sharing, churn, and Genesis-specific policy.
