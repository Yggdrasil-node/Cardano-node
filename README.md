<p align="center">
  <img src="https://github.com/Yggdrasil-node/.github/blob/main/images/Yggdrasil_node_full_logo.png" alt="Yggdrasil CardanoNode Logo" width="100%"/>
</p>

# Yggdrasil Cardano Node in Rust

Yggdrasil is a pure Rust Cardano node workspace targeting long-term protocol and serialization parity with the upstream Cardano node.

## Quick Navigation

- [Current Status](#current-status)
- [Workspace Layout](#workspace-layout)
- [Commands](#commands)
- [Documentation](#documentation)
- [Next Development Phases](#next-development-phases)

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
- **Validation baseline**: `cargo test-all` discovers 4,630 tests across the workspace, all passing at every slice boundary.
- CI workflow and workspace cargo aliases for check/test/lint.

### Status: 100% feature-complete

As of the 2026-Q2 audit closure, every confirmed-active parity slice tracked in [`docs/AUDIT_VERIFICATION_2026Q2.md`](docs/AUDIT_VERIFICATION_2026Q2.md) is closed, including the runtime integrations that previously lived as follow-ups: multi-session BlockFetch orchestration through the per-peer `FetchWorkerPool` (mirroring upstream `Ouroboros.Network.BlockFetch.ClientRegistry`) consuming `partition_fetch_range_across_peers`; the ChainSync `observe_header(slot)` hook feeding per-peer `DensityWindow` instances surfaced through `PeerMetrics.density`; and weight-aware connection-manager scheduling driven by `HotPeerScheduling` with density-biased demotion.  The remaining production-readiness gate is the operator-side manual rehearsal in [`docs/MANUAL_TEST_RUNBOOK.md`](docs/MANUAL_TEST_RUNBOOK.md) §2–9 (preprod/mainnet sync, hash compare vs Haskell node, restart-resilience cycles, parallel-fetch rehearsal §6.5, sign-off summary).

### Ongoing operational work

- Mainnet sync endurance testing per the runbook.
- Extended cardano-tracer interoperability validation.

## Workspace Layout

```text
.
├── .cargo/
│   └── config.toml
├── .github/
│   └── workflows/
│       └── ci.yml
├── AGENTS.md
├── Cargo.lock
├── Cargo.toml
├── LICENSE
├── docs/
│   ├── ARCHITECTURE.md
│   ├── CONTRIBUTING.md
│   ├── DEPENDENCIES.md
│   └── SPECS.md
├── crates/
│   ├── cddl-codegen/
│   │   ├── AGENTS.md
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   └── tests/
│   ├── consensus/
│   │   ├── AGENTS.md
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   └── tests/
│   ├── crypto/
│   │   ├── AGENTS.md
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   └── tests/
│   ├── ledger/
│   │   ├── AGENTS.md
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   └── tests/
│   ├── mempool/
│   │   ├── AGENTS.md
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   └── tests/
│   ├── network/
│   │   ├── AGENTS.md
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   └── tests/
│   └── storage/
│       ├── AGENTS.md
│       ├── Cargo.toml
│       ├── src/
│       └── tests/
├── node/
│   ├── AGENTS.md
│   ├── Cargo.toml
│   ├── src/
│   │   ├── AGENTS.md
│   │   ├── lib.rs
│   │   ├── main.rs
│   │   ├── runtime.rs
│   │   └── sync.rs
│   └── tests/
│       ├── runtime.rs
│       ├── smoke.rs
│       └── sync.rs
├── specs/
│   ├── mini-ledger.cddl
│   └── upstream-test-vectors/
│       ├── AGENTS.md
│       └── cardano-base/
└── rust-toolchain.toml
```

## Commands

Workspace aliases:

```bash
cargo check-all
cargo test-all
cargo lint
```

Equivalent direct commands:

```bash
cargo check --workspace --all-targets
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

## Documentation

- Architecture: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)
- Dependency policy: [docs/DEPENDENCIES.md](docs/DEPENDENCIES.md)
- Specification priority: [docs/SPECS.md](docs/SPECS.md)
- Contribution workflow: [docs/CONTRIBUTING.md](docs/CONTRIBUTING.md)

## Next Development Phases

1. Implement consensus-network bridge for ledger peers to source peers from immutable ledger state.
2. Add governor-style peer policy with promotion, demotion, and churn behavior.
3. Expand upstream parity testing with vendored ledger and consensus test vectors.
4. Documentation refresh to reflect current network provider and registry capabilities.