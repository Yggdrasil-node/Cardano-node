---
name: node-crate-agent
description: Guidance for runtime orchestration, CLI, and integration work
---

Focus on wiring crates together cleanly, preserving deterministic startup and shutdown behavior, and keeping runtime concerns out of domain crates.

## Scope
- Runtime orchestration, CLI, sync lifecycle, and top-level process behavior.
- Integration of storage, consensus, ledger, mempool, and network crates.

## Non-Negotiable Rules
- The node crate MUST remain an integration layer and MUST NOT absorb ledger or consensus business logic.
- Configuration, runtime startup, and sync orchestration MUST stay explicit.
- Composition MUST be preferred over cross-crate shortcuts.
- Major runtime entry points MUST have smoke coverage.
- Public node-facing integration types and runtime helpers MUST have Rustdocs when startup, shutdown, configuration, or sync semantics are not obvious.
- Naming and terminology MUST remain close to the official `cardano-node` so operational concepts map cleanly.
- Integration behavior MUST always be explained by anchoring it in the official node and the relevant upstream IntersectMBO implementation.

## Upstream References (add or update as needed)
- Node integration repository: <https://github.com/IntersectMBO/cardano-node/>
- Node runtime and packaging reference: <https://github.com/IntersectMBO/cardano-node/tree/master/cardano-node/>
- Default network configuration reference: <https://github.com/IntersectMBO/cardano-node/tree/master/configuration/>
- Submit API and auxiliary integration reference: <https://github.com/IntersectMBO/cardano-node/tree/master/cardano-submit-api/>

## Current Phase
- Keep the node crate thin and integration-focused.
- Runtime bootstrap wiring is implemented (`NodeConfig`, `PeerSession`, `bootstrap`) with smoke coverage.
- First sync orchestration slice is implemented (`sync_step`, `sync_steps`) to coordinate ChainSync and BlockFetch without embedding ledger/consensus rules.
- Block deserialization bridge is implemented for Shelley (`sync_step_decoded`, `decode_shelley_blocks`) as a typed handoff stage from network payloads.
- Typed ChainSync decode bridge is implemented (`sync_step_typed`, `decode_shelley_header`, `decode_point`) for header/point/tip payloads.
- Typed multi-step orchestration is implemented (`sync_steps_typed`, `TypedSyncProgress`) for deterministic step-by-step progress tracking.
- Bounded typed loop + storage handoff helpers are implemented (`sync_until_typed`, `apply_typed_step_to_volatile`, `apply_typed_progress_to_volatile`).
- Typed intersection finding (`typed_find_intersect`), batch sync-and-apply (`sync_batch_apply`), and KeepAlive heartbeat (`keepalive_heartbeat`) are implemented.
- Prefer smokeable runtime wiring over feature-rich operational behavior at this stage.
