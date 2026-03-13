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
- **CLI**: `clap`-based binary with `run` (connect + sync) and `default-config` (emit JSON) subcommands. CLI flags (`--peer`, `--network-magic`, `--no-verify`, `--batch-size`) override config-file values.
- **Configuration**: `NodeConfigFile` (JSON, serde) with peer address, network magic, protocol versions, KES parameters, and keepalive interval. `default_config()` returns mainnet defaults.
- Runtime bootstrap wiring is implemented (`NodeConfig`, `PeerSession`, `bootstrap`) with smoke coverage.
- Full sync orchestration stack is implemented: `sync_step`, `sync_steps`, typed decode bridges, bounded loops, intersection finding, batch apply, managed sync service with graceful shutdown via `tokio::signal::ctrl_c`.
- Multi-era block decode (`MultiEraBlock`, `decode_multi_era_block`, `decode_multi_era_blocks`) with Byron opaque, Shelley/Allegra/Mary/Alonzo decoded as `ShelleyBlock`, Babbage decoded as `BabbageBlock`, and Conway decoded as `ConwayBlock` is implemented. All seven era tags (0–7) are handled.
- Consensus header verification bridge (`verify_shelley_header`, `verify_multi_era_block`, `VerificationConfig`) is wired into the sync flow.
- Block body hash verification (`verify_block_body_hash`, `VerificationConfig.verify_body_hash`) computes Blake2b-256 of block body elements and compares against the header-declared hash. Wired into `sync_batch_apply_verified`.
- Block header hash computation uses real Blake2b-256.
- Mempool sync eviction (`extract_tx_ids`, `evict_confirmed_from_mempool`) is implemented.
- Prefer smokeable runtime wiring over feature-rich operational behavior at this stage.
