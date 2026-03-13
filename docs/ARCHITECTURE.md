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

## Current Milestone
The project is past pure scaffolding and now includes a working protocol/runtime foundation:
- Network transport + mux + handshake + peer lifecycle are implemented.
- All four current mini-protocols have state machines, wire codecs, and typed client drivers.
- Node bootstrap and first sync orchestration helpers are implemented.
- First typed decode bridge is implemented for Shelley blocks fetched via BlockFetch.

The next architecture milestone is broad typed payload flow from network protocol messages into ledger/storage boundaries, then staged consensus integration on top.
