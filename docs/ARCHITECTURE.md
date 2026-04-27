---
title: Architecture
layout: default
parent: Reference
nav_order: 1
---

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
The project has a complete Cardano-era type system, a functional node binary, and a fully-tested multi-peer dispatch layer:
- Full era type coverage from Byron through Conway with typed CBOR codecs.
- Multi-era UTxO validation with coin and multi-asset preservation checks.
- Network transport + mux + handshake + peer lifecycle with all five mini-protocol state machines, wire codecs, and typed client/server drivers.
- Reusable topology domain types, topology-root configuration parsing, root-provider snapshots, peer registry state, peer candidate ordering, bootstrap-target sequencing, reconnect attempt ordering, and preferred-peer retry state now live in `crates/network`; `node` only feeds those helpers into runtime startup.
- Multi-era block decode (all 7 era tags) with consensus header verification (KES/OpCert).
- Node binary with `clap` CLI (`run`, `validate-config`, `status`, `default-config`), JSON configuration, upstream-aligned tracing config fields, local runtime trace emission, and managed sync service with graceful shutdown.
- Epoch-boundary wiring in runtime sync paths now includes real per-pool performance inputs for rewards and Shelley PPUP application at epoch transition.
- Mempool with TTL-aware admission, fee ordering, and block-application eviction.
- File-backed storage implementations behind `ImmutableStore`, `VolatileStore`, and `LedgerStore` traits.
- Storage crate now also exposes a minimal `ChainDb` coordination layer for best-known tip recovery, typed ledger checkpoint restore and replay, checkpoint retention/truncation, volatile-prefix promotion into immutable storage, and rollback-time snapshot truncation without moving sync policy into `node`.
- Consensus hardening with `SecurityParam`, `ChainState` volatile chain tracker, rollback depth enforcement, and stability window detection.

### Live runtime data flows (2026-Q2 audit closure)

End-to-end wired and tested between the consensus, network, and node crates:

- **Genesis density signal** — ChainSync `RollForward` pushes per-peer `slot` observations into `crates/consensus::DensityWindow`; the runtime `node::sync::DensityRegistry` aggregates per peer; the `crates/network::governor::run_governor_loop` reads density into `PeerMetrics.density` before each tick; `combined_score` adds a `HIGH_DENSITY_BONUS` for peers above `LOW_DENSITY_THRESHOLD = 0.6`, biasing hot demotion toward laggards.
- **Hot-peer scheduling weights** — `crates/network::governor::HotPeerScheduling` carries upstream-default per-`MiniProtocolNum` weights (BlockFetch=10, ChainSync=3, TxSubmission=2, KeepAlive=1, PeerSharing=1); `node::runtime::apply_hot_weights` reads from the table on every promote-to-hot call so operator overrides via `set_hot_protocol_weight` land at the next promotion; the mux writer's per-round weighted round-robin reads `WeightHandle` atomically each round so updates take effect immediately.
- **Multi-peer BlockFetch dispatch primitives** — `node::sync::partition_fetch_range_across_peers` translates the `max_concurrent_block_fetch_peers` config knob into per-peer `BlockFetchAssignment`s using `crates/network::blockfetch_pool::split_range`; `node::sync::execute_multi_peer_blockfetch_plan<B>` dispatches assignments concurrently via `tokio::JoinSet`, propagates errors with `abort_all`, and reassembles chunks in chain order via `ReorderBuffer<B>`; `node::sync::dispatch_range_with_tentative` wraps the above in the consensus-correctness contract (announce `try_set_tentative_header` before dispatch, call `clear_tentative_trap` on any chunk failure). The dispatcher itself stays tentative-state-agnostic so async tasks cannot race on mutation — the consensus boundary lives in the single layer.

The 2026-Q2 audit ([`docs/AUDIT_VERIFICATION_2026Q2.md`](AUDIT_VERIFICATION_2026Q2.md)) closed every confirmed-active parity slice plus the runtime integrations originally tracked as follow-ups (`+117` cycle delta plus E-Phase6-Seam, E-Inline, E-Workers, E-Production-Spawn, E-Migration, E-Wire, E-Promote, E-Runbook, and Phase 6 observability).  The 2026-Q3 operational pass on `main` then closed the [audit C-1/H-1/H-2/M-1..M-8/L-1..L-9](code-audit.md) findings and surfaced + fixed the byron→shelley fee-validation parity bug at preprod slot 518 460 (see [`docs/REAL_PREPROD_POOL_VERIFICATION.md`](REAL_PREPROD_POOL_VERIFICATION.md)).  Live workspace test count: **4,634** passing, 0 failing.

Upstream parity testing is complete with CBOR golden round-trip tests and cross-subsystem integration tests. Wire-format field names align with official Cardano CDDL schemas.

The remaining production-readiness gate is operator-side: the parallel-fetch rehearsal (`docs/MANUAL_TEST_RUNBOOK.md` §6.5) and the mainnet sync endurance run (§2–9), both of which require wallclock time and operator judgement before the default `max_concurrent_block_fetch_peers` knob flips above 1.

Topology parsing and preset-specific config resolution currently stay in `node` because they are operational concerns tied to the node binary's config format. Once peer selection grows into ledger peers, peer sharing, or long-lived governor policy, that logic should move behind a network-crate boundary rather than continuing to grow in `node`.

## Upstream-Aligned Networking Plan
- Phase 1: topology-model parity in `crates/network` is complete. Local and public root topology types now live in `yggdrasil-network`, with local-root support for `hotValency`, `warmValency`, `diffusionMode`, trustability, legacy `valency` compatibility, and upstream-style `useBootstrapPeers` and `useLedgerPeers` semantics.
- Phase 2: root-set providers in `crates/network` is complete. The crate now exposes a resolved startup snapshot, mutable root-provider state, a refresh-oriented provider API for local, bootstrap, and public roots with disjointness and precedence handling, and a DNS-backed provider that covers local roots, bootstrap peers, and configured public roots. An optional `DnsRefreshPolicy` adds time-gated re-resolution with exponential backoff on stale results (upstream-aligned 60 s base / 900 s max). That refresh path can also reconcile the peer registry directly.
- Phase 3: peer registry state in `crates/network` is complete. The crate now exposes a minimal registry for peer source and status aligned with upstream `PeerSource` and `PeerStatus` concepts, including local root, public root, bootstrap, ledger, big-ledger, and peer-share origins plus cold, cooling, warm, and hot states. Root-provider refreshes already reconcile through this registry, and the crate now also exposes set-reconciliation helpers for ledger, big-ledger, and peer-share inputs so `node` does not need to hand-roll source bookkeeping. Ledger peer provider layer is complete: `LedgerPeerProvider` trait, `LedgerPeerSnapshot` normalization (deduplicates and enforces disjoint ledger/big-ledger sets), `LedgerPeerProviderRefresh` (combined/per-kind), `apply_ledger_peer_refresh()` helper, `refresh_ledger_peer_registry()` orchestration, and `ScriptedLedgerPeerProvider` for testing. Provider refreshes reconcile the `PeerRegistry` on crate-owned paths without node involvement.
- Phase 4: consensus-network bridge for ledger peers is complete. The network crate owns the live orchestration seam (`live_refresh_ledger_peer_registry_observed`) and applies policy reconciliation from consensus-fed `(latest_slot, judgement, ledger_snapshot)` plus snapshot-file observations. Node runtime now provides storage-backed source adapters only, and consumes the same observed judgement returned by the network orchestration for governor mode/churn decisions, removing duplicate node-side ledger judgement derivation while preserving startup/reconnect ledger-peer refresh behavior.
- Phase 5: governor-style policy. Only after the previous phases exist should Yggdrasil add promotion, demotion, peer sharing, public-root refresh backoff, churn, or Genesis-specific security behavior. The implementation should keep policy separate from mechanism, as in upstream `PeerSelectionActions`, `PeerSelectionPolicy`, and governor state modules. **Status: complete.** Promotion/demotion logic, peer sharing, public-root and big-ledger backoff, two-phase churn cycle, sensitive/normal mode, association mode, hot-peer scheduling, and density-biased demotion all live in `crates/network::governor`.
- Phase 6: runtime multi-session orchestration. **Status: complete.** End-to-end multi-peer concurrent BlockFetch is wired and the operator activates it by setting `max_concurrent_block_fetch_peers > 1` (default remains `1` pending §6.5 rehearsal sign-off).  The consensus-correctness contract is locked in `dispatch_range_with_tentative` and tested (announce → dispatch → clear-trap-on-failure).

  1. **Warm-peer BlockFetch handle accessor.** **Done** (Slice E-Phase6-Seam, commit `5d44c70`). `OutboundPeerManager::with_hot_block_fetch_clients<R>(&mut self, f: FnOnce(&mut [(SocketAddr, &mut BlockFetchClient)]) -> R) -> R` exposes hot peers' BlockFetch handles as a borrow-checked slice; `hot_peer_addrs()` is the cheap snapshot for sizing concurrency.

  2. **Sync-loop dispatch consumer.** **Done** (Slice E-Wire, commit `9f87447`). `sync_batch_verified_with_tentative` accepts `block_fetch: Option<&mut BlockFetchClient>` plus an optional `MultiPeerDispatchContext { pool: SharedFetchWorkerPool, max_concurrent_knob }`.  When `Some` AND `effective_block_fetch_concurrency(workers, knob) > 1`, the per-RollForward fetch step reads the shared pool under a brief `tokio::sync::RwLock::read` guard, partitions the range via `partition_fetch_range_across_peers`, calls `pool.dispatch_plan(...)`, and clears the tentative trap on error; otherwise the legacy single-peer path runs unchanged.

  3. **Async-borrow lifetime constraint.** **Done** (Slice E-Workers, commit `434af60`; production spawn `cafc31a`; migration `0f612aa` + `7c06baf`). Resolved by per-peer worker tasks: each peer's `BlockFetchClient` is owned by its own tokio task that drains a `mpsc::Receiver<FetchRequest>` queue. The sync loop dispatches via `mpsc::Sender::send` (no `&mut BlockFetchClient` borrow crosses the await), and per-request `oneshot::Sender` returns the result. Workers run in parallel because each is its own task. Mirrors upstream `BlockFetch.ClientRegistry` per-peer `FetchClientStateVars` + STM exactly. `PeerSession.block_fetch: Option<BlockFetchClient>` plus `take_block_fetch()` lets the runtime move the client out into a worker without dropping the session.

  4. **Connection-manager coordination.** **Done** (Slice E-Migration `0f612aa` + Slice E-Promote `1249f7f`). The governor's `evaluate_hot_promotions` produces N promotions per tick; `apply_cm_actions` calls `OutboundPeerManager::migrate_session_to_worker(peer)` after successful `promote_to_warm` when `max_concurrent_block_fetch_peers > 1`, emitting a `Net.BlockFetch.Worker` info trace.  On peer disconnect, the now-async `demote_to_cold` calls `unregister_worker(peer)` to remove the worker and `prune_closed()` to GC dead workers.  On reconnect, the next promote re-spawns a worker via `FetchWorkerHandle::spawn_with_block_fetch_client`.  The dispatcher's error-propagation path (drop pending oneshot receivers, propagate first error) is correct for mid-fetch peer loss — surviving workers stay alive for subsequent iterations; only the offending request's response is lost.

  5. **Operational rollout.** Default `max_concurrent_block_fetch_peers = 1` keeps the legacy path active and the change is purely additive.  Operators opt in by setting the knob `> 1` after running the §6.5 parallel-fetch rehearsal in [`docs/MANUAL_TEST_RUNBOOK.md`](MANUAL_TEST_RUNBOOK.md) (steps 6.5a–6.5f cover 2- and 4-peer soak with hash-comparison vs. Haskell node and restart-resilience cycles).  Phase 6 observability (`yggdrasil_blockfetch_workers_registered` gauge + `_migrated_total` counter, commit `b3a6080`) gives operator dashboards the instrumentation needed to alert on stuck migration.  Upstream `bfcMaxConcurrencyBulkSync = 2` is the natural default for syncing nodes.

  Reference: upstream `Ouroboros.Network.BlockFetch.ClientRegistry` (per-peer `FetchClientStateVars`) + `Ouroboros.Network.BlockFetch.Decision.fetchDecisions` + `Ouroboros.Network.BlockFetch.State.completeBlockDownload`.

## Planning Constraints
- Prefer the official type split over local simplifications. Upstream distinguishes local root groups from public roots, public roots from bootstrap peers, and ledger peers from all configured root sets.
- Keep the dynamic parts asynchronous. Upstream treats local roots, public roots, ledger peers, and snapshot data as time-varying sources observed by the networking layer rather than one-shot startup inputs.
- Preserve root-set invariants. Official peer-selection state enforces that local roots and public roots do not overlap and that root counts respect peer-selection targets.
- Keep `node` focused on orchestration. It should provide config loading, CLI overrides, and consensus-facing signals, but the network crate should own peer sources, peer state, retry policy, and future governor behavior.
