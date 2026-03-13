<p align="center">
  <img src="https://github.com/Yggdrasil-node/.github/blob/main/images/Yggdrasil_node_full_logo.png" alt="Yggdrasil CardanoNode Logo" width="100%"/>
</p>

# Yggdrasil Cardano Node in Rust

Yggdrasil is a pure Rust Cardano node workspace targeting long-term protocol and serialization parity with the upstream Cardano node.

## Current Status

Implemented currently:
- Cargo workspace with stable crate boundaries for crypto, cddl-codegen, ledger, storage, consensus, mempool, network, and node integration.
- Crypto primitives with vector-backed verification/proving coverage (Blake2b, Ed25519, VRF, SimpleKES, SumKES).
- Ledger core typed identifiers and hand-rolled CBOR codec; Shelley-era transaction/header/block structures and a first UTxO transition slice.
- Network stack: SDU framing, async bearer transport, mux/demux, handshake, peer lifecycle, and full state machines + CBOR wire codecs for ChainSync, BlockFetch, KeepAlive, and TxSubmission2.
- Typed mini-protocol client drivers for all four data protocols.
- SDU segmentation/reassembly support for large protocol messages via mux segmentation and `MessageChannel` reassembly.
- Node runtime bootstrap (`NodeConfig`, `PeerSession`, `bootstrap`) and first sync orchestration helpers (`sync_step`, `sync_steps`) coordinating ChainSync + BlockFetch.
- Node Shelley decode bridge (`sync_step_decoded`, `decode_shelley_blocks`) for typed block handoff from BlockFetch bytes.
- CI workflow and workspace cargo aliases for check/test/lint.

Still in progress:
- Full typed payload bridging from all network protocol payloads into ledger/domain structures.
- Deeper ledger rule completeness and multi-era transition coverage.
- End-to-end storage/consensus-integrated sync loop and multi-peer management.
- Full upstream parity validation against official node traces and fixtures.

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

- Architecture: `docs/ARCHITECTURE.md`
- Dependency policy: `docs/DEPENDENCIES.md`
- Specification priority: `docs/SPECS.md`
- Contribution workflow: `docs/CONTRIBUTING.md`

## Next Development Phases

1. Extend decode bridging from Shelley block bodies to typed ChainSync point/tip/header structures.
2. Extend sync orchestration from step helpers to a resilient long-running pipeline with storage handoff.
3. Add staged consensus + ledger integration checks around fetched headers/blocks.
4. Expand parity testing against pinned upstream fixtures and behavior traces.