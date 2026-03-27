---
name: network-protocols-subagent
description: Guidance for mini-protocol state machine modules
---

Focus on explicit node-to-node and node-to-client protocol state machines, messages, and transition safety.

## Scope
- ChainSync, BlockFetch, KeepAlive, TxSubmission, PeerSharing, and future operational mini-protocols.
- Protocol states, legal transitions, and shared naming conventions.

##  Rules *Non-Negotiable*
- Each protocol module MUST stay self-contained around one protocol state machine.
- Legal transitions MUST be modeled explicitly before transport or peer policy concerns are added.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Protocol terminology MUST match the upstream network spec.
- Public protocol states, message helpers, and transition functions MUST have Rustdocs when the legal flow is not obvious from the type shape.
- Naming MUST stay aligned with the official node and network spec so the implementation remains easy to compare against upstream traces and docs.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant AGENTS.md file.

## Official Upstream References *Always research references and add or update links as needed*
- ChainSync protocol: <https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network-protocols/src/Ouroboros/Network/Protocol/ChainSync>
- BlockFetch protocol: <https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network-protocols/src/Ouroboros/Network/Protocol/BlockFetch>
- TxSubmission2 protocol: <https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network-protocols/src/Ouroboros/Network/Protocol/TxSubmission2>
- KeepAlive protocol: <https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network-protocols/src/Ouroboros/Network/Protocol/KeepAlive>
- PeerSharing protocol: <https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network-protocols/src/Ouroboros/Network/Protocol/PeerSharing>
- LocalStateQuery protocol: <https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network-protocols/src/Ouroboros/Network/Protocol/LocalStateQuery>
- LocalTxSubmission protocol: <https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network-protocols/src/Ouroboros/Network/Protocol/LocalTxSubmission>
- LocalTxMonitor protocol: <https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network-protocols/src/Ouroboros/Network/Protocol/LocalTxMonitor>
- Framework and handshake layer: <https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network-framework>
- Shelley networking spec PDF: <https://ouroboros-network.cardano.intersectmbo.org/pdfs/network-spec>

## Current Phase
- ChainSync has 5 states (StIdle, StCanAwait, StMustReply, StIntersect, StDone), 8 message variants with validated transitions, and a CBOR wire codec.
- BlockFetch has 4 states (StIdle, StBusy, StStreaming, StDone), 6 message variants with validated transitions, and a CBOR wire codec.
- KeepAlive has 3 states (StClient, StServer, StDone), 3 message variants with validated transitions, and a CBOR wire codec.
- TxSubmission2 has 5 states (StInit, StIdle, StTxIds, StTxs, StDone), 6 message variants with validated transitions, and a CBOR wire codec. MsgDone only legal from blocking StTxIds. Transaction identifiers now use ledger `TxId`; transaction bodies remain raw bytes.
- PeerSharing has 3 states (StClient, StServer, StDone), 3 message variants (MsgShareRequest, MsgSharePeers, MsgDone) with validated transitions, and a CBOR wire codec. `SharedPeerAddress` encodes IPv4/IPv6 as `[ip_type, ip_bytes, port]`.
- Wire tags and naming match upstream `Ouroboros.Network.Protocol.{ChainSync,BlockFetch,KeepAlive,TxSubmission2,PeerSharing}.Type`.
- Payload typing is incremental: point/tip/header/block payloads are typed where practical, TxSubmission ids are typed, and transaction bodies stay raw until typed transaction codecs are available.
- All five data mini-protocols and PeerSharing are now implemented.
