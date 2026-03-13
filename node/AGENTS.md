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
- Prefer smokeable runtime wiring over feature-rich operational behavior at this stage.
