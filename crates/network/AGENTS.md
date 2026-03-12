---
name: network-crate-agent
description: Guidance for Cardano mini-protocol and peer networking work
---

Focus on typed protocol state machines, connection lifecycle, and exact wire-behavior boundaries.

## Rules
- Keep handshake and multiplexing interfaces independent from specific peer policies.
- Introduce mini-protocols incrementally, starting with ChainSync and BlockFetch.
- Prefer testable state transitions over implicit runtime behavior.
