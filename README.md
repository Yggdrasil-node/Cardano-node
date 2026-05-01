<p align="center">
  <img src="docs/assets/images/Yggrasil_banner.png" alt="YggdrasilNode — A Cardano Node Project Written In Rust" width="100%"/>
</p>

# Yggdrasil Cardano Node in Rust

[![CI](https://github.com/yggdrasil-node/Cardano-node/actions/workflows/ci.yml/badge.svg)](https://github.com/yggdrasil-node/Cardano-node/actions/workflows/ci.yml)
[![Pages](https://github.com/yggdrasil-node/Cardano-node/actions/workflows/pages.yml/badge.svg)](https://github.com/yggdrasil-node/Cardano-node/actions/workflows/pages.yml)
[![Release](https://github.com/yggdrasil-node/Cardano-node/actions/workflows/release.yml/badge.svg)](https://github.com/yggdrasil-node/Cardano-node/actions/workflows/release.yml)
[![Latest release](https://img.shields.io/github/v/release/yggdrasil-node/Cardano-node?include_prereleases&sort=semver)](https://github.com/yggdrasil-node/Cardano-node/releases/latest)
[![Rust 1.95.0](https://img.shields.io/badge/rust-1.95.0-orange)](rust-toolchain.toml)
[![Tests](https://img.shields.io/badge/tests-4640%20passing-brightgreen)](#current-status)

Yggdrasil is a pure Rust Cardano node workspace targeting long-term protocol and serialization parity with the upstream Cardano node.

**Documentation**: <https://yggdrasil-node.github.io/Cardano-node/> · [User Manual](https://yggdrasil-node.github.io/Cardano-node/manual/) · [Quick Start](https://yggdrasil-node.github.io/Cardano-node/manual/quick-start/) · [Releases](https://github.com/yggdrasil-node/Cardano-node/releases)

## Quick Navigation

- [Install](#install)
- [Current Status](#current-status)
- [Workspace Layout](#workspace-layout)
- [Verification](#verification)
- [Documentation](#documentation)
- [Roadmap (post-1.0)](#roadmap-post-10)

## Install

> **Pre-1.0 status:** prebuilt release tarballs are published once the project
> tags a `v*` release. Until then, the source build (under one minute on a
> warm cache) and the Docker path are the supported install routes.

**From source (recommended for testing):**

```bash
git clone https://github.com/yggdrasil-node/Cardano-node.git yggdrasil
cd yggdrasil
cargo build --release --bin yggdrasil-node
sudo install -m 0755 target/release/yggdrasil-node /usr/local/bin/
yggdrasil-node validate-config --network mainnet --database-path /var/lib/yggdrasil/db
```

**Docker:**

```bash
git clone https://github.com/yggdrasil-node/Cardano-node && cd Cardano-node
docker compose up -d
docker compose logs -f
```

**From a published release tarball (Linux x86_64 / aarch64) — once tagged:**

```bash
curl -fsSL https://raw.githubusercontent.com/yggdrasil-node/Cardano-node/main/node/scripts/install_from_release.sh | bash
```

The installer falls back to clear instructions when no release has been
published yet. Full details: [Installation](https://yggdrasil-node.github.io/Cardano-node/manual/installation/).

## Current Status

### Implemented

- Cargo workspace with stable crate boundaries for crypto, cddl-codegen, ledger, storage, consensus, mempool, network, and node integration.
- **Crypto**: Blake2b-256/512, Ed25519, VRF (standard + batchcompat), SimpleKES, SumKES (depth 0–6+), with upstream vector-backed coverage and zeroize hardening.
- **Ledger**: Full era type coverage Byron through Conway. Hand-rolled CBOR codec. Multi-era UTxO (`MultiEraUtxo`, `MultiEraTxOut`) with era-aware `apply_block()` dispatch, coin/multi-asset preservation, TTL/validity-interval checks. PlutusData AST with full CBOR support. Certificate hierarchy (19 variants). Credential, address, and governance types. Epoch-boundary processing includes stake-snapshot rotation, reward distribution, governance ratification/enactment, and Shelley PPUP protocol-parameter update application with genesis-delegate quorum.
- **Network**: SDU framing, async bearer transport, mux/demux, handshake, peer lifecycle, reusable peer candidate ordering, upstream-aligned topology domain types for local and public roots, a root-provider layer that resolves, tracks, and refreshes local, bootstrap, and public roots with upstream-style precedence, a DNS-backed provider for local roots, bootstrap peers, and configured public roots with optional time-gated refresh policy (exponential backoff, upstream-aligned 60 s / 900 s clamps), a minimal peer registry that tracks peer source and cold/cooling/warm/hot status in the crate instead of `node`, including crate-owned reconciliation helpers for root, ledger, big-ledger, and peer-share source sets, and a ledger peer provider layer with `LedgerPeerProvider` trait, `LedgerPeerSnapshot` normalization (deduplicates and enforces disjoint ledger/big-ledger sets), `LedgerPeerProviderRefresh` (combined/per-kind), `apply_ledger_peer_refresh()` helper, `refresh_ledger_peer_registry()` orchestration, and `ScriptedLedgerPeerProvider` for testing. All five mini-protocol state machines + CBOR wire codecs + typed client/server drivers (ChainSync, BlockFetch, KeepAlive, TxSubmission2, PeerSharing). SDU segmentation/reassembly for large protocol messages.
- **Consensus**: Praos leader election, typed chain selection (VRF tiebreaker), epoch math, OpCert verification, KES period checks, block header verification with SumKES. `SecurityParam` (Ouroboros `k`), `ChainState` volatile chain state tracker with rollback depth enforcement and stability window detection.
- **Storage**: Trait-based `ImmutableStore`, `VolatileStore`, `LedgerStore` with in-memory and file-backed implementations, plus a minimal `ChainDb` coordinator for best-known tip recovery, volatile-prefix promotion into immutable storage, and rollback-time ledger snapshot truncation.
- **Mempool**: Fee-ordered queue with `TxId`-based entries, duplicate detection, capacity enforcement, TTL-aware admission, block-application eviction.
- **Node CLI**: `clap`-based binary with `run` (connect to peer and sync), `validate-config` (operator preflight for config, peer-snapshot inputs, and any existing storage recovery state), `status` (inspect on-disk storage and report sync position, block counts, and checkpoint state), and `default-config` (emit JSON config) subcommands. JSON configuration file support with CLI flag overrides, topology/config parsing that feeds reusable network-crate topology and peer-ordering helpers, and upstream-aligned tracing fields (`TurnOnLogging`, `UseTraceDispatcher`, `TraceOptions`, `TraceOptionNodeName`, `TraceOptionForwarder`). `NodeMetrics` provides atomic operational counters wired into the hot sync loops, with `--metrics-port` exposing a Prometheus-compatible HTTP `/metrics` endpoint and a JSON `/metrics/json` endpoint on `127.0.0.1`.
- **Node sync orchestration**: Full multi-era sync pipeline from bootstrap through managed service. Multi-era block decode (all 7 era tags). Consensus header verification bridge. Block header hash computation (Blake2b-256). Ordered bootstrap relay fallback plus reconnecting verified sync on ChainSync or BlockFetch connectivity loss. Graceful shutdown via Ctrl-C signal handling. A local `NodeTracer` now emits human- or machine-formatted runtime trace objects for bootstrap, reconnect, sync progress, and shutdown/failure paths. Live sync now evicts confirmed and expired transactions from the shared mempool, and epoch-boundary reward math uses tracked per-pool performance instead of an always-perfect stub.
- **Upstream parity**: CBOR golden round-trip tests, cross-subsystem integration tests, and wire-format field naming aligned with official Cardano CDDL specifications.
- **Validation baseline**: `cargo test-all` discovers 4,749 tests across the workspace (R232 cumulative — up from 4,640 at v0.2.0 baseline), all passing at every slice boundary.  The R211→R231 operational-parity arc adds: mainnet sync end-to-end (R211/R213), full LSQ surface verified on all 3 networks (R212–R215), bidirectional P2P parity (R220+R221), Phase A.6 GetGenesisConfig (R214), Phase D.2 5-counter lifetime peer-stats (R222–R226), Phase D.1 rollback-depth observability (R225), Phase E.1 5/5 documentary pins in-sync (R201+R216), and full Prometheus-output regression coverage for every R200/R217/R225/R226 observability metric (R229+R230+R231).
- CI workflow and workspace cargo aliases for check/test/lint.

### Status: 100% feature-complete

As of the 2026-Q2 audit closure, every confirmed-active parity slice tracked in [`docs/AUDIT_VERIFICATION_2026Q2.md`](docs/AUDIT_VERIFICATION_2026Q2.md) is closed, including the runtime integrations that previously lived as follow-ups: multi-session BlockFetch orchestration through the per-peer `FetchWorkerPool` (mirroring upstream `Ouroboros.Network.BlockFetch.ClientRegistry`) consuming `partition_fetch_range_across_peers`; the ChainSync `observe_header(slot)` hook feeding per-peer `DensityWindow` instances surfaced through `PeerMetrics.density`; and weight-aware connection-manager scheduling driven by `HotPeerScheduling` with density-biased demotion.  The remaining production-readiness gate is the operator-side manual rehearsal in [`docs/MANUAL_TEST_RUNBOOK.md`](docs/MANUAL_TEST_RUNBOOK.md) §2–9 (preprod/mainnet sync, hash compare vs Haskell node, restart-resilience cycles, parallel-fetch rehearsal §6.5, sign-off summary).

### Ongoing operational work

- Mainnet sync endurance testing per the runbook (Phase E.2 — 24h+ rehearsal).
- Extended cardano-tracer interoperability validation.

### Deferred substantive items (R211→R231 arc surfaces)

Each requires multi-day implementation or sustained operator time:

- **Phase D.1 full deep-rollback recovery** — historical stake-snapshot reconstruction so rollbacks beyond `k` blocks don't force re-sync from origin.  R225's `yggdrasil_rollback_depth_blocks` Prometheus histogram is the empirical-data prerequisite that justifies (or de-prioritises) the implementation cost based on actual mainnet rollback distribution.
- **Phase D.2 bytes-out** — per-mini-protocol egress byte accounting on the server-emit path.  The 5 lifetime peer-stats counters (`peer_lifetime_sessions_total`, `_failures_total`, `_bytes_in_total`, `_unique_peers`, `_handshakes_total`) ship today via R222+R223+R224+R226; bytes-out remains 0 until the per-protocol egress instrumentation lands.
- **Phase E.1 cardano-base** — coordinated vendored fixture refresh.  The other 5/5 documentary upstream pins are in-sync (R201+R216); `cardano-base` is gated on a `git mv` of the vendored test-vector tree at `specs/upstream-test-vectors/cardano-base/<sha>/`.
- **Phase E.2** — 24h+ mainnet sync rehearsal.  Operator-time gate; yggdrasil's mainnet sync is end-to-end working (R211+R213) and exposes all the observability surface needed (R200/R217/R225/R226 histograms + R222+R226 lifetime counters).

## Workspace Layout

The workspace is a strict bottom-up dependency stack — see [crates/AGENTS.md](crates/AGENTS.md) for direction rules.

| Crate / path | Purpose |
| --- | --- |
| [crates/crypto](crates/crypto) | Blake2b, Ed25519, VRF (std + batchcompat), KES (Simple + Sum 0–6+), BLS12-381, secp256k1. |
| [crates/cddl-codegen](crates/cddl-codegen) | Parses pinned Cardano CDDL, emits Rust + reproducible CBOR codecs. |
| [crates/ledger](crates/ledger) | Era types Byron→Conway, multi-era UTxO, per-era apply rules, governance, PPUP, MIR, ratification. |
| [crates/storage](crates/storage) | `ImmutableStore` / `VolatileStore` / `LedgerStore` traits + file-backed impls + `ChainDb` coordinator. |
| [crates/consensus](crates/consensus) | Praos leader election, KES/OpCert checks, `ChainState`, nonce evolution. |
| [crates/mempool](crates/mempool) | Fee-ordered queue with TTL, eviction, ledger revalidation. |
| [crates/network](crates/network) | Mux, mini-protocols, governor, peer registry, root + ledger-peer providers, diffusion types. |
| [crates/plutus](crates/plutus) | CEK machine, builtin semantics, cost model. |
| [node](node) | `yggdrasil-node` binary — CLI, config, sync runtime, inbound server, governor loop, block producer, NtC. |
| [docs](docs) | Architecture, dependency policy, specs, parity plan, manual test runbook, user manual. |
| [specs](specs) | Pinned CDDL fixtures + vendored upstream test vectors. |

## Verification

The required gates before declaring work done are the same three workflow CI runs:

```bash
cargo check-all   # cargo check --workspace --all-targets
cargo test-all    # cargo test --workspace --all-features
cargo lint        # cargo clippy --workspace --all-targets --all-features -- -D warnings
```

All three must pass. The release build also runs `cargo doc --workspace --no-deps`. Aliases live in [.cargo/config.toml](.cargo/config.toml); CI lives in [.github/workflows/ci.yml](.github/workflows/ci.yml).

## Documentation

- [User Manual](https://yggdrasil-node.github.io/Cardano-node/manual/) — install, configure, run, monitor, troubleshoot, and produce blocks.
- [Architecture](docs/ARCHITECTURE.md) — phase-by-phase implementation overview.
- [Dependency policy](docs/DEPENDENCIES.md) — rules for adding/removing third-party crates.
- [Specification priority](docs/SPECS.md) — where to anchor parity-sensitive changes.
- [Contribution workflow](docs/CONTRIBUTING.md) — gates, AGENTS.md rules, commit conventions.
- [Manual test runbook](docs/MANUAL_TEST_RUNBOOK.md) — operator rehearsal procedure for sync, hash compare, restart resilience, and parallel-fetch validation.
- [Security policy](SECURITY.md) — supported versions, vulnerability disclosure.
- [Changelog](CHANGELOG.md) — release-by-release record.

## Roadmap (post-1.0)

Yggdrasil 1.0 closes the 2026-Q2 audit. Post-1.0 work tracked through GitHub Issues includes:

- Sustained mainnet endurance soak (week-scale) with hash-compare against the Haskell node.
- Extended `cardano-tracer` interoperability validation across both forwarder and stdout backends.
- Default `max_concurrent_block_fetch_peers` flip from `1` to `2` once `MANUAL_TEST_RUNBOOK.md` §6.5 sign-off lands.
- Future Conway tail-parameter cost-model entries beyond the vendored 251-name surface in `crates/plutus`.

## License

Apache-2.0 — see [LICENSE](LICENSE).