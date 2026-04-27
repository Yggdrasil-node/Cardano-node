# Changelog

All notable changes to Yggdrasil are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Documentation site published at <https://yggdrasil-node.github.io/Cardano-node/>
  with operator user manual (12 chapters) and reference docs.
- Release workflow that builds Linux x86_64 + aarch64 binaries on tag push,
  computes SHA256 checksums, and publishes a GitHub Release.
- Issue templates, PR template, CODEOWNERS, dependabot config.
- `Dockerfile` + `.dockerignore` for container deployments.
- Operator scripts: `install_from_release.sh`, systemd unit template.
- `SECURITY.md` with vulnerability disclosure policy.

### Fixed

- Operator-facing Prometheus metric names corrected across the manual,
  runbook, healthcheck, restart-resilience and pool-producer scripts. The
  emitted names (`yggdrasil_current_block_number`, `yggdrasil_reconnects`,
  `yggdrasil_rollbacks`, `yggdrasil_stable_blocks_promoted`,
  `yggdrasil_batches_completed`, `yggdrasil_mempool_tx_added`,
  `yggdrasil_mempool_tx_rejected`, `yggdrasil_inbound_connections_accepted`,
  `yggdrasil_inbound_connections_rejected`, `yggdrasil_active_peers`,
  `yggdrasil_blocks_synced`, `yggdrasil_current_slot`) are now what the
  docs and scripts reference. Prior `*_total` / legacy aliases were stale
  and would have returned no series on a real scrape.
- `node/scripts/install_from_release.sh` now prints actionable
  build-from-source instructions (instead of a bare error) when no
  GitHub release has been published yet.
- README install section reordered for the pre-1.0 window: source build
  and Docker shown first, release tarball noted as gated on the first
  `v*` tag.

### Changed

- Per-crate `AGENTS.md` files refreshed for the closure-cycle runtime
  integrations (consensus density seam, network density signal, node
  multi-peer dispatch + worker pool).

### Documentation

- ARCHITECTURE.md Phase 6 marked complete.
- Test count refreshed to 4,630 across user-facing surfaces.
- Removed stale "follow-up runtime integration" / "future-milestone" phrasing.

## [0.1.0] — 2026-Q2

### Yggdrasil 1.0 closure

First feature-complete release after the 2026-Q2 parity audit. Every
confirmed-active parity slice is closed; every runtime integration
originally tracked as a follow-up has landed.

### Added — closure cycle slices

- **Slice B** — CDDL parser range constraints (`N..M`, `.le`, `.ge`,
  `.lt`, `.gt`, `.size N..M`).
- **Slice D** — `HotPeerScheduling` per-mini-protocol weight table
  mirroring upstream `Ouroboros.Network.PeerSelection.Governor.HotPeers`.
- **Slice E (foundation)** — `effective_block_fetch_concurrency` +
  `partition_fetch_range_across_peers` + `BlockFetchAssignment`
  primitives.
- **Slice GD** — genesis density tracking primitive
  (`crates/consensus/src/genesis_density.rs::DensityWindow`,
  `DEFAULT_SLOT_WINDOW = 6480`, `DEFAULT_LOW_DENSITY_THRESHOLD = 0.6`).
- **Slice GD-RT** — ChainSync header density observation hook
  (`DensityRegistry`).
- **Slice GD-Governor** — density-biased hot demotion in `PeerMetrics`.
- **Slice GD-Final** — runtime data flow unifying the density seam.
- **Slice D-Scheduler** — `HotPeerScheduling`-driven mux egress weights.
- **Slice E-Dispatch** — `execute_multi_peer_blockfetch_plan`
  parallel executor with `tokio::JoinSet` + `ReorderBuffer`.
- **Slice E-Tentative** — `dispatch_range_with_tentative` consensus-
  correctness contract.
- **Slice E-Phase6-Seam** — `OutboundPeerManager` hot-peer accessors.
- **Slice E-Inline** — non-spawning multi-peer dispatcher
  (`execute_multi_peer_blockfetch_plan_inline`).
- **Slice E-Workers** — per-peer fetch worker primitive
  (`FetchWorkerHandle`, `FetchWorkerPool`) mirroring upstream
  `Ouroboros.Network.BlockFetch.ClientRegistry`.
- **Slice E-Production-Spawn** —
  `FetchWorkerHandle::spawn_with_block_fetch_client` wiring real
  `BlockFetchClient` into a worker.
- **Slice E-Migration** — `PeerSession.block_fetch: Option<...>` plus
  `migrate_session_to_worker` / `unregister_worker`.
- **Slice E-Wire** — sync-loop multi-peer dispatch branch +
  `MultiPeerDispatchContext`.
- **Slice E-Promote** — governor migrates `BlockFetchClient` on
  `promote_to_warm` when the operator knob is `> 1`.
- **Phase 6 observability** — Prometheus counters
  `yggdrasil_blockfetch_workers_registered` (gauge) and
  `yggdrasil_blockfetch_workers_migrated_total` (counter).

### Operator surface

- `max_concurrent_block_fetch_peers` config knob (default `1`,
  flippable to `2` after §6.5 rehearsal).
- §6.5 parallel-fetch rehearsal added to the manual test runbook.

### Test count

- 4,630 tests passing across the workspace, 0 failing.
- All four gates clean: `cargo check-all`, `cargo test-all`,
  `cargo lint`, `cargo doc --workspace --no-deps`.

[Unreleased]: https://github.com/yggdrasil-node/Cardano-node/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/yggdrasil-node/Cardano-node/releases/tag/v0.1.0
