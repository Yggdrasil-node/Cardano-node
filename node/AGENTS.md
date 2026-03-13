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
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Integration behavior MUST always be explained by anchoring it in the official node and the relevant upstream IntersectMBO implementation.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If anything of the context is outdated, missing, or incorrect, edit the file accordingly. make sure that single line exceeding ".maxTokenizationLineLength"

## Official Upstream References (add or update as needed)
- Node integration repository: <https://github.com/IntersectMBO/cardano-node/>
- Node runtime and packaging reference: <https://github.com/IntersectMBO/cardano-node/tree/master/cardano-node/>
- Default network configuration reference: <https://github.com/IntersectMBO/cardano-node/tree/master/configuration/>
- Submit API and auxiliary integration reference: <https://github.com/IntersectMBO/cardano-node/tree/master/cardano-submit-api/>
- Enviorment configuration references "preview, Preprod, mainnet: <https://book.world.dev.cardano.org/env-preview.html>, <https://book.world.dev.cardano.org/env-preprod.html>, <https://book.world.dev.cardano.org/env-mainnet.html>

## Current Phase
- Keep the node crate thin and integration-focused.
- **Configuration**: `NodeConfigFile` (JSON, serde) with peer address, network magic, protocol versions, KES parameters, and keepalive interval. `default_config()` returns mainnet defaults. `NetworkPreset` enum (`Mainnet | Preprod | Preview`) with `FromStr`/`Display` and per-network constructors (`mainnet_config()`, `preprod_config()`, `preview_config()`).
- **CLI**: `clap`-based binary with `run` (connect + sync) and `default-config` (emit JSON) subcommands. CLI flags (`--peer`, `--network-magic`, `--no-verify`, `--batch-size`, `--network`) override config-file values. `--network` accepts `mainnet`, `preprod`, or `preview` as a preset.
- **Network config files**: `node/configuration/{mainnet,preprod,preview}/` each contain byron-genesis.json, shelley-genesis.json, alonzo-genesis.json, conway-genesis.json, config.json, topology.json sourced from the Cardano Operations Book.
- Runtime bootstrap wiring is implemented (`NodeConfig`, `PeerSession`, `bootstrap`) with smoke coverage.
- Full sync orchestration stack is implemented: `sync_step`, `sync_steps`, typed decode bridges, bounded loops, intersection finding, batch apply, managed sync service with graceful shutdown via `tokio::signal::ctrl_c`.
- Multi-era block decode (`MultiEraBlock`, `decode_multi_era_block`, `decode_multi_era_blocks`) with Byron opaque, Shelley/Allegra/Mary/Alonzo decoded as `ShelleyBlock`, Babbage decoded as `BabbageBlock`, and Conway decoded as `ConwayBlock` is implemented. All seven era tags (0–7) are handled.
- Consensus header verification bridge (`verify_shelley_header`, `verify_multi_era_block`, `VerificationConfig`) is wired into the sync flow.
- Block body hash verification (`verify_block_body_hash`, `VerificationConfig.verify_body_hash`) computes Blake2b-256 of block body elements and compares against the header-declared hash. Wired into `sync_batch_apply_verified`.
- Block header hash computation uses real Blake2b-256.
- Mempool sync eviction (`extract_tx_ids`, `evict_confirmed_from_mempool`) is implemented. TxSubmission runtime integration now includes `serve_txsubmission_request_from_mempool` for one-shot fee-ordered request handling, plus `serve_txsubmission_request_from_reader`/`run_txsubmission_service` for upstream-style managed outbound serving using a `TxSubmissionMempoolReader`, `MempoolSnapshot`, and monotonic `last_idx` cursor. `run_txsubmission_service_shared` provides the same managed outbound flow over `SharedMempool` so concurrent mempool updates can be observed while the service is running. `add_tx_to_mempool`/`add_tx_to_shared_mempool` now provide explicit addTx-style inbound admission helpers, and `add_txs_to_mempool`/`add_txs_to_shared_mempool` provide ordered addTxs-style admission where accepted transactions advance the staged `LedgerState` for later transactions in the same batch. All paths relay stored submitted-transaction bytes and send `MsgDone` on blocking requests when no eligible transactions remain.
- Verified sync service (`run_verified_sync_service`, `VerifiedSyncServiceConfig`, `VerifiedSyncServiceOutcome`) uses the multi-era verified pipeline with per-block nonce evolution tracking and optional ChainState tracking. Returns final `NonceEvolutionState` and `ChainState` on shutdown. CLI `run` command now uses this pipeline by default.
- ChainState integration: `multi_era_block_to_chain_entry`, `track_chain_state`, `promote_stable_blocks` wire consensus `ChainState` into the sync flow. `VerifiedSyncServiceConfig.security_param` enables chain tracking with stability window enforcement.
- Genesis parameters in `NodeConfigFile`: `epoch_length` (432000), `security_param_k` (2160), `active_slot_coeff` (0.05). Stability window computed as `3k/f`.
- Prefer smokeable runtime wiring over feature-rich operational behavior at this stage.
