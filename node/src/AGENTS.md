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
- Managed sync service (`run_sync_service`, `SyncServiceConfig`, `SyncServiceOutcome`) with `tokio::select!` shutdown control is implemented.
- Consensus header verification bridge (`shelley_opcert_to_consensus`, `shelley_header_body_to_consensus`, `shelley_header_to_consensus`, `verify_shelley_header`) is implemented.
- Multi-era block decode (`MultiEraBlock`, `decode_multi_era_block`, `decode_multi_era_blocks`) with Byron opaque and Shelley decoded is implemented.
- Block header hash computation uses real Blake2b-256 via `ShelleyHeader::header_hash()`; `shelley_block_to_block` and `compute_tx_id` use proper cryptographic hashing.
- Verified multi-era sync pipeline (`multi_era_block_to_block`, `verify_multi_era_block`, `sync_step_multi_era`, `apply_multi_era_step_to_volatile`, `sync_batch_apply_verified`, `VerificationConfig`) is implemented wiring consensus verification into the multi-era sync flow.
- Next: expand multi-era decode to Babbage/Conway, integrate mempool eviction with sync pipeline, add persistent storage backend.
