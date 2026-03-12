---
name: network-protocols-subagent
description: Guidance for mini-protocol state machine modules
---

Focus on explicit node-to-node and node-to-client protocol state machines, messages, and transition safety.

## Scope
- ChainSync, BlockFetch, and later operational mini-protocols.
- Protocol states, legal transitions, and shared naming conventions.

## Non-Negotiable Rules
- Each protocol module MUST stay self-contained around one protocol state machine.
- Legal transitions MUST be modeled explicitly before transport or peer policy concerns are added.
- Protocol terminology MUST match the upstream network spec.
- Public protocol states, message helpers, and transition functions MUST have Rustdocs when the legal flow is not obvious from the type shape.
- Naming MUST stay aligned with the official node and network spec so the implementation remains easy to compare against upstream traces and docs.

## Upstream References (add or update as needed)
- Protocol implementations: <https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network-protocols>
- Framework and handshake layer: <https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network-framework>
- Shelley networking spec PDF: <https://ouroboros-network.cardano.intersectmbo.org/pdfs/network-spec>

## Current Phase
- ChainSync has 5 states (StIdle, StCanAwait, StMustReply, StIntersect, StDone) and 8 message variants with validated transitions.
- BlockFetch has 4 states (StIdle, StBusy, StStreaming, StDone) and 6 message variants with validated transitions.
- Wire tags and naming match upstream `Ouroboros.Network.Protocol.{ChainSync,BlockFetch}.Type`.
- Payload types are opaque (`Vec<u8>`); typed point/tip/header/block payloads will come with CBOR codec work.
- Other protocols (TxSubmission2, KeepAlive, PeerSharing) deferred until shared patterns stabilize.
