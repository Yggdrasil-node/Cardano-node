---
name: node-src-subagent
description: Guidance for node runtime and sync orchestration implementation details
---

Focus on runtime composition of network clients and orchestration helpers that remain thin integration layers.

## Scope
- `runtime.rs`, `sync.rs`, and library exports under `node/src`.
- Peer bootstrap wiring and sync control flow coordination.

## Non-Negotiable Rules
- Keep ledger and consensus business rules outside `node/src`.
- Favor small, explicit orchestration steps that are easy to smoke-test.
- Propagate typed errors and avoid hidden retries/backoff logic unless explicitly required.
- Keep naming close to `cardano-node` operational concepts.
- Public orchestration helpers MUST include Rustdocs when flow is non-trivial.

## Upstream References (add or update as needed)
- `cardano-node` runtime: <https://github.com/IntersectMBO/cardano-node/tree/master/cardano-node>
- `ouroboros-consensus` integration behavior: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus>

## Current Phase
- Bootstrap wiring is implemented (`NodeConfig`, `PeerSession`, `bootstrap`).
- First sync orchestration slice is implemented (`sync_step`, `sync_steps`).
- Shelley block deserialization bridge is implemented (`sync_step_decoded`, `decode_shelley_blocks`).
- Typed ChainSync decode bridge is implemented (`sync_step_typed`, `decode_shelley_header`, `decode_point`).
- Typed multi-step orchestration is implemented (`sync_steps_typed`, `TypedSyncProgress`).
- Bounded typed loop and storage handoff helpers are implemented (`sync_until_typed`, `apply_typed_step_to_volatile`, `apply_typed_progress_to_volatile`).
- Typed intersection finding (`typed_find_intersect`), batch sync-and-apply (`sync_batch_apply`), and KeepAlive heartbeat (`keepalive_heartbeat`) are implemented.
- Next: long-running managed sync service with graceful shutdown, then staged consensus/ledger integration on fetched blocks.
