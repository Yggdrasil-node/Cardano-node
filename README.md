<p align="center">
  <img src="https://github.com/Yggdrasil-node/.github/blob/main/images/Yggdrasil_node_full_logo.png" alt="Yggdrasil Node Logo" width="600"/>
</p>

# Yggdrasil Cardano Node in Rust

Yggdrasil is a pure Rust Cardano node workspace. The current repository state is the foundation milestone: workspace scaffolding, crate boundaries, project policy, agent instructions, CI, and compileable crate skeletons.

## Current Status

Implemented in this milestone:
- Cargo workspace with crate boundaries for crypto, consensus, ledger, network, storage, mempool, code generation, and node integration.
- Root and crate-local `AGENTS.md` files for focused Copilot guidance.
- Baseline project documentation for architecture, dependency policy, specification priority, and contribution workflow.
- CI workflow plus local cargo aliases for check, test, and lint.
- Compileable Rust skeletons and smoke tests across the workspace.

Not implemented yet:
- Full Cardano cryptographic parity: VRF proving and verification paths are still pending, and KES remains a staged baseline rather than the final production scheme.
- Era-accurate ledger rules.
- Ouroboros consensus implementation.
- Mini-protocol networking and sync.
- Haskell-parity serialization and replay validation.

## Workspace Layout

```text
.
├── AGENTS.md
├── Cargo.toml
├── docs/
│   ├── ARCHITECTURE.md
│   ├── CONTRIBUTING.md
│   ├── DEPENDENCIES.md
│   └── SPECS.md
├── crates/
│   ├── cddl-codegen/
│   ├── consensus/
│   ├── crypto/
│   ├── ledger/
│   ├── mempool/
│   ├── network/
│   └── storage/
├── node/
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

1. Implement the crypto crate beyond placeholders, starting with Blake2b, key material handling, and test-vector infrastructure.
2. Build the initial `cddl-codegen` pipeline for pinned Cardano schema inputs.
3. Define the first real ledger slice and its state-transition tests.
4. Layer consensus, storage integration, mempool, and networking on top of stabilized domain interfaces.