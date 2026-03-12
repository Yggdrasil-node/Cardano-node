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

## Non-Negotiable Rules
- `**/AGENTS.md` files MUST stay current and MUST remain operational rather than long-form documentation.
- New dependencies MUST be justified in `docs/DEPENDENCIES.md` before they are treated as accepted.
- FFI-backed cryptography and hidden native dependencies MUST NOT be introduced.
- Generated artifacts MUST remain reproducible and generated code MUST NOT be edited by hand.
- Implementation work MUST favor incremental milestones that compile and test cleanly.
- Public modules, types, and functions MUST have proper Rustdocs whenever behavior is non-obvious or externally consumed.
- Explanations of behavior or naming MUST be cross-checked against the official `cardano-node` and the relevant upstream IntersectMBO repositories.
- Type and function naming MUST stay as close to upstream terminology as practical so parity work and fixture comparison remain tractable.
- Cryptographic, protocol, and serialization parity with the official node is a non-negotiable long-term target even when an implementation slice is still incomplete.

## Verification Expectations
- `cargo check-all`
- `cargo test-all`
- `cargo lint`

## Current Phase
- Workspace foundation is complete and compileable.
- Active implementation work has started in `crates/crypto` and `crates/cddl-codegen`.
- New subfolder-level AGENTS.md files should only be added where a folder has a stable domain boundary.

Refer to `docs/ARCHITECTURE.md`, `docs/DEPENDENCIES.md`, `docs/SPECS.md`, and `docs/CONTRIBUTING.md` for project policy and workflow details.
