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
- **Ledger**: Full era type coverage Byron through Conway. Hand-rolled CBOR codec. Multi-era UTxO (`MultiEraUtxo`, `MultiEraTxOut`) with era-aware `apply_block()` dispatch, coin/multi-asset preservation, TTL/validity-interval checks. PlutusData AST with full CBOR support. Certificate hierarchy (19 variants). Credential, address, and governance types.
- **Network**: SDU framing, async bearer transport, mux/demux, handshake, peer lifecycle, reusable peer candidate ordering, upstream-aligned topology domain types for local and public roots, and a root-provider layer that resolves, tracks, and refreshes local, bootstrap, and public roots with upstream-style precedence. All four mini-protocol state machines + CBOR wire codecs + typed client drivers (ChainSync, BlockFetch, KeepAlive, TxSubmission2). SDU segmentation/reassembly for large protocol messages.
- **Consensus**: Praos leader election, typed chain selection (VRF tiebreaker), epoch math, OpCert verification, KES period checks, block header verification with SumKES. `SecurityParam` (Ouroboros `k`), `ChainState` volatile chain state tracker with rollback depth enforcement and stability window detection.
- **Storage**: Trait-based `ImmutableStore`, `VolatileStore`, `LedgerStore` with in-memory and file-backed implementations.
- **Mempool**: Fee-ordered queue with `TxId`-based entries, duplicate detection, capacity enforcement, TTL-aware admission, block-application eviction.
- **Node CLI**: `clap`-based binary with `run` (connect to peer and sync) and `default-config` (emit JSON config) subcommands. JSON configuration file support with CLI flag overrides, topology/config parsing that feeds reusable network-crate topology and peer-ordering helpers, and upstream-aligned tracing fields (`TurnOnLogging`, `UseTraceDispatcher`, `TraceOptions`, `TraceOptionNodeName`, `TraceOptionForwarder`).
- **Node sync orchestration**: Full multi-era sync pipeline from bootstrap through managed service. Multi-era block decode (all 7 era tags). Consensus header verification bridge. Block header hash computation (Blake2b-256). Ordered bootstrap relay fallback plus reconnecting verified sync on ChainSync or BlockFetch connectivity loss. Graceful shutdown via Ctrl-C signal handling. A local `NodeTracer` now emits human- or machine-formatted runtime trace objects for bootstrap, reconnect, sync progress, and shutdown/failure paths.
- **Upstream parity**: CBOR golden round-trip tests, cross-subsystem integration tests, and wire-format field naming aligned with official Cardano CDDL specifications.
- CI workflow and workspace cargo aliases for check/test/lint.

### In Progress

- Dynamic root providers, peer registry state, and richer long-lived multi-peer management beyond ordered reconnect and startup root snapshots.
- Dedicated cardano-tracer transport and metrics endpoints beyond local stdout trace emission.
- Mainnet sync endurance testing.

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

1. Implement on-disk storage backends (immutable append-only, volatile with rollback) behind existing storage traits.
2. Harden consensus: chain selection refinement, rollback handling, fixed-point leadership arithmetic.
3. Expand upstream parity testing with vendored ledger and consensus test vectors.
4. Documentation refresh to reflect full era coverage and CLI capabilities.