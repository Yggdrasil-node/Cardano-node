# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Yggdrasil is a pure Rust Cardano node targeting long-term protocol and serialization parity with the upstream Haskell node (IntersectMBO). No FFI-backed cryptography; everything is native Rust. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the full architectural picture and [AGENTS.md](AGENTS.md) for the detailed in-repo implementation journal.

## AGENTS.md Files Are Primary Context

Every subdirectory that matters (`crates/*`, `node/`, `node/src/`, `specs/`, `docs/`, `specs/upstream-test-vectors/`) has an `AGENTS.md`. They are operational, must stay current, and take precedence over the root file for local details. **Read them before changing code in a given area, and update them after changes.** The root [AGENTS.md](AGENTS.md) is very long — prefer targeted reads with `offset`/`limit` over full reads.

## Commands

Workspace aliases (defined in [.cargo/config.toml](.cargo/config.toml)):

```bash
cargo check-all   # cargo check --workspace --all-targets
cargo test-all    # cargo test --workspace --all-features
cargo lint        # cargo clippy --workspace --all-targets --all-features -- -D warnings
```

All three (`check-all`, `test-all`, `lint`) are the required verification expectations before declaring work done.

Running a single test:

```bash
cargo test -p yggdrasil-ledger --test <integration_test_name>      # one integration test file
cargo test -p yggdrasil-network <substring>                        # by name substring within a crate
cargo test -p yggdrasil-node --lib <mod::path::test_name> -- --exact
```

Crate package names use the `yggdrasil-` prefix (`yggdrasil-crypto`, `yggdrasil-ledger`, `yggdrasil-consensus`, `yggdrasil-network`, `yggdrasil-storage`, `yggdrasil-mempool`, `yggdrasil-plutus`, plus the `yggdrasil-node` binary).

Rust toolchain is pinned to 1.85.0 with `clippy` and `rustfmt` (see [rust-toolchain.toml](rust-toolchain.toml)). Edition is 2024.

## Workspace Topology and Dependency Order

Crates form a strict dependency stack — respect this direction when adding cross-crate calls:

1. `crates/crypto` — Blake2b, Ed25519, VRF (std + batchcompat), KES (Simple + Sum depth 0–6+).
2. `crates/cddl-codegen` — parses pinned Cardano CDDL, generates Rust structs + CBOR codecs; outputs must remain reproducible and must not be hand-edited.
3. `crates/ledger` + `crates/storage` — era types (Byron → Conway), multi-era UTxO, rules, epoch boundary, governance enactment; trait-based `ImmutableStore`/`VolatileStore`/`LedgerStore` plus a `ChainDb` coordinator.
4. `crates/consensus` + `crates/mempool` — Praos leader election, OpCert/KES checks, `ChainState`, nonce evolution; fee-ordered mempool with TTL + block-application eviction.
5. `crates/network` — SDU framing, mux, handshake, all five mini-protocols (ChainSync, BlockFetch, KeepAlive, TxSubmission2, PeerSharing) with typed client + server drivers, peer registry, root providers, ledger-peer provider, governor decision engine, inbound governor, connection manager, `blockfetch_pool`.
6. `crates/plutus` — CEK machine, builtin semantics, cost model (used via the `PlutusEvaluator` trait in `ledger`).
7. `node/` — thin orchestration layer. CLI (`clap`), JSON config, sync runtime (`sync.rs`, `runtime.rs`), inbound server (`server.rs`), block producer, tracer, local NtC query dispatcher.

The `node/` crate **must stay an integration layer**. Reusable policy, peer-selection state, or protocol-facing state machines belong in `crates/*`, not in `node/`. Extraction rule: move logic out of `node/` as soon as it is reused across runtime paths, owns non-trivial protocol/peer state, or needs tests independent of the CLI entry point.

## Upstream Parity Is Non-Negotiable

- Spec priority: (1) formal ledger specs, (2) Cardano CDDL schemas, (3) accepted CIPs, (4) Haskell implementation behavior.
- Type and function naming must stay as close to upstream IntersectMBO terminology as practical (e.g. `HeaderBody`, `ChainState`, `EnactState`, `PeerTxState`, `GovernorState`).
- Research the relevant [IntersectMBO repo](https://github.com/orgs/IntersectMBO/repositories/) (cardano-base, cardano-ledger, ouroboros-consensus, ouroboros-network, cardano-node, plutus) before introducing local terminology, behavior, or design.
- `pragma-org/amaru` and `txpipe/dolos` may be consulted as examples but are not authoritative.
- Byte-accurate CBOR serialization and cryptographic parity are long-term targets even when an implementation slice is incomplete. Generated artifacts must remain reproducible.
- New dependencies must be justified in [docs/DEPENDENCIES.md](docs/DEPENDENCIES.md) before being treated as accepted.
- FFI cryptography and hidden native dependencies are forbidden.

## Workspace Lints

Denied in [Cargo.toml](Cargo.toml) `[workspace.lints.clippy]`: `dbg_macro`, `todo`, `unwrap_used`. `cargo lint` also enforces `-D warnings` workspace-wide. `yggdrasil-crypto` is compiled at `opt-level = 3` in both `dev` and `test` profiles to keep hashing/VRF test times reasonable.

## Style

- Typesafe Rust with proper Rustdocs on public APIs when behavior is non-obvious.
- Prefer incremental milestones that compile and test cleanly over speculative completeness.
- Keep `AGENTS.md` files operational (actionable rules + current status), not long-form docs.
- When a folder's `AGENTS.md` is outdated, missing, or incorrect, update it as part of the change.
