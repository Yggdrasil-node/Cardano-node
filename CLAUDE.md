# CLAUDE.md

Guidance for Claude Code (claude.ai/code) and other AI assistants when working with this repository.

## Project

**Yggdrasil** is a pure Rust Cardano node targeting long-term protocol and serialization parity with the upstream Haskell node ([IntersectMBO](https://github.com/orgs/IntersectMBO/repositories/)). No FFI-backed cryptography; everything is native Rust. Edition 2024, toolchain pinned to `1.95.0` (see [rust-toolchain.toml](rust-toolchain.toml)).

For the architectural picture see [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md). For the running implementation journal and operational rules see [AGENTS.md](AGENTS.md).

## AGENTS.md Files Are Primary Context

Every meaningful subdirectory has an `@AGENTS.md`. They are operational, kept current, and take precedence over this file for local details. **Read them before changing code in a given area, and update them after changes.** The root [AGENTS.md](AGENTS.md) is very long — prefer targeted reads with `offset`/`limit` over full reads.

| Path | Purpose |
| --- | --- |
| [AGENTS.md](AGENTS.md) | Workspace-wide rules, upstream references, and rolling implementation journal |
| [crates/AGENTS.md](crates/AGENTS.md) | Crate-index conventions and dependency direction |
| [crates/crypto/AGENTS.md](crates/crypto/AGENTS.md) | Hashing, Ed25519, VRF, KES, BLS12-381 |
| [crates/cddl-codegen/AGENTS.md](crates/cddl-codegen/AGENTS.md) | CDDL parsing + generated CBOR codecs |
| [crates/ledger/AGENTS.md](crates/ledger/AGENTS.md) | Era types, rules, governance, Plutus integration trait |
| [crates/storage/AGENTS.md](crates/storage/AGENTS.md) | `ImmutableStore`/`VolatileStore`/`LedgerStore` and `ChainDb` |
| [crates/consensus/AGENTS.md](crates/consensus/AGENTS.md) | Praos, KES/OpCert, `ChainState`, nonce evolution |
| [crates/mempool/AGENTS.md](crates/mempool/AGENTS.md) | Fee-ordered mempool, TTL, eviction, TxSubmission accounting |
| [crates/network/AGENTS.md](crates/network/AGENTS.md) | Mux, mini-protocols, governor, peer registry, diffusion types |
| [crates/plutus/AGENTS.md](crates/plutus/AGENTS.md) | CEK machine, builtins, cost model |
| [node/AGENTS.md](node/AGENTS.md) | Runtime orchestration boundary rules |
| [node/src/AGENTS.md](node/src/AGENTS.md) | CLI, config, sync, server, block production |
| [node/configuration/AGENTS.md](node/configuration/AGENTS.md) | Vendored mainnet/preprod/preview operator configs |
| [docs/AGENTS.md](docs/AGENTS.md) | Architecture/dependency/spec/contributing docs policy |
| [specs/AGENTS.md](specs/AGENTS.md) | Pinned CDDL fixtures and provenance |
| [specs/upstream-test-vectors/AGENTS.md](specs/upstream-test-vectors/AGENTS.md) | Vendored upstream vectors (must not be hand-edited) |

## Commands

Workspace aliases (defined in [.cargo/config.toml](.cargo/config.toml)):

```bash
cargo check-all   # cargo check --workspace --all-targets
cargo test-all    # cargo test --workspace --all-features
cargo lint        # cargo clippy --workspace --all-targets --all-features -- -D warnings
```

All four (`fmt --all -- --check`, `check-all`, `test-all`, `lint`) are the required verification expectations before declaring work done. CI ([.github/workflows/ci.yml](.github/workflows/ci.yml)) runs the same set.

Running a single test:

```bash
cargo test -p yggdrasil-ledger --test <integration_test_name>      # one integration test file
cargo test -p yggdrasil-network <substring>                        # by name substring within a crate
cargo test -p yggdrasil-node --lib <mod::path::test_name> -- --exact
```

Crate package names use the `yggdrasil-` prefix: `yggdrasil-crypto`, `yggdrasil-cddl-codegen`, `yggdrasil-ledger`, `yggdrasil-storage`, `yggdrasil-consensus`, `yggdrasil-mempool`, `yggdrasil-network`, `yggdrasil-plutus`, plus the `yggdrasil-node` binary.

## Workspace Topology and Dependency Order

Crates form a strict dependency stack — respect this direction when adding cross-crate calls:

1. `crates/crypto` — Blake2b, Ed25519, VRF (std + batchcompat, ietfdraft03/13), KES (Simple + Sum depth 0–6+), BLS12-381 (PlutusV3/CIP-0381), secp256k1 (ECDSA + BIP-340).
2. `crates/cddl-codegen` — parses pinned Cardano CDDL, generates Rust structs + `CborEncode`/`CborDecode` impls; outputs must remain reproducible and must not be hand-edited.
3. `crates/ledger` + `crates/storage` — era types Byron→Conway, multi-era UTxO, all per-era apply rules, epoch boundary, governance enactment, PPUP, MIR, ratification engine; trait-based `ImmutableStore`/`VolatileStore`/`LedgerStore` plus a `ChainDb` coordinator with file-backed implementations and crash-recovery.
4. `crates/consensus` + `crates/mempool` — Praos leader election, OpCert/KES checks, `ChainState`, nonce evolution (TPraos + Praos), per-pool OpCert counter monotonicity; fee-ordered mempool with TTL, block-application eviction, ledger revalidation, TxSubmission inbound byte/count accounting.
5. `crates/network` — SDU framing, mux, handshake, all five mini-protocols (ChainSync, BlockFetch, KeepAlive, TxSubmission2, PeerSharing) with typed client + server drivers, peer registry, root providers, ledger-peer provider, governor decision engine, inbound governor, connection manager, diffusion types, `blockfetch_pool`.
6. `crates/plutus` — CEK machine, builtin semantics, cost model (used via the `PlutusEvaluator` trait in `ledger`).
7. `node/` — thin orchestration layer. `clap` CLI, JSON/YAML config, sync runtime, inbound server, governor loop, block producer, tracer/metrics, NtC local socket dispatcher.

The `node/` crate **must stay an integration layer**. Reusable policy, peer-selection state, or protocol-facing state machines belong in `crates/*`, not in `node/`. **Extraction rule:** move logic out of `node/` as soon as it is reused across runtime paths, owns non-trivial protocol/peer state, or needs tests independent of the CLI entry point.

## Node Binary Surface

`yggdrasil-node` exposes the following `clap` subcommands (see [node/src/main.rs](node/src/main.rs) and [node/src/AGENTS.md](node/src/AGENTS.md) for flags):

- `run` — connect, sync, serve inbound peers, run governor + optional block producer.
- `validate-config` — operator preflight for config, peer-snapshot inputs, recovery state, genesis-hash integrity, governor sanity, KES/Praos invariants.
- `status` — inspect on-disk storage and report sync position, block counts, checkpoint state, ledger counts.
- `default-config` — emit the default JSON config to stdout.
- `cardano-cli` — pure-Rust subset (`version`, `show-upstream-config`, `query-tip`).
- `query` (Unix) — NtC LocalStateQuery dispatcher for all 24 supported tags (0–23).
- `submit-tx` (Unix) — NtC LocalTxSubmission with `0x`-prefix-tolerant `--tx-hex`.

`--network mainnet|preprod|preview` selects a preset; `--config` overrides the file; individual flags (`--peer`, `--network-magic`, `--port`, `--host-addr`, `--database-path`, `--topology`, `--metrics-port`, etc.) override config-file values.

## Upstream Parity Is Non-Negotiable

- **Spec priority:** (1) formal ledger specifications and protocol papers, (2) Cardano CDDL schemas, (3) accepted CIPs, (4) Haskell implementation behavior.
- **Naming:** type and function names must stay as close to upstream IntersectMBO terminology as practical (e.g. `HeaderBody`, `ChainState`, `EnactState`, `PeerTxState`, `GovernorState`, `ConnectionManagerCounters`).
- **Research:** consult the relevant [IntersectMBO repo](https://github.com/orgs/IntersectMBO/repositories/) before introducing local terminology, behavior, or design. `pragma-org/amaru` and `txpipe/dolos` may be consulted as Rust-port examples but are not authoritative.
- **Serialization & crypto:** byte-accurate CBOR serialization and cryptographic parity are long-term targets even when an implementation slice is incomplete. Generated artifacts must remain reproducible.
- **Dependencies:** new dependencies must be justified in [docs/DEPENDENCIES.md](docs/DEPENDENCIES.md) before being treated as accepted.
- **No FFI:** FFI cryptography and hidden native dependencies are forbidden.

### Official IntersectMBO Upstream References

Anchor every parity-sensitive change to one of these:

- **Node integration** — [`IntersectMBO/cardano-node`](https://github.com/IntersectMBO/cardano-node) (`cardano-node/`, `cardano-submit-api/`, `configuration/`).
- **Ledger** — [`IntersectMBO/cardano-ledger`](https://github.com/IntersectMBO/cardano-ledger) (`eras/`, `libs/cardano-ledger-binary/`, `libs/`).
- **Formal specs** — [`IntersectMBO/formal-ledger-specifications`](https://github.com/IntersectMBO/formal-ledger-specifications) and the [published spec site](https://intersectmbo.github.io/formal-ledger-specifications/site).
- **Consensus** — [`IntersectMBO/ouroboros-consensus`](https://github.com/IntersectMBO/ouroboros-consensus) (`ouroboros-consensus/`, `ouroboros-consensus-protocol/`, `ouroboros-consensus-cardano/`, `ouroboros-consensus-diffusion/`, `docs/agda-spec`).
- **Network** — [`IntersectMBO/ouroboros-network`](https://github.com/IntersectMBO/ouroboros-network) (`network-mux/`, `ouroboros-network-framework/`, `ouroboros-network-protocols/`, `ouroboros-network/`, `cardano-diffusion/`).
- **Crypto** — [`IntersectMBO/cardano-base`](https://github.com/IntersectMBO/cardano-base) (`cardano-crypto-class/`, `cardano-crypto-praos/`, `cardano-crypto-peras/`).
- **Plutus** — [`IntersectMBO/plutus`](https://github.com/IntersectMBO/plutus) (`plutus-core/`, CEK machine and cost model).
- **Haddock docs** — [ledger](https://cardano-ledger.cardano.intersectmbo.org/), [consensus](https://ouroboros-consensus.cardano.intersectmbo.org/haddocks/), [network](https://ouroboros-network.cardano.intersectmbo.org/), [base](https://base.cardano.intersectmbo.org/).
- **Operations** — [Cardano Operations Book](https://book.world.dev.cardano.org/) ([env-mainnet](https://book.world.dev.cardano.org/env-mainnet.html), [env-preprod](https://book.world.dev.cardano.org/env-preprod.html), [env-preview](https://book.world.dev.cardano.org/env-preview.html)).

The root [AGENTS.md](AGENTS.md) carries per-crate references to the exact upstream subdirectory each crate mirrors.

## Workspace Lints

Denied in [Cargo.toml](Cargo.toml) `[workspace.lints.clippy]`: `dbg_macro`, `todo`, `unwrap_used`. `cargo lint` also enforces `-D warnings` workspace-wide. `yggdrasil-crypto` is compiled at `opt-level = 3` in both `dev` and `test` profiles to keep hashing/VRF test times reasonable.

Tests can opt out of `unwrap_used` via the per-crate `#![cfg_attr(test, allow(clippy::unwrap_used))]` attribute already present in `lib.rs`/`main.rs` files.

## Style

- Typesafe Rust with proper Rustdocs on public APIs when behavior is non-obvious.
- Prefer incremental milestones that compile and test cleanly over speculative completeness.
- Keep `AGENTS.md` files operational (actionable rules + current status), not long-form documentation.
- When a folder's `AGENTS.md` is outdated, missing, or incorrect, update it as part of the change.
- Use upstream Haskell module references in commit messages, comments, and journal entries (e.g. `Ouroboros.Network.PeerSelection.Governor`, `Cardano.Ledger.Conway.Rules.Utxo`) so parity work and fixture comparison remain tractable.
- Avoid hand-editing generated artifacts under `crates/cddl-codegen` outputs or vendored data under `node/configuration/*` and `specs/upstream-test-vectors/`.
