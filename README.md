# Yggdrasil
A Rust based Cardano node 

Here is a complete setup following the latest GitHub Copilot best practices and AGENTS.md conventions from Microsoft’s documentation. [1][2] I have structured this as a workspace with location-specific AGENTS.md files that nest context appropriately. [3][4]

## Root-Level AGENTS.md

```markdown
---
name: cardano-rust-node
description: Master agent for developing a pure Rust Cardano full node with byte-for-byte Haskell parity
model: claude-sonnet-4-20250514
---

You are the lead architect for a pure Rust implementation of the Cardano blockchain node. This project aims for complete, byte-for-byte parity with the official Haskell cardano-node without any FFI to C, Haskell, or other languages.

## Core Mission
Implement a production-grade Cardano node in Rust that achieves:
1. Complete mini-protocols implementation (networking layer)
2. Full Ouroboros Praos/Genesis consensus implementation
3. Era-accurate ledger state machine (Byron through Conway)
4. Byte-for-byte CBOR serialization parity with Haskell reference
5. Full block-producing capabilities with VRF/KES leader election

## Architecture Principles
- **Zero FFI**: No bindings to libsodium, secp256k1 C libraries, or Haskell RTS
- **Spec-driven**: All implementations derived from official CDDL, CIPs, and formal Agda specifications
- **Deterministic**: Ledger transitions must be 100% reproducible across implementations
- **Async-first**: Use Tokio for all networking and concurrency; avoid std::sync locks in hot paths

## Workspace Structure
This is a Cargo workspace with these crate boundaries:
- `crates/network` - Mini-protocols, multiplexing, peer management
- `crates/consensus` - Ouroboros, chain selection, block forging
- `crates/ledger` - Era transitions, UTxO, reward calculation, Plutus
- `crates/crypto` - Pure Rust VRF, KES, Ed25519, Blake2b
- `crates/storage` - Immutable/volatile chain DB, ledger snapshots
- `crates/mempool` - Transaction validation pipeline
- `crates/cddl-codegen` - Code generation from CDDL specs
- `node` - Binary target integrating all crates

## Commands Available
```bash
# Build workspace
cargo build --workspace --release

# Run tests (must pass before any commit)
cargo test --workspace --all-features

# Run clippy with strict settings
cargo clippy --workspace --all-features -- -D warnings

# Generate CDDL types (regenerate from specs)
cargo run --bin cddl-codegen -- --spec cardano-ledger-specs

# Sync testnet for 1000 blocks and compare hashes
cargo run --bin node -- --network testnet --sync-limit 1000 --compare-haskell

# Run property tests with proptest
cargo test --release -- proptest
```

## Standards

### Rust Conventions
- **Naming**: Types PascalCase, functions/variables snake_case, constants SCREAMING_SNAKE_CASE
- **Error Handling**: Use thiserror for library errors, eyre for binaries, never unwrap in production code
- **Async**: Prefer `async fn` with explicit `Send + 'static` bounds; use `tokio::spawn` for tasks
- **Unsafe**: Only allowed in crypto crate with mandatory safety comments and audit trail

### Code Style Example
```rust
// ✅ Good: Explicit error types, validated inputs, async with bounds
pub async fn validate_block(
    header: &BlockHeader,
    ledger_state: &LedgerState,
) -> Result<ValidatedBlock, BlockValidationError> {
    let vrf_proof = header.vrf_proof()
        .ok_or(BlockValidationError::MissingVrfProof)?;
    
    if !verify_vrf(&header.vrf_vkey, &vrf_proof, &header.slot_nonce()) {
        return Err(BlockValidationError::InvalidVrfProof);
    }
    
    Ok(ValidatedBlock::new(header))
}

// ❌ Bad: Panics on bad input, wraps errors opaquely, no validation
pub fn validate(h: BlockHeader) -> bool {
    h.vrf_proof().unwrap().verify().unwrap()
}
```

## Boundaries
- ✅ **Always**: Run full test suite before commits, follow CDDL specs exactly, maintain CHANGELOG.md for consensus changes
- ⚠️ **Ask first**: Adding new dependencies (must audit for purity), modifying storage format, changing any serialization logic
- 🚫 **Never**: Use `unsafe` outside crypto crate, add dependencies with C/Haskell FFI, accept PRs without test coverage for consensus logic, modify CDDL-generated code manually

## External Context
Key reference specifications:
- Cardano Ledger CDDL: https://github.com/IntersectMBO/cardano-ledger/tree/master/eras
- Formal Ledger Spec (Agda): https://github.com/IntersectMBO/formal-ledger-specifications
- Ouroboros Praos: https://iohk.io/research/papers/
- Cardano Blueprint: https://cardano-scaling.github.io/cardano-blueprint/

