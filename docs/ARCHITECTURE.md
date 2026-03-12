# Architecture

Yggdrasil is organized as a Rust workspace with explicit crate boundaries so protocol, ledger, storage, and integration work can evolve independently.

## Crate Topology
- `crates/crypto`: hashing, signatures, VRF, KES, and cryptographic encoding boundaries.
- `crates/cddl-codegen`: parsing pinned Cardano specifications and generating Rust types and CBOR helpers.
- `crates/ledger`: transaction and block state transitions plus era-aware domain modeling.
- `crates/storage`: immutable storage, rollback-aware volatile storage, and snapshot facilities.
- `crates/consensus`: chain selection, leader election, epoch math, and rollback coordination.
- `crates/mempool`: transaction admission, prioritization, and block-application eviction.
- `crates/network`: handshake, mini-protocol state machines, peer management, and multiplexing.
- `node`: runtime wiring, CLI, sync loop, and operational entry points.

## Dependency Order
1. `crypto`
2. `cddl-codegen`
3. `ledger` and `storage`
4. `consensus` and `mempool`
5. `network`
6. `node`

## Design Principles
- Keep public interfaces small and spec-traceable.
- Separate generated types from handwritten state-machine logic.
- Let storage and network depend on stable domain interfaces rather than concrete implementations.
- Build parity tooling alongside implementation rather than as a final afterthought.

## First Milestone
The first executable milestone is foundation only: a buildable workspace, clear documentation, crate skeletons, and CI. Functional protocol parity is deferred to later milestones.
