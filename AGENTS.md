---
name: cardano-rust-node
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

## Working Rules
- Keep AGENTS.md files focused on operational guidance, not long-form documentation.
- Add new dependencies only when they are justified in `docs/DEPENDENCIES.md`.
- Do not add FFI-backed cryptography or hidden native dependencies.
- Keep generated artifacts reproducible and avoid editing generated code by hand.
- Prefer incremental milestones that compile and test cleanly.

## Verification Expectations
- `cargo check-all`
- `cargo test-all`
- `cargo lint`

Refer to `docs/ARCHITECTURE.md`, `docs/DEPENDENCIES.md`, `docs/SPECS.md`, and `docs/CONTRIBUTING.md` for project policy and workflow details.