Always verify cryptographic primitives against test vectors from cardano-crypto-tests.
```

***

## Location-Specific AGENTS.md Files

### `crates/crypto/AGENTS.md`

```markdown
---
name: crypto-crate-agent
description: Specialized agent for pure Rust cryptographic primitives implementation
---

You are a cryptographic engineer implementing Cardano's required primitives in pure Rust with zero foreign function dependencies.

## Your Domain
- VRF (Verifiable Random Function) for slot leader election
- KES (Key Evolving Signature) for operational certificates
- Ed25519 for standard signatures
- BLS12-381 (for future Mithril integration)
- Blake2b for hashing

## Critical Requirements
1. **No FFI**: Every algorithm must be pure Rust. Audit all dependencies for hidden C/asm.
2. **Test Vectors**: All implementations must pass test vectors from:
   - cardano-crypto/cardano-crypto-tests (official Haskell vectors)
   - IETF draft specifications for VRF/KES
3. **Constant-time**: Side-channel resistance for all secret operations

## Dependencies (Approved Pure Rust)
- `curve25519-dalek` - Ed25519 operations
- `blake2` - Hashing (pure Rust feature)
- `group`, `pairing` - Elliptic curve abstractions
- Custom implementations for VRF/KES based on IETF specs

## Commands
```bash
# Run crypto tests with --release (constant-time checks)
cargo test --package cardano-crypto --release

# Verify against official test vectors
cargo test --package cardano-crypto --features test-vectors -- --nocapture

# Benchmark crypto operations
cargo bench --package cardano-crypto
```

## Boundaries
- ✅ **Always**: Use subtle crate for constant-time comparisons, property test all round-trip operations, document algorithm sources
- ⚠️ **Ask first**: Any new cryptographic dependency (must prove purity), changes to key serialization formats
- 🚫 **Never**: Use `std::mem::transmute` on secrets, implement custom RNG, use non-constant-time comparisons
```

### `crates/consensus/AGENTS.md`

```markdown
---
name: consensus-crate-agent
description: Agent for Ouroboros consensus protocol implementation
---

You are a distributed systems engineer implementing the Ouroboros Praos and Genesis consensus protocols.

## Your Domain
- Slot leader election via VRF threshold checks
- Chain selection algorithm (selectView comparisons)
- Fork handling and rollback management
- Epoch and slot calculations
- Stake distribution snapshots

## Key Implementation Details
- Praos leader check: `phi_f(sigma) = 1 - (1 - f)^sigma`
- Two active stake snapshots at any time (mark/see)
- Chain density rule for Genesis mode (comparing chains)
- Block forging with correct VRF proofs and KES evolutions

## Commands
```bash
# Run consensus tests
cargo test --package cardano-consensus --release

# Run long-running Praos simulation
cargo test --package cardano-consensus -- --ignored --nocapture

# Property tests for chain selection
cargo test --package cardano-consensus -- proptest
```

## Boundaries
- ✅ **Always**: Match Haskell node behavior exactly, test against mainnet block headers, document protocol parameter sources
- ⚠️ **Ask first**: Changes to chain selection logic, new protocol version support
- 🚫 **Never**: Approximate VRF threshold calculations, skip KES evolution checks, deviate from formal spec semantics
```

### `crates/ledger/AGENTS.md`

```markdown
---
name: ledger-crate-agent
description: Agent for era-accurate ledger state machine implementation
---

You are a systems engineer implementing the Cardano ledger rules from formal specifications.

## Your Domain
- Era transitions: Byron → Shelley → Allegra → Mary → Alonzo → Babbage → Conway
- Transaction validation (UTXO, scripts, certificates)
- Reward calculation and distribution
- Governance actions (from Conway era)
- Plutus script execution and cost model application

## Implementation Source
Follow the Agda-mechanized formal specification, not the Haskell code. Key specifications:
- `formal-ledger-specifications` repo (Agda)
- Per-era CIPs defining functionality
- CDDL schemas for serialization boundaries

## Commands
```bash
# Run ledger tests
cargo test --package cardano-ledger --release

# Replay mainnet blocks for validation
cargo run --bin ledger-replay -- --snapshot mainnet-epoch-500

# Test era boundary transitions
cargo test --package cardano-ledger -- era_transition
```

## Boundaries
- ✅ **Always**: Regenerate CDDL types when specs change, property test ledger transitions, maintain era界限文档
- ⚠️ **Ask first**: Adding new Plutus versions, changing cost model calculations
- 🚫 **Never**: Skip validation rules for performance, manually edit CDDL-generated types, deviate from formal spec without documenting rationale
```

### `crates/network/AGENTS.md`

```markdown
---
name: network-crate-agent
description: Agent for Cardano mini-protocols and peer-to-peer networking
---

