<p align="center">
  <img src="docs/assets/images/Yggrasil_banner.png" alt="YggdrasilNode — A Cardano Node Project Written In Rust" width="100%"/>
</p>

# Yggdrasil Cardano Node in Rust

[![CI](https://github.com/yggdrasil-node/Cardano-node/actions/workflows/ci.yml/badge.svg)](https://github.com/yggdrasil-node/Cardano-node/actions/workflows/ci.yml)
[![Pages](https://github.com/yggdrasil-node/Cardano-node/actions/workflows/pages.yml/badge.svg)](https://github.com/yggdrasil-node/Cardano-node/actions/workflows/pages.yml)
[![Release](https://github.com/yggdrasil-node/Cardano-node/actions/workflows/release.yml/badge.svg)](https://github.com/yggdrasil-node/Cardano-node/actions/workflows/release.yml)
[![Latest release](https://img.shields.io/github/v/release/yggdrasil-node/Cardano-node?include_prereleases&sort=semver)](https://github.com/yggdrasil-node/Cardano-node/releases/latest)
[![Rust 1.95.0](https://img.shields.io/badge/rust-1.95.0-orange)](rust-toolchain.toml)
[![Tests](https://img.shields.io/badge/tests-4.7K%2B%20passing-brightgreen)](#current-status)

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

> **Pre-1.0 status:** `v0.2.0` is the public code-level parity closure
> release for the 2026-Q2 audit cycle. Linux release tarballs are published
> for x86_64 and aarch64; source builds remain recommended for auditing,
> development, custom CPU targets, or operators who want to reproduce the
> binary locally.

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

**From a published release tarball (Linux x86_64 / aarch64):**

```bash
curl -fsSL https://raw.githubusercontent.com/yggdrasil-node/Cardano-node/main/node/scripts/install_from_release.sh | bash
```

The installer verifies the downloaded archive against the published
`SHA256SUMS.txt`. Full details: [Installing from Releases](https://yggdrasil-node.github.io/Cardano-node/manual/releases/).

## Current Status

### Implemented

- Cargo workspace with stable crate boundaries for crypto, cddl-codegen, ledger, storage, consensus, mempool, network, and node integration.
- **Crypto**: Blake2b-256/512, Ed25519, VRF (standard + batchcompat), SimpleKES, SumKES (depth 0–6+), with upstream vector-backed coverage and zeroize hardening.
- **Ledger**: Full era type coverage Byron through Conway. Hand-rolled CBOR codec. Multi-era UTxO (`MultiEraUtxo`, `MultiEraTxOut`) with era-aware `apply_block()` dispatch, coin/multi-asset preservation, TTL/validity-interval checks. PlutusData AST with full CBOR support. Certificate hierarchy (19 variants). Credential, address, and governance types. Epoch-boundary processing includes stake-snapshot rotation, reward distribution, governance ratification/enactment, and Shelley PPUP protocol-parameter update application with genesis-delegate quorum.
- **Network**: SDU framing, async bearer transport, mux/demux, handshake, peer lifecycle, reusable peer candidate ordering, upstream-aligned topology domain types for local and public roots, a root-provider layer that resolves, tracks, and refreshes local, bootstrap, and public roots with upstream-style precedence, a DNS-backed provider for local roots, bootstrap peers, and configured public roots with optional time-gated refresh policy (exponential backoff, upstream-aligned 60 s / 900 s clamps), a minimal peer registry that tracks peer source and cold/cooling/warm/hot status in the crate instead of `node`, including crate-owned reconciliation helpers for root, ledger, big-ledger, and peer-share source sets, and a ledger peer provider layer with `LedgerPeerProvider` trait, `LedgerPeerSnapshot` normalization (deduplicates and enforces disjoint ledger/big-ledger sets), `LedgerPeerProviderRefresh` (combined/per-kind), `apply_ledger_peer_refresh()` helper, `refresh_ledger_peer_registry()` orchestration, and `ScriptedLedgerPeerProvider` for testing. All five mini-protocol state machines + CBOR wire codecs + typed client/server drivers (ChainSync, BlockFetch, KeepAlive, TxSubmission2, PeerSharing). SDU segmentation/reassembly for large protocol messages.
- **Consensus**: Praos leader election, typed chain selection (VRF tiebreaker), epoch math, OpCert verification, KES period checks, block header verification with SumKES. `SecurityParam` (Ouroboros `k`), `ChainState` volatile chain state tracker with rollback depth enforcement and stability window detection.
- **Storage**: Trait-based `ImmutableStore`, `VolatileStore`, `LedgerStore` with in-memory and file-backed implementations, plus a minimal `ChainDb` coordinator for best-known tip recovery, volatile-prefix promotion into immutable storage, rollback-time ledger snapshot truncation, and slot-indexed ChainDepState sidecar snapshots for nonce/OpCert rollback restore.
- **Mempool**: Fee-ordered queue with `TxId`-based entries, duplicate detection, capacity enforcement, TTL-aware admission, block-application eviction.
- **Node CLI**: `clap`-based binary with `run` (connect to peer and sync), `validate-config` (operator preflight for config, peer-snapshot inputs, and any existing storage recovery state), `status` (inspect on-disk storage and report sync position, block counts, and checkpoint state), and `default-config` (emit JSON config) subcommands. JSON configuration file support with CLI flag overrides, topology/config parsing that feeds reusable network-crate topology and peer-ordering helpers, and upstream-aligned tracing fields (`TurnOnLogging`, `UseTraceDispatcher`, `TraceOptions`, `TraceOptionNodeName`, `TraceOptionForwarder`). `NodeMetrics` provides atomic operational counters wired into the hot sync loops, with `--metrics-port` exposing a Prometheus-compatible HTTP `/metrics` endpoint and a JSON `/metrics/json` endpoint on `127.0.0.1`.
- **Node sync orchestration**: Full multi-era sync pipeline from bootstrap through managed service. Multi-era block decode (all 7 era tags). Consensus header verification bridge. Block header hash computation (Blake2b-256). Ordered bootstrap relay fallback plus reconnecting verified sync on ChainSync or BlockFetch connectivity loss. Graceful shutdown via Ctrl-C signal handling. A local `NodeTracer` emits human- or machine-formatted runtime trace objects for bootstrap, reconnect, sync progress, and shutdown/failure paths. Live sync evicts confirmed and expired transactions from the shared mempool, epoch-boundary reward math uses tracked per-pool performance, and rollback recovery restores nonce/OpCert ChainDepState from sidecar history before replaying stored blocks to the selected rollback point.
- **Upstream parity**: CBOR golden round-trip tests, cross-subsystem integration tests, and wire-format field naming aligned with official Cardano CDDL specifications.
- **Validation baseline**: `cargo test-all` covers 4.7K+ workspace tests and is green at every slice boundary. The R211→R245 operational-parity arc adds: mainnet sync end-to-end (R211/R213), full LSQ surface verified on all 3 networks (R212–R215), bidirectional P2P parity (R220+R221), Phase A.6 GetGenesisConfig (R214), Phase D.2 lifetime peer-stats plus aggregate bytes-out (R222–R237), Phase D.1 rollback-depth observability plus exact ChainDepState sidecar restore-and-replay (R225/R237/R238), Phase E.1 all 6 documentary pins in-sync after the coordinated `cardano-base` fixture refresh and two `cardano-ledger` refreshes (R201+R216+R239+R243+R245), Byron genesis hash preflight parity via upstream canonical JSON hashing (R244), Conway BBODY `HeaderProtVerTooHigh` testnet grace through the Dijkstra transition (R245), Prometheus-output regression coverage for the R200/R217/R225/R226 observability metrics (R229+R230+R231), and `node/scripts/parallel_blockfetch_soak.sh` automation for the §6.5 multi-peer BlockFetch default-flip evidence gate (R240).
- CI workflow and workspace cargo aliases for check/test/lint.

### Status: code-level parity closure

As of R245, every confirmed-active code-level parity slice tracked in [`docs/AUDIT_VERIFICATION_2026Q2.md`](docs/AUDIT_VERIFICATION_2026Q2.md) is closed, including the runtime integrations that previously lived as follow-ups: multi-session BlockFetch orchestration through the per-peer `FetchWorkerPool` (mirroring upstream `Ouroboros.Network.BlockFetch.ClientRegistry`) consuming `partition_fetch_range_across_peers`; the ChainSync `observe_header(slot)` hook feeding per-peer `DensityWindow` instances surfaced through `PeerMetrics.density`; weight-aware connection-manager scheduling driven by `HotPeerScheduling` with density-biased demotion; Phase D.1 rollback sidecar hardening, where nonce/OpCert ChainDepState restores to the exact rollback point using slot-indexed sidecar bundles and stored-block replay; the coordinated upstream pin refreshes through the latest `cardano-ledger` BBODY/GOV drift; startup verification of all four preset genesis hashes, including Byron's upstream canonical JSON hash path; and the upstream Conway BBODY temporary suppression of `HeaderProtVerTooHigh` on testnets until protocol major 12. The remaining production-readiness gates are operator-side rehearsals in [`docs/MANUAL_TEST_RUNBOOK.md`](docs/MANUAL_TEST_RUNBOOK.md) §2–9 (preprod/mainnet sync, hash compare vs Haskell node, restart-resilience cycles, parallel-fetch rehearsal §6.5, sign-off summary); R240 adds the `parallel_blockfetch_soak.sh` harness so §6.5 evidence collection is reproducible.

### Ongoing operational work

- Mainnet sync endurance testing per the runbook (Phase E.2 — 24h+ rehearsal).
- Extended cardano-tracer interoperability validation.

### Remaining gates (R211→R245 arc)

The remaining items are operator-time gates, not known code-level parity blockers:

- **Phase E.2** — 24h+ mainnet sync rehearsal. Operator-time gate; yggdrasil's mainnet sync is end-to-end working (R211+R213) and exposes the observability surface needed for sign-off.
- **Parallel BlockFetch default flip** — runbook §6.5 must pass with `parallel_blockfetch_soak.sh` before changing the default `max_concurrent_block_fetch_peers = 1`.
- **Tracer interoperability** — extended `cardano-tracer` validation across the forwarder and stdout backends remains ongoing operational work.

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

For fast preview-network block-producer rehearsal, the ignored harness bundle can generate upstream `cardano-cli` credentials, a funding wallet, registration certificates, configs, and bounded relay/producer smoke runs:

```bash
cargo build --release --bin yggdrasil-node
FORCE=1 node/scripts/preview_producer_harness.sh all
RUN_SECONDS=300 MIN_SLOT_ADVANCE=1000 node/scripts/preview_producer_harness.sh endurance-producer
node/scripts/preview_producer_harness.sh funding-address
```

The preview harness writes pool metadata into `tmp/preview-producer/metadata/` and commits that hash into the generated pool certificate. Defaults are ticker `RUST` and name `WORLDS FIRST RUST NODE`; override `POOL_TICKER`, `POOL_NAME`, `POOL_DESCRIPTION`, `POOL_HOMEPAGE`, or `POOL_METADATA_URL` before running `certs` when needed.

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
