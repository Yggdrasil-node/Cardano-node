---
name: node-src-subagent
description: Guidance for node runtime and sync orchestration implementation details
---

Focus on runtime composition of network clients and orchestration helpers that remain thin integration layers.

## Scope
- `main.rs` (CLI entry point), `config.rs` (JSON config types), `runtime.rs`, `sync.rs`, and library exports under `node/src`.
- Peer bootstrap wiring, configuration parsing, and sync control flow coordination.

## Non-Negotiable Rules
- Keep ledger and consensus business rules outside `node/src`.
- Favor small, explicit orchestration steps that are easy to smoke-test.
- Propagate typed errors and avoid hidden retries/backoff logic unless explicitly required.
- Keep naming close to `cardano-node` operational concepts.
- Public orchestration helpers MUST include Rustdocs when flow is non-trivial.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.

## Official Upstream References (add or update as needed)
- `cardano-node` runtime: <https://github.com/IntersectMBO/cardano-node/tree/master/cardano-node/>
- `ouroboros-consensus` integration behavior: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/>

## Current Phase
- **CLI**: `main.rs` uses `clap` with `run` (connect + sync) and `default-config` (emit JSON) subcommands. CLI flags (`--peer`, `--network-magic`, `--no-verify`, `--batch-size`, `--config`, `--network`) override config-file values. `--network` accepts `mainnet`, `preprod`, or `preview` presets.
- **Config**: `config.rs` defines `NodeConfigFile` (serde JSON) with peer address, network magic, protocol versions, KES params, keepalive interval. `default_config()` returns mainnet defaults. `NetworkPreset` enum with `to_config()` provides per-network constructors.
- Bootstrap wiring is implemented (`NodeConfig`, `PeerSession`, `bootstrap`).
- Full sync orchestration stack: `sync_step`, typed decode bridges, bounded loops, intersection finding, batch apply, managed sync service with `tokio::select!` shutdown + `ctrl_c` signal handling. Typed sync now consumes decoded ChainSync headers, typed point/tip payloads, decoded Shelley blocks, and multi-era BlockFetch batch helpers from `yggdrasil-network`. Verified sync also uses raw+decoded BlockFetch batch helpers while keeping body-hash checks and verification policy in `node`.
- Multi-era block decode for all 7 era tags (Byron through Conway). Byron blocks are structurally decoded via `ByronBlock::decode_ebb()`/`decode_main()`, carrying epoch, slot, chain_difficulty, prev_hash, and raw header bytes. Alonzo (tag 5) uses dedicated `AlonzoBlock` (5-element format with `invalid_transactions` and TPraos header), distinct from the 4-element `ShelleyBlock` used for Shelley/Allegra/Mary (tags 2–4).
- Consensus header verification bridge for both Shelley-era (`shelley_header_to_consensus`, `verify_shelley_header`) and Praos-era (`praos_header_to_consensus`, `verify_praos_header`) headers. `verify_multi_era_block` dispatches to the correct verifier per era (Shelley through Alonzo → Shelley, Babbage/Conway → Praos).
- VRF data flow: bridge functions now carry leader VRF proof/output (and nonce VRF for TPraos) through to the consensus `HeaderBody`. `verify_block_vrf` extracts VRF key + leader proof from each era's block header and delegates to `verify_leader_proof`. `VrfVerificationParams` bundles epoch nonce, sigma, and active slot coefficient.
- Nonce evolution wiring: `apply_nonce_evolution` extracts the per-era VRF nonce contribution (TPraos `nonce_vrf` or Praos `vrf_result`) and prev_hash from a `MultiEraBlock` and feeds them to `NonceEvolutionState::apply_block`. Byron blocks are skipped. This enables epoch nonce tracking during sync without modifying `sync_batch_apply_verified`.
- Block body hash verification: `verify_block_body_hash` computes Blake2b-256 of body elements and compares against the hash declared in the header. `extract_header_block_body_hash` handles both 14-element (Praos) and 15-element (Shelley) header bodies. Wired into `sync_batch_apply_verified` via `VerificationConfig.verify_body_hash`.
- Block header hash computation uses real Blake2b-256.
- Mempool sync eviction: `extract_tx_ids` + `evict_confirmed_from_mempool`. TxSubmission runtime wiring now includes `serve_txsubmission_request_from_mempool`, a thin one-shot helper that answers a single `TxSubmissionClient` request from fee-ordered mempool state, plus `serve_txsubmission_request_from_reader` and `run_txsubmission_service`, which use an upstream-aligned `TxSubmissionMempoolReader`/`MempoolSnapshot` flow with a monotonic `last_idx` cursor so managed outbound serving only advertises transactions newer than the last acknowledged snapshot position.
- Verified sync service: `run_verified_sync_service` with `VerifiedSyncServiceConfig` (batch_size + verification + optional nonce config + optional security_param) and `VerifiedSyncServiceOutcome` (includes final `NonceEvolutionState`, optional `ChainState`, and `stable_block_count`). Uses `sync_batch_apply_verified` in a managed loop with `tokio::select!` shutdown. CLI `run` command now uses this as the default pipeline.
- ChainState integration: `multi_era_block_to_chain_entry` extracts `ChainEntry` from multi-era blocks (all eras including Byron), `track_chain_state` applies sync steps to `ChainState` and drains stable entries, `promote_stable_blocks` copies stable blocks from volatile to immutable store.
- Genesis parameters: `NodeConfigFile` now has `epoch_length`, `security_param_k`, `active_slot_coeff` (all with serde defaults). `run` command computes `stability_window = 3k/f` and builds `NonceEvolutionConfig`.