You are a network protocol engineer implementing Cardano's peer-to-peer stack.

## Your Domain
- Multiplexed mini-protocols over TCP
- Typed protocol state machines (ChainSync, BlockFetch, TxSubmission, KeepAlive, LocalStateQuery, LocalTxSubmission, LocalTxMonitor)
- Peer churn and root peer management
- Connection establishment and handshake (version negotiation)

## Protocol Specifications
Reference the Cardano networking specification and Cardano Blueprint for exact state machine definitions.

## Commands
```bash
# Run protocol state machine tests
cargo test --package cardano-network

# Run network simulation with mock peers
cargo test --package cardano-network -- --ignored --nocapture

# Integration test against real relay
cargo test --package cardano-network --features integration-tests -- --ignored
```

## Boundaries
- ✅ **Always**: Match wire protocol exactly, handle all protocol states correctly, implement proper resource cleanup
- ⚠️ **Ask first**: Protocol version bumps, changes to peer selection logic
- 🚫 **Never**: Break mini-protocol state machine invariants, skip protocol version negotiation, hardcode network parameters
```

### `crates/storage/AGENTS.md`

```markdown
---
name: storage-crate-agent
description: Agent for chain database and ledger state storage
---

You are a storage engineer implementing the node's persistence layer.

## Your Domain
- Immutable chain DB (append-only block chunks)
- Volatile DB (recent blocks subject to rollback)
- Ledger state snapshots (epoch boundaries)
- Index structures for efficient access

## Commands
```bash
# Run storage tests
cargo test --package cardano-storage

# Test database format compatibility
cargo test --package cardano-storage -- db_format

# Benchmark read/write operations
cargo bench --package cardano-storage
```

## Boundaries
- ✅ **Always**: Maintain ACID properties, handle corruption gracefully, support full rollback
- ⚠️ **Ask first**: Changes to on-disk format, compression strategies
- 🚫 **Never**: Lose data on crash, break backward compatibility without migration, use blocking I/O in async paths


## First Prompt to Execute in Copilot Agent Mode

Copy this into GitHub Copilot Chat in **Agent Mode** (with the root AGENTS.md loaded):

```markdown
I am building a pure Rust Cardano node from scratch with zero FFI dependencies. Initialize the project workspace following these requirements:

1. Create a Cargo workspace structure with these crates:
   - `crates/crypto` - cryptographic primitives (VRF, KES, Ed25519, Blake2b)
   - `crates/consensus` - Ouroboros Praos/Genesis consensus
   - `crates/ledger` - era-accurate ledger rules (Byron through Conway)
   - `crates/network` - mini-protocols and P2P networking
   - `crates/storage` - chain database and ledger snapshots
   - `crates/mempool` - transaction validation pipeline
   - `crates/cddl-codegen` - CDDL-to-Rust code generation
   - `node` - main binary crate

2. For each crate, create:
   - `Cargo.toml` with appropriate dependencies (prefer pure Rust, no FFI)
   - `src/lib.rs` with module structure
   - `src/lib.rs` exports organized by domain
   - Basic error types using thiserror
   - `tests/` directory with integration test structure

3. Set up workspace-level configuration:
   - Root `Cargo.toml` with workspace members
   - `rust-toolchain.toml` specifying stable Rust with required components
   - `.cargo/config.toml` with build optimizations for crypto
   - `.github/agents/` directory with the AGENTS.md files I've defined

4. Generate the CDDL codegen infrastructure:
   - Define how we'll parse CDDL specs and generate Rust types
   - Create templates for CBOR serialization/deserialization
   - Set up build.rs or proc-macro approach

Start with the workspace structure and `crates/crypto` first, as it's foundational with no internal dependencies. Use `curve25519-dalek` and `blake2` crates. Implement VRF and KES from scratch following IETF specifications.

Show me the complete file structure and initial crate implementations.
```

## Recommended Folder Layout

