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

## Upstream References (add or update as needed)
- `cardano-node` runtime: <https://github.com/IntersectMBO/cardano-node/tree/master/cardano-node>
- `ouroboros-consensus` integration behavior: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus>

## Current Phase
- **CLI**: `main.rs` uses `clap` with `run` (connect + sync) and `default-config` (emit JSON) subcommands. CLI flags (`--peer`, `--network-magic`, `--no-verify`, `--batch-size`, `--config`) override config-file values.
- **Config**: `config.rs` defines `NodeConfigFile` (serde JSON) with peer address, network magic, protocol versions, KES params, keepalive interval. `default_config()` returns mainnet defaults.
- Bootstrap wiring is implemented (`NodeConfig`, `PeerSession`, `bootstrap`).
- Full sync orchestration stack: `sync_step`, typed decode bridges, bounded loops, intersection finding, batch apply, managed sync service with `tokio::select!` shutdown + `ctrl_c` signal handling.
- Multi-era block decode for all 7 era tags (Byron through Conway).
- Consensus header verification bridge for both Shelley-era (`shelley_header_to_consensus`, `verify_shelley_header`) and Praos-era (`praos_header_to_consensus`, `verify_praos_header`) headers. `verify_multi_era_block` dispatches to the correct verifier per era (Shelley through Alonzo → Shelley, Babbage/Conway → Praos).
- Block body hash verification: `verify_block_body_hash` computes Blake2b-256 of body elements and compares against the hash declared in the header. `extract_header_block_body_hash` handles both 14-element (Praos) and 15-element (Shelley) header bodies. Wired into `sync_batch_apply_verified` via `VerificationConfig.verify_body_hash`.
- Block header hash computation uses real Blake2b-256.
- Mempool sync eviction: `extract_tx_ids` + `evict_confirmed_from_mempool`.
