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
- `node` owns orchestration, CLI, and runtime integration.

## Upstream References
- `crates/crypto`: <https://github.com/IntersectMBO/cardano-base/tree/master/cardano-crypto-class> and <https://github.com/IntersectMBO/cardano-base/tree/master/cardano-crypto-praos>
- `crates/cddl-codegen`: <https://github.com/IntersectMBO/cardano-ledger/tree/master/eras> and <https://github.com/IntersectMBO/cardano-ledger/tree/master/libs/cardano-ledger-binary>
- `crates/ledger`: <https://github.com/IntersectMBO/cardano-ledger> and <https://github.com/IntersectMBO/formal-ledger-specifications>
- `crates/storage`: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus>
- `crates/consensus`: <https://github.com/IntersectMBO/ouroboros-consensus> and <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/docs/agda-spec>
- `crates/mempool`: <https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus> and <https://github.com/IntersectMBO/cardano-node/tree/master/cardano-submit-api>
- `crates/network`: <https://github.com/IntersectMBO/ouroboros-network> and <https://ouroboros-network.cardano.intersectmbo.org/pdfs/network-spec>
- `node`: <https://github.com/IntersectMBO/cardano-node> and <https://github.com/IntersectMBO/cardano-node/tree/master/configuration>

## Working Rules
- Keep `**/AGENTS.md` files updated and focused on operational guidance, not long-form documentation.
- Add new dependencies only when they are justified in `docs/DEPENDENCIES.md`.
- Do not add FFI-backed cryptography or hidden native dependencies.
- Keep generated artifacts reproducible and avoid editing generated code by hand.
- Prefer incremental milestones that compile and test cleanly.

## Verification Expectations
- `cargo check-all`
- `cargo test-all`
- `cargo lint`

## Current Phase
- Workspace foundation is complete and compileable.
- Active implementation work has started in `crates/crypto` and `crates/cddl-codegen`.
- New subfolder-level AGENTS.md files should only be added where a folder has a stable domain boundary.

Refer to `docs/ARCHITECTURE.md`, `docs/DEPENDENCIES.md`, `docs/SPECS.md`, and `docs/CONTRIBUTING.md` for project policy and workflow details.
