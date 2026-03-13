---
name: Ygdrasil-cardano-rust-node
description: Root agent for the Yggdrasil Rust Cardano node workspace
---

You are implementing a pure Rust Cardano node with no FFI dependencies.

## Mission
- Maintain a production-oriented Cargo workspace for a long-lived Cardano node implementation.
- Preserve deterministic behavior, byte-accurate serialization goals, and clear crate boundaries.
- Favor interfaces and tests that support staged delivery over speculative completeness.

## Spec Priority
1. Formal ledger specifications and protocol papers
2. Cardano ledger CDDL schemas
3. Accepted Cardano improvement proposals
4. Haskell implementation behavior for compatibility verification

## Workspace Boundaries
- `crates/crypto` owns cryptographic primitives and related encodings.
- `crates/cddl-codegen` owns code generation from pinned specifications.
- `crates/ledger` owns ledger state transitions and era modeling.
- `crates/storage` owns durable storage and snapshot interfaces.
- `crates/consensus` owns chain selection, leader election, and rollback rules.
- `crates/mempool` owns transaction intake and ordering.
- `crates/network` owns multiplexing, mini-protocols, and peer management.
- `node/` owns orchestration, CLI, and runtime integration.
- `specs/upstream-test-vectors` officially test-vectors from the `IntersectMBO` repositories.

## Upstream References (add or update as needed)
- `crates/crypto`: <https://github.com/IntersectMBO/cardano-base/tree/master/cardano-crypto-class> and <https://github.com/IntersectMBO/cardano-base/tree/master/cardano-crypto-praos>
- `crates/cddl-codegen`: <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras> and <https://github.com/IntersectMBO/cardano-ledger/tree/master/libs/cardano-ledger-binary>
- `crates/ledger`: <https://github.com/IntersectMBO/cardano-ledger> and <https://github.com/IntersectMBO/formal-ledger-specifications>
- `crates/storage`: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/>
- `crates/consensus`: <https://github.com/IntersectMBO/ouroboros-consensus/> and <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/docs/agda-spec>
- `crates/mempool`: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/> and <https://github.com/IntersectMBO/cardano-node/tree/master/cardano-submit-api/>
- `crates/network`: <https://github.com/IntersectMBO/ouroboros-network/> and <https://ouroboros-network.cardano.intersectmbo.org/pdfs/network-spec>
- `node/`: <https://github.com/IntersectMBO/cardano-node/> and <https://github.com/IntersectMBO/cardano-node/tree/master/configuration/>

## Non-Negotiable Rules
- Always write typesafe Rust code.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If anything of the context is outdated, missing, or incorrect, edit the file accordingly.
- Always research the official relevant upstream IntersectMBO repositories before introducing any local terminology, behavior, or design that is not directly traceable to an upstream source.
- New dependencies MUST be justified in `docs/DEPENDENCIES.md` before they are treated as accepted.
- FFI-backed cryptography and hidden native dependencies MUST NOT be introduced.
- Generated artifacts MUST remain reproducible and generated code MUST NOT be edited by hand.
- Implementation work MUST favor incremental milestones that compile and test cleanly.
- Public modules, types, and functions MUST have proper Rustdocs whenever behavior is non-obvious or externally consumed.
- Explanations of behavior or naming MUST be cross-checked against the official `cardano-node` and the relevant upstream IntersectMBO repositories.
- Type and function naming MUST stay as close to upstream terminology as practical so parity work and fixture comparison remain tractable.
- Cryptographic, protocol, and serialization parity with the official node is a non-negotiable long-term target even when an implementation slice is still incomplete.
- when you dont know how to proceed after reserching the official node and upstream repositories, you can reserch <https://github.com/pragma-org/amaru/> and <https://github.com/txpipe/dolos/> for examples of how other Rust Cardano projects have approached similar problems, but do not treat them as authoritative sources for design or behavior decisions.

## Verification Expectations
- `cargo check-all`
- `cargo test-all`
- `cargo lint`

## Current Phase
- Workspace foundation is complete and compileable.
- Active implementation work is in progress across `crates/crypto`, `crates/cddl-codegen`, `crates/ledger`, `crates/network`, and `node/`.
- `crates/network` now includes handshake + mux + peer lifecycle, all four mini-protocol state machines/wire codecs, typed client drivers, and SDU segmentation/reassembly support for large protocol messages.
- `node/` orchestration and sync pipeline:
  - Bootstrap: `NodeConfig`, `PeerSession`, `bootstrap`.
  - Raw sync: `sync_step`, `sync_steps`, `sync_step_decoded`, `decode_shelley_blocks`.
  - Typed sync: `sync_step_typed`, `decode_shelley_header`, `decode_point`, `sync_steps_typed`, `sync_until_typed`.
  - Storage handoff: `apply_typed_step_to_volatile`, `apply_typed_progress_to_volatile`.
  - Intersection + batch: `typed_find_intersect`, `sync_batch_apply`.
  - KeepAlive: `keepalive_heartbeat`.
  - Managed service: `run_sync_service`, `SyncServiceConfig`, `SyncServiceOutcome`.
  - Consensus bridge: `shelley_opcert_to_consensus`, `shelley_header_to_consensus`, `verify_shelley_header`.
  - Multi-era decode: `MultiEraBlock`, `decode_multi_era_block`, `decode_multi_era_blocks` (Byron/Shelley/Allegra/Mary/Alonzo/Babbage/Conway — all seven era tags).
  - Header hash: `ShelleyHeader::header_hash` (Blake2b-256), `compute_tx_id`.
  - Verified pipeline: `multi_era_block_to_block`, `verify_multi_era_block`, `sync_step_multi_era`, `sync_batch_apply_verified`, `VerificationConfig`.
  - Mempool eviction: `extract_tx_ids`, `evict_confirmed_from_mempool`.
- `crates/mempool` now includes fee-ordered queue with `TxId`-based entries, duplicate detection, capacity enforcement, `remove_by_id`, `remove_confirmed` for block-application eviction, TTL-aware admission (`insert_checked`, `purge_expired`), and iterator support.
- `crates/ledger` now includes `LedgerState` with `ShelleyUtxo` integration, atomic block application, Allegra era types (`AllegraTxBody`, `NativeScript`), Mary era types (`Value`, `MultiAsset`, `MaryTxBody`), Alonzo era types (`ExUnits`, `Redeemer`, `AlonzoTxOut`, `AlonzoTxBody`), Byron envelope (`ByronBlock`), Babbage era types (`DatumOption`, `BabbageTxOut`, `BabbageTxBody`, `BabbageBlock`), Conway era types (`Vote`, `Voter`, `GovActionId`, `Anchor`, `VotingProcedure`, `ProposalProcedure`, `VotingProcedures`, `ConwayTxBody`, `ConwayBlock`), and signed integer CBOR helpers. Full era type and block coverage from Byron through Conway is complete.
- New subfolder-level AGENTS.md files should only be added where a folder has a stable domain boundary.

Refer to and update `docs/ARCHITECTURE.md`, `docs/DEPENDENCIES.md`, `docs/SPECS.md`, and `docs/CONTRIBUTING.md` for project policy and workflow details and keep `./README.md` updated.
