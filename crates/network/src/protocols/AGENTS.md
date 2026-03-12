---
name: network-protocols-subagent
description: Guidance for mini-protocol state machine modules
---

Focus on explicit node-to-node and node-to-client protocol state machines, messages, and transition safety.

## Scope
- ChainSync, BlockFetch, and later operational mini-protocols.
- Protocol states, legal transitions, and shared naming conventions.

## Rules
- Keep each protocol module self-contained around one protocol state machine.
- Model legal transitions explicitly before adding transport or peer policy concerns.
- Prefer protocol terminology that matches the upstream network spec.

## Upstream References
- Protocol implementations: <https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network-protocols>
- Framework and handshake layer: <https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network-framework>
- Shelley networking spec PDF: <https://ouroboros-network.cardano.intersectmbo.org/pdfs/network-spec>

## Current Phase
- ChainSync and BlockFetch are the primary targets.
- Keep other protocol additions behind stable shared protocol patterns.
