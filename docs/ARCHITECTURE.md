# Architecture

Yggdrasil is organized as a Rust workspace with explicit crate boundaries so protocol, ledger, storage, and integration work can evolve independently.

## Crate Topology
- `crates/crypto`: hashing, signatures, VRF, KES, and cryptographic encoding boundaries.
- `crates/cddl-codegen`: parsing pinned Cardano specifications and generating Rust types and CBOR helpers.
- `crates/ledger`: transaction and block state transitions plus era-aware domain modeling.
- `crates/storage`: immutable storage, rollback-aware volatile storage, ledger snapshot facilities, and a minimal ChainDB-style coordination layer.
- `crates/consensus`: chain selection, leader election, epoch math, and rollback coordination.
- `crates/mempool`: transaction admission, prioritization, and block-application eviction.
- `crates/network`: handshake, mini-protocol state machines, peer management, topology domain types, root-provider snapshots, peer registry state, peer candidate ordering, and multiplexing.
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
- Reusable topology domain types, topology-root configuration parsing, root-provider snapshots, peer registry state, peer candidate ordering, bootstrap-target sequencing, reconnect attempt ordering, and preferred-peer retry state now live in `crates/network`; `node` only feeds those helpers into runtime startup.
- Multi-era block decode (all 7 era tags) with consensus header verification (KES/OpCert).
- Node binary with `clap` CLI (`run` + `default-config`), JSON configuration, upstream-aligned tracing config fields, local runtime trace emission, and managed sync service with graceful shutdown.
- Mempool with TTL-aware admission, fee ordering, and block-application eviction.
- File-backed storage implementations behind `ImmutableStore`, `VolatileStore`, and `LedgerStore` traits.
- Storage crate now also exposes a minimal `ChainDb` coordination layer for best-known tip recovery, volatile-prefix promotion into immutable storage, and rollback-time snapshot truncation without moving sync policy into `node`.
- Consensus hardening with `SecurityParam`, `ChainState` volatile chain tracker, rollback depth enforcement, and stability window detection.

Upstream parity testing is complete with CBOR golden round-trip tests and cross-subsystem integration tests. Wire-format field names align with official Cardano CDDL schemas. 640 workspace tests pass across all crates.

The next architecture milestones are end-to-end multi-peer management, dedicated tracer transport/metrics export, and mainnet sync endurance testing.

Topology parsing and preset-specific config resolution currently stay in `node` because they are operational concerns tied to the node binary's config format. Once peer selection grows into ledger peers, peer sharing, or long-lived governor policy, that logic should move behind a network-crate boundary rather than continuing to grow in `node`.

## Upstream-Aligned Networking Plan
- Phase 1: topology-model parity in `crates/network` is complete. Local and public root topology types now live in `yggdrasil-network`, with local-root support for `hotValency`, `warmValency`, `diffusionMode`, trustability, legacy `valency` compatibility, and upstream-style `useBootstrapPeers` and `useLedgerPeers` semantics.
- Phase 2: root-set providers in `crates/network` is complete. The crate now exposes a resolved startup snapshot, mutable root-provider state, a refresh-oriented provider API for local, bootstrap, and public roots with disjointness and precedence handling, and a DNS-backed provider that covers local roots, bootstrap peers, and configured public roots. An optional `DnsRefreshPolicy` adds time-gated re-resolution with exponential backoff on stale results (upstream-aligned 60 s base / 900 s max). That refresh path can also reconcile the peer registry directly.
- Phase 3: peer registry state in `crates/network` is complete. The crate now exposes a minimal registry for peer source and status aligned with upstream `PeerSource` and `PeerStatus` concepts, including local root, public root, bootstrap, ledger, big-ledger, and peer-share origins plus cold, cooling, warm, and hot states. Root-provider refreshes already reconcile through this registry, and the crate now also exposes set-reconciliation helpers for ledger, big-ledger, and peer-share inputs so `node` does not need to hand-roll source bookkeeping. Ledger peer provider layer is complete: `LedgerPeerProvider` trait, `LedgerPeerSnapshot` normalization (deduplicates and enforces disjoint ledger/big-ledger sets), `LedgerPeerProviderRefresh` (combined/per-kind), `apply_ledger_peer_refresh()` helper, `refresh_ledger_peer_registry()` orchestration, and `ScriptedLedgerPeerProvider` for testing. Provider refreshes reconcile the `PeerRegistry` on crate-owned paths without node involvement.
- Phase 4: consensus-network bridge for ledger peers is in progress. The network crate now owns ledger-peer eligibility judgement helpers for `useLedgerAfterSlot`, latest-slot gating, ledger-state judgement, and peer-snapshot freshness, while the remaining work is to feed immutable-ledger-derived peers and real freshness signals from consensus/storage into that path.
- Phase 5: governor-style policy. Only after the previous phases exist should Yggdrasil add promotion, demotion, peer sharing, public-root refresh backoff, churn, or Genesis-specific security behavior. The implementation should keep policy separate from mechanism, as in upstream `PeerSelectionActions`, `PeerSelectionPolicy`, and governor state modules.

## Planning Constraints
- Prefer the official type split over local simplifications. Upstream distinguishes local root groups from public roots, public roots from bootstrap peers, and ledger peers from all configured root sets.
- Keep the dynamic parts asynchronous. Upstream treats local roots, public roots, ledger peers, and snapshot data as time-varying sources observed by the networking layer rather than one-shot startup inputs.
- Preserve root-set invariants. Official peer-selection state enforces that local roots and public roots do not overlap and that root counts respect peer-selection targets.
- Keep `node` focused on orchestration. It should provide config loading, CLI overrides, and consensus-facing signals, but the network crate should own peer sources, peer state, retry policy, and future governor behavior.
