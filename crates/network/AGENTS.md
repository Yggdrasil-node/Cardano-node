---
name: network-crate-agent
description: Guidance for Cardano mini-protocol and peer networking work
---

Focus on typed protocol state machines, connection lifecycle, and exact wire-behavior boundaries.

## Scope
- Handshake, multiplexing, mini-protocol state machines, and peer lifecycle.
- Node-to-node and node-to-client protocol surfaces.

## Rules
- Keep handshake and multiplexing interfaces independent from specific peer policies.
- Introduce mini-protocols incrementally, starting with ChainSync and BlockFetch.
- Prefer testable state transitions over implicit runtime behavior.

## Upstream References
- Networking repository root: <https://github.com/IntersectMBO/ouroboros-network>
- Multiplexer package: <https://github.com/IntersectMBO/ouroboros-network/tree/main/network-mux>
- Framework and handshake layer: <https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network-framework>
- Protocol implementations: <https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network-protocols>
- Shelley networking spec PDF: <https://ouroboros-network.cardano.intersectmbo.org/pdfs/network-spec>

## Current Phase
- Keep the current implementation focused on handshake, mux, ChainSync, and BlockFetch boundaries.
- Add other protocols only after the shared framing is stable.
