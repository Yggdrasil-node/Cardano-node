# Architecture

Yggdrasil is organized as a Rust workspace with explicit crate boundaries so protocol, ledger, storage, and integration work can evolve independently.

## Crate Topology
- `crates/crypto`: hashing, signatures, VRF, KES, and cryptographic encoding boundaries.
- `crates/cddl-codegen`: parsing pinned Cardano specifications and generating Rust types and CBOR helpers.
- `crates/ledger`: transaction and block state transitions plus era-aware domain modeling.
- `crates/storage`: immutable storage, rollback-aware volatile storage, and snapshot facilities.
- `crates/consensus`: chain selection, leader election, epoch math, and rollback coordination.
- `crates/mempool`: transaction admission, prioritization, and block-application eviction.
- `crates/network`: handshake, mini-protocol state machines, peer management, peer candidate ordering, and multiplexing.
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
- Keep `node` as an orchestration layer: configuration loading, CLI overrides, runtime startup, and shutdown belong there, but reusable peer policy, tracer transports, and protocol-facing state machines belong in crates.
- Extract logic out of `node` when any of these become true: the code is reused by more than one runtime path, it owns non-trivial protocol or peer-selection state, or it would need independent tests that do not depend on the CLI/runtime entrypoint.

## Current Milestone
The project has a complete Cardano-era type system and a functional node binary:
- Full era type coverage from Byron through Conway with typed CBOR codecs.
- Multi-era UTxO validation with coin and multi-asset preservation checks.
- Network transport + mux + handshake + peer lifecycle with all four mini-protocol state machines, wire codecs, and typed client drivers.
- Reusable peer candidate ordering, bootstrap-target sequencing, reconnect attempt ordering, and preferred-peer retry state now live in `crates/network`; `node` only parses the upstream topology/config shape and feeds those helpers into runtime startup.
- Multi-era block decode (all 7 era tags) with consensus header verification (KES/OpCert).
- Node binary with `clap` CLI (`run` + `default-config`), JSON configuration, upstream-aligned tracing config fields, local runtime trace emission, and managed sync service with graceful shutdown.
- Mempool with TTL-aware admission, fee ordering, and block-application eviction.
- File-backed storage implementations behind `ImmutableStore`, `VolatileStore`, and `LedgerStore` traits.
- Consensus hardening with `SecurityParam`, `ChainState` volatile chain tracker, rollback depth enforcement, and stability window detection.

Upstream parity testing is complete with CBOR golden round-trip tests and cross-subsystem integration tests. Wire-format field names align with official Cardano CDDL schemas. 640 workspace tests pass across all crates.

The next architecture milestones are end-to-end multi-peer management, dedicated tracer transport/metrics export, and mainnet sync endurance testing.

Topology parsing and preset-specific config resolution currently stay in `node` because they are operational concerns tied to the node binary's config format. Once peer selection grows into ledger peers, peer sharing, or long-lived governor policy, that logic should move behind a network-crate boundary rather than continuing to grow in `node`.