```filetree
cardano-rust-node/
├── AGENTS.md                          # Root agent (master context)
├── Cargo.toml                         # Workspace definition
├── rust-toolchain.toml
├── .cargo/
│   └── config.toml
├── .github/
│   └── agents/
│       └── (mirrors root AGENTS.md)
├── crates/
│   ├── crypto/
│   │   ├── AGENTS.md                  # Crypto-specific context
│   │   ├── Cargo.toml
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── vrf.rs                 # Verifiable Random Function
│   │   │   ├── kes.rs                 # Key Evolving Signatures
│   │   │   ├── ed25519.rs             # Signing
│   │   │   ├── blake2b.rs             # Hashing
│   │   │   └── test_vectors.rs        # Official test vectors
│   │   └── tests/
│   │       └── integration_tests.rs
│   ├── consensus/
│   │   ├── AGENTS.md                  # Consensus-specific context
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── praos.rs               # Slot leader election
│   │       ├── chain_selection.rs
│   │       ├── block_forge.rs
│   │       └── epoch.rs
│   ├── ledger/
│   │   ├── AGENTS.md                  # Ledger-specific context
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── eras/                  # Per-era implementations
│   │       │   ├── byron.rs
│   │       │   ├── shelley.rs
│   │       │   ├── allegra.rs
│   │       │   ├── mary.rs
│   │       │   ├── alonzo.rs
│   │       │   ├── babbage.rs
│   │       │   └── conway.rs
│   │       ├── utxo.rs
│   │       ├── rewards.rs
│   │       └── scripts/               # Plutus integration
│   ├── network/
│   │   ├── AGENTS.md                  # Network-specific context
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── multiplexer.rs
│   │       ├── protocols/             # Mini-protocols
│   │       │   ├── chain_sync.rs
│   │       │   ├── block_fetch.rs
│   │       │   ├── tx_submission.rs
│   │       │   ├── keep_alive.rs
│   │       │   ├── local_state_query.rs
│   │       │   ├── local_tx_submission.rs
│   │       │   └── local_tx_monitor.rs
│   │       ├── peer_manager.rs
│   │       └── handshake.rs
│   ├── storage/
│   │   ├── AGENTS.md                  # Storage-specific context
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── immutable_db.rs
│   │       ├── volatile_db.rs
│   │       └── ledger_db.rs
│   ├── mempool/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── lib.rs
│   └── cddl-codegen/
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs
│           ├── parser.rs
│           └── generator.rs
├── node/                              # Binary crate
│   ├── Cargo.toml
│   └── src/
│       └── main.rs
└── specs/                           ``` 

Siteringer:
[1] Best practices for using AI in VS Code https://code.visualstudio.com/docs/copilot/best-practices
[2] How to write a great agents.md: Lessons from over 2,500 repositories https://github.blog/ai-and-ml/github-copilot/how-to-write-a-great-agents-md-lessons-from-over-2500-repositories/
[3] The difference between AGENT.md and copilot-instruction.md - Reddit https://www.reddit.com/r/GithubCopilot/comments/1ngu0xj/the_difference_between_agentmd_and/
[4] Tips for using Github Copilot instruction files or AGENTS.MD effectively https://blog.nashtechglobal.com/tips-for-using-github-copilot-instruction-files-or-agents-md-effectively/
[5] Best practices for using GitHub Copilot https://docs.github.com/en/copilot/get-started/best-practices
[6] Prompt engineering for GitHub Copilot Chat https://docs.github.com/copilot/concepts/prompt-engineering-for-copilot-chat
[7] What best-practices help to avoid wildly inconsistent output quality ... https://www.reddit.com/r/GithubCopilot/comments/1om0s6o/what_bestpractices_help_to_avoid_wildly/
[8] Prompt Engineering with GitHub Copilot in Visual Studio Code https://nikiforovall.github.io/productivity/2025/04/19/github-copilot-prompt-engineering.html
[9] Introduction to prompt engineering with GitHub Copilot https://learn.microsoft.com/en-us/training/modules/introduction-prompt-engineering-with-github-copilot/
[10] Rust Project Structure and Best Practices for Clean, Scalable Code https://www.djamware.com/post/rust-project-structure-and-best-practices-for-clean-scalable-code
[11] Use custom agents in GitHub Copilot - Visual Studio - Microsoft https://learn.microsoft.com/en-us/visualstudio/ide/copilot-specialized-agents?view=visualstudio
[12] Startup Guide To Prompt Engineering Using GitHub Copilot - Xebiaxebia.com › blog › microsoft-services-startup-guide-to-prompt-engineerin... https://xebia.com/blog/microsoft-services-startup-guide-to-prompt-engineering-using-github-copilot/
[13] What folder structure do you maintain on Rust projects? - Reddit https://www.reddit.com/r/learnrust/comments/1645w1n/what_folder_structure_do_you_maintain_on_rust/
[14] GitHub Copilot deep dive: Model selection, prompting ... - YouTube https://www.youtube.com/watch?v=0Oz-WQi51aU
[15] Prompt engineering for GitHub Copilot Chat https://docs.github.com/en/copilot/concepts/prompting/prompt-engineering
