---
name: node-crate-agent
description: Guidance for runtime orchestration, CLI, and integration work
---

Focus on wiring crates together cleanly, preserving deterministic startup and shutdown behavior, and keeping runtime concerns out of domain crates.

## Scope
- Runtime orchestration, CLI, sync lifecycle, and top-level process behavior.
- Integration of storage, consensus, ledger, mempool, and network crates.

## Rules
- Treat the node crate as an integration layer, not a place for ledger or consensus business logic.
- Keep configuration, runtime startup, and sync orchestration explicit.
- Prefer composition over cross-crate shortcuts.
- Add smoke coverage for major runtime entry points.

## Upstream References
- Node integration repository: <https://github.com/IntersectMBO/cardano-node>
- Node runtime and packaging reference: <https://github.com/IntersectMBO/cardano-node/tree/master/cardano-node>
- Default network configuration reference: <https://github.com/IntersectMBO/cardano-node/tree/master/configuration>
- Submit API and auxiliary integration reference: <https://github.com/IntersectMBO/cardano-node/tree/master/cardano-submit-api>

## Current Phase
- Keep the node crate thin and integration-focused.
- Prefer smokeable runtime wiring over feature-rich operational behavior at this stage.
