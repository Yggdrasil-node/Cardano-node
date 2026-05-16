# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.
Pure-Rust port of `cardano-node` targeting **100% protocol parity**, **100% naming parity**, **100% functionality parity**, and **100% filename parity** with the Haskell reference. The reference is **always the latest IntersectMBO/cardano-node release tag** — currently `11.0.1`; bump in lockstep across `docs/parity-matrix.json`, `scripts/check-parity-matrix.py`, `scripts/setup-reference.sh` (`CARDANO_NODE_VERSION`), and prose mentions in `AGENTS.md` and this file. The locally-vendored install at `.reference-haskell-cardano-node/install/` may lag the policy tag; run `bash scripts/setup-reference.sh --force` to bring it up. Byte-for-byte equivalence is mandatory at every observable boundary — CBOR, hashes, signatures, network framing, leader-check arithmetic. Internals are negotiable; *wire formats are not*.

## Project

**Yggdrasil** is a pure Rust Cardano node targeting long-term protocol and serialization parity with the upstream Haskell `cardano-node`. No FFI-backed cryptography; everything is native Rust. Edition 2024, toolchain pinned to `1.95.0` (see [rust-toolchain.toml](rust-toolchain.toml)).

For the architectural picture see [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md). For the running implementation journal and operational rules see [AGENTS.md](AGENTS.md). Current implementation status, the rolling parity journal, and open operator-side gates live in [AGENTS.md](AGENTS.md) (Current Phase) and [docs/PARITY_SUMMARY.md](docs/PARITY_SUMMARY.md) — read those for the in-flight slice rather than relying on this file.

The five verification gates in [Commands](#commands) below must all pass before declaring work done.

## Upstream Reference Repo

A full Haskell `cardano-node` checkout (with all dependency repos AND a working compiled install) lives at [`.reference-haskell-cardano-node/`](.reference-haskell-cardano-node/) — **read this for upstream parity research instead of fetching from GitHub**. Layout falls into three operational tiers:

**Source — upstream repos for parity research and grep-target:**

| Local path | Upstream repo / role |
| --- | --- |
| `.reference-haskell-cardano-node/cardano-node/` | `cardano-node` Haskell binary sources (`Cardano.Node.*`) |
| `.reference-haskell-cardano-node/cardano-submit-api/` | tx submission service |
| `.reference-haskell-cardano-node/cardano-testnet/` | testnet harness |
| `.reference-haskell-cardano-node/cardano-tracer/` | trace forwarder service |
| `.reference-haskell-cardano-node/configuration/cardano/` | shipped genesis + topology configs |
| `.reference-haskell-cardano-node/deps/cardano-base/` | crypto primitives, base types, slotting |
| `.reference-haskell-cardano-node/deps/cardano-cli/` | `cardano-cli` operator CLI sources |
| `.reference-haskell-cardano-node/deps/cardano-ledger/` | per-era ledger rules + CDDL schemas |
| `.reference-haskell-cardano-node/deps/ouroboros-consensus/` | consensus protocol, ChainDB, mempool |
| `.reference-haskell-cardano-node/deps/ouroboros-network/` | mux, mini-protocols, peer selection |
| `.reference-haskell-cardano-node/deps/plutus/` | Plutus core, CEK machine, builtins |
| `.reference-haskell-cardano-node/deps/hermod-tracing/` | `trace-dispatcher` package — `Cardano.Logging.*` (`TraceObject` + its `Serialise` instance) |

**Binaries — compiled Haskell tooling for forensic comparison:**

| Local path | What it gives you |
| --- | --- |
| `.reference-haskell-cardano-node/install/bin/cardano-node` | the live Haskell node (run side-by-side for sync-rate / tip parity) |
| `.reference-haskell-cardano-node/install/bin/cardano-cli` | NtC client for tip/protocol-params/utxo queries |
| `.reference-haskell-cardano-node/install/bin/db-analyser` | dump blocks/headers/txs from a Haskell ChainDB (R251/R253 forensic helper) |
| `.reference-haskell-cardano-node/install/bin/db-synthesizer` | synthesize a chain for stress tests |
| `.reference-haskell-cardano-node/install/bin/db-truncater` | rewind a ChainDB to an earlier point |
| `.reference-haskell-cardano-node/install/bin/cardano-tracer` | trace receiver for the trace-forwarder mini-protocol |
| `.reference-haskell-cardano-node/install/bin/cardano-submit-api` / `cardano-testnet` / `dmq-node` / `bech32` | additional operator tooling |

**Operator artifacts — per-network configs and live runtime data:**

| Local path | What it contains |
| --- | --- |
| `.reference-haskell-cardano-node/install/share/{mainnet,preprod,preview}/` | per-network operator bundle (`config.json`, `topology.json`, `peer-snapshot.json`, `checkpoints.json`, `tracer-config.json`, `byron-genesis.json`, `shelley-genesis.json`, `alonzo-genesis.json`, `conway-genesis.json`, `submit-api-config.json`) — these are the canonical files yggdrasil's `node/configuration/{mainnet,preprod,preview}/` mirrors |
| `.reference-haskell-cardano-node/install/run/<network>/db/` | live ChainDB of an actively-syncing Haskell node (use with `db-analyser` for byte-level on-chain comparison) |
| `.reference-haskell-cardano-node/install/run/<network>/socket/` | NtC socket of the running Haskell node |
| `.reference-haskell-cardano-node/install/run/<network>/log/` | the Haskell node's log output |
| `.reference-haskell-cardano-node/install/run-node.sh` | startup script wired to `share/<network>/` |
| `.reference-haskell-cardano-node/install/cardano-node-<VERSION>-sha256sums.txt` | binary-bundle checksum manifest for the locally-vendored install (may lag the policy tag tracked in `docs/parity-matrix.json`) |

The vendored checkout is gitignored and authoritative for upstream Haskell module references. When reading or grepping upstream code, use these paths so the agent operates on a stable, locally-versioned tree rather than HEAD-of-master at GitHub. The compiled binaries under `install/bin/` are the canonical reference for running a Haskell node in parallel during a parity rehearsal — see `node/scripts/parallel_blockfetch_soak.sh` and `node/scripts/compare_tip_to_haskell.sh` for harnesses that wire to them. **Do not edit anything under `.reference-haskell-cardano-node/`.**

## AGENTS.md Files Are Primary Context

Every meaningful subdirectory has an `@AGENTS.md`. They are operational, kept current, and take precedence over this file for local details. **Read them before changing code in a given area, and update them after changes.** The root [AGENTS.md](AGENTS.md) is very long — prefer targeted reads with `offset`/`limit` over full reads.

| Path | Purpose |
| --- | --- |
| [AGENTS.md](AGENTS.md) | Workspace-wide rules, upstream references, and rolling implementation journal |
| [crates/AGENTS.md](crates/AGENTS.md) | Crate-index conventions and dependency direction |
| [crates/crypto/AGENTS.md](crates/crypto/AGENTS.md) | Hashing, Ed25519, VRF, KES, BLS12-381 |
| [crates/ledger/AGENTS.md](crates/ledger/AGENTS.md) | Era types, rules, governance, Plutus integration trait |
| [crates/storage/AGENTS.md](crates/storage/AGENTS.md) | `ImmutableStore`/`VolatileStore`/`LedgerStore` and `ChainDb` |
| [crates/consensus/AGENTS.md](crates/consensus/AGENTS.md) | Praos, KES/OpCert, `ChainState`, nonce evolution |
| [crates/consensus/src/mempool/AGENTS.md](crates/consensus/src/mempool/AGENTS.md) | Fee-ordered mempool, TTL, eviction, TxSubmission accounting |
| [crates/network/AGENTS.md](crates/network/AGENTS.md) | Mux, mini-protocols, governor, peer registry, diffusion types |
| [crates/plutus/AGENTS.md](crates/plutus/AGENTS.md) | CEK machine, builtins, cost model |
| [crates/tools/cardano-cli/AGENTS.md](crates/tools/cardano-cli/AGENTS.md) | Pure-Rust port of `cardano-cli` (R289+, ~237 files mirroring 180 upstream `.hs`); Phase F bootstrap state + R298+ migration roadmap (R447: relocated under `crates/tools/`) |
| [node/AGENTS.md](node/AGENTS.md) | Runtime orchestration boundary rules |
| [node/src/AGENTS.md](node/src/AGENTS.md) | CLI, config, sync, server, block production |
| [node/configuration/AGENTS.md](node/configuration/AGENTS.md) | Vendored mainnet/preprod/preview operator configs |
| [docs/AGENTS.md](docs/AGENTS.md) | Architecture/dependency/spec/contributing docs policy |
| [scripts/AGENTS.md](scripts/AGENTS.md) | CI parity validators (`check-strict-mirror`, `check-parity-matrix`, `check-fixture-manifest`, `check-reference-artifacts`) + `setup-reference.sh` |
| [specs/AGENTS.md](specs/AGENTS.md) | Pinned CDDL fixtures and provenance |
| [specs/upstream-test-vectors/AGENTS.md](specs/upstream-test-vectors/AGENTS.md) | Vendored upstream vectors (must not be hand-edited) |
| [.claude/AGENTS.md](.claude/AGENTS.md) | Claude Code harness config: session-start hook, permissions, Stop hook, subagents, skills, filetree, slash commands |

## Commands

Workspace aliases (defined in [.cargo/config.toml](.cargo/config.toml))
plus the strict-mirror drift-guard:

```bash
cargo fmt --all -- --check                   # rustfmt gate
cargo check-all                              # cargo check --workspace --all-targets
cargo test-all                               # cargo test  --workspace --all-features
cargo lint                                   # cargo clippy --workspace --all-targets --all-features -- -D warnings
python3 scripts/check-strict-mirror.py --fail-on-violation
                                             # strict 1:1 file-mirror drift-guard (R288: fail-build)
```

All five are the required verification expectations before declaring work done. CI ([.github/workflows/ci.yml](.github/workflows/ci.yml)) runs the same set.

Other parity-flow gates (run when the touched area is in scope):

```bash
python3 scripts/check-parity-matrix.py            # validates docs/parity-matrix.json schema + on-disk paths (CI gate)
python3 scripts/check-fixture-manifest.py         # cross-checks cardano-base SHA pin + vendored corpus (CI gate)
python3 scripts/check-reference-artifacts.py      # validates .reference-haskell-cardano-node/install/ tree (local only)
python3 scripts/audit-strict-mirror.py            # rebuild docs/strict-mirror-audit.tsv after a rename/annotate change
python3 .claude/scripts/filetree.py check         # reports stale .claude/filetree/manifest.json entries
bash    scripts/setup-reference.sh                # refresh .reference-haskell-cardano-node/ to the policy tag
```

The first two (`check-parity-matrix.py` + `check-fixture-manifest.py`) run in CI on every build because they operate on checked-in files. `check-reference-artifacts.py` requires the vendored 1.3 GB upstream install tree (built by `setup-reference.sh`); it's a local/operator check, not a CI gate.

Parity-flow surfaces:

- [`docs/parity-matrix.json`](docs/parity-matrix.json) — Rust ↔ Haskell parity inventory (validated by `scripts/check-parity-matrix.py`; reference tag tracked at `reference.tag`, currently `11.0.1`).
- [`docs/strict-mirror-audit.tsv`](docs/strict-mirror-audit.tsv) — per-file strict 1:1 verdict table (a/c verified, c-needed scheduled, NEEDS-REVIEW pending hand-grade). The CI drift-guard reads this as its allowlist.
- [`docs/upstream-haskell-files.txt`](docs/upstream-haskell-files.txt) — flat-file index of every `.hs` under the vendored upstream tree, rebuilt by `scripts/setup-reference.sh`.
- [`.claude/skills/round-extraction/SKILL.md`](.claude/skills/round-extraction/SKILL.md) — authoring-time strict-mirror skill; CI drift-guard is its runtime counterpart. Every new sub-module file MUST have either a strict upstream `.hs` mirror OR a `## Naming parity` docstring stanza ending with `**Strict mirror:** none.` plus the upstream symbols/files surfaced.
- [`.claude/agents/haskell-reference-auditor.md`](.claude/agents/haskell-reference-auditor.md) — read-heavy parity-comparison subagent; delegate to it before claiming parity.
- [`.claude/agents/filetree-reviewer.md`](.claude/agents/filetree-reviewer.md) — filetree-description maintainer subagent.
- [`.claude/skills/cardano-filetree-maintainer/SKILL.md`](.claude/skills/cardano-filetree-maintainer/SKILL.md) — invoke for filetree maintenance.
- Slash commands: `/parity-check`, `/filetree-check`, `/parity-plan <feature>`.

### Codespace setup (Claude Code on the web)

[`.claude/hooks/session-start.sh`](.claude/hooks/session-start.sh), registered in [`.claude/settings.json`](.claude/settings.json), runs at session start in the web environment (gated on `$CLAUDE_CODE_REMOTE`):

- Provisions the pinned `1.95.0` toolchain via [`rust-toolchain.toml`](rust-toolchain.toml).
- Ensures `pkg-config` / `build-essential` are present.
- Pre-fetches workspace dependencies with `cargo fetch --locked`.
- Runs async (`{"async": true, "asyncTimeout": 300000}`); the agent loop may begin before the hook finishes.

`.claude/settings.json` also allow-lists the cargo/git commands needed for the five verification gates and adds a `Stop` hook that re-prints the verification reminder. Local sessions are unaffected.

### Running a single test

```bash
cargo test -p yggdrasil-ledger --test <integration_test_name>      # one integration test file
cargo test -p yggdrasil-network <substring>                        # by name substring within a crate
cargo test -p yggdrasil-node --lib <mod::path::test_name> -- --exact
```

Crate package names use the `yggdrasil-` prefix: `yggdrasil-crypto`, `yggdrasil-ledger`, `yggdrasil-storage`, `yggdrasil-consensus` (now contains `mempool` submodule), `yggdrasil-network`, `yggdrasil-plutus`, plus the `yggdrasil-node` binary.

## Workspace Topology and Dependency Order

Crates form a strict dependency stack — respect this direction when adding cross-crate calls:

1. `crates/crypto` — Blake2b, Ed25519, VRF (std + batchcompat, ietfdraft03/13), KES (Simple + Sum depth 0–6+), BLS12-381 (PlutusV3/CIP-0381), secp256k1 (ECDSA + BIP-340).
2. `crates/ledger` + `crates/storage` — era types Byron→Conway, multi-era UTxO, all per-era apply rules, epoch boundary, governance enactment, PPUP, MIR, ratification engine; trait-based `ImmutableStore`/`VolatileStore`/`LedgerStore` plus a `ChainDb` coordinator with file-backed implementations, crash-recovery, rollback-time checkpoint replay, and opaque slot-indexed ChainDepState sidecar helpers. Era-specific `CborEncode`/`CborDecode` impls under `crates/ledger/src/eras/*/cbor.rs` are hand-coded against upstream CDDL (`.reference-haskell-cardano-node/deps/cardano-ledger/eras/<era>/impl/cddl/data/<era>.cddl`) — not auto-generated — because real upstream parity needs per-era Byron / array-vs-map / optional-field semantics that CDDL underspecifies.
3. `crates/consensus` + `crates/consensus/src/mempool` — Praos leader election, OpCert/KES checks, `ChainState`, nonce evolution (TPraos + Praos), per-pool OpCert counter monotonicity; fee-ordered mempool with TTL, block-application eviction, ledger revalidation, TxSubmission inbound byte/count accounting.
4. `crates/network` — SDU framing, mux, handshake, all five mini-protocols (ChainSync, BlockFetch, KeepAlive, TxSubmission2, PeerSharing) with typed client + server drivers, peer registry, root providers, ledger-peer provider, governor decision engine, inbound governor, connection manager, diffusion types, `blockfetch_pool`.
5. `crates/plutus` — CEK machine, builtin semantics, cost model (used via the `PlutusEvaluator` trait in `ledger`).
6. `node/` — thin orchestration layer. `clap` CLI, JSON/YAML config, sync runtime, inbound server, governor loop, block producer, tracer/metrics, NtC local socket dispatcher.

The `node/` crate **must stay an integration layer**. Reusable policy, peer-selection state, or protocol-facing state machines belong in `crates/*`, not in `node/`. **Extraction rule:** move logic out of `node/` as soon as it is reused across runtime paths, owns non-trivial protocol/peer state, or needs tests independent of the CLI entry point.

## Node Binary Surface

`yggdrasil-node` exposes the following `clap` subcommands (see [node/src/cli.rs](node/src/cli.rs) for definitions and [node/src/AGENTS.md](node/src/AGENTS.md) for flags):

- `run` — connect, sync, serve inbound peers, run governor + optional block producer.
- `validate-config` — operator preflight for config, peer-snapshot inputs, recovery state, genesis-hash integrity, governor sanity, KES/Praos invariants.
- `status` — inspect on-disk storage and report sync position, block counts, checkpoint state, ledger counts.
- `default-config` — emit the default JSON config to stdout.
- `cardano-cli` — pure-Rust subset (`version`, `show-upstream-config`, `query-tip`).
- `query` (Unix) — NtC LocalStateQuery dispatcher plus upstream era-specific LSQ surface verified through `cardano-cli`; see [node/src/AGENTS.md](node/src/AGENTS.md) for the exact current dispatcher inventory.
- `submit-tx` (Unix) — NtC LocalTxSubmission with `0x`-prefix-tolerant `--tx-hex`.
- `query tx-mempool` (Unix) — NtC LocalTxMonitor (`info`/`next-tx`/`tx-exists`).

`--network mainnet|preprod|preview` selects a preset; `--config` overrides the file; individual flags (`--peer`, `--network-magic`, `--port`, `--host-addr`, `--database-path`, `--topology`, `--metrics-port`, `--max-concurrent-block-fetch-peers`, etc.) override config-file values.

## Upstream Parity Is Non-Negotiable

- **Spec priority:** (1) formal ledger specifications and protocol papers, (2) Cardano CDDL schemas, (3) accepted CIPs, (4) Haskell implementation behavior.
- **Naming:** type and function names must stay as close to upstream Haskell terminology as practical (e.g. `HeaderBody`, `ChainState`, `EnactState`, `PeerTxState`, `GovernorState`, `ConnectionManagerCounters`).
- **Research:** consult the relevant local Haskell source under [`.reference-haskell-cardano-node/`](.reference-haskell-cardano-node/) before introducing local terminology, behavior, or design. `pragma-org/amaru` and `txpipe/dolos` may be consulted as Rust-port examples but are not authoritative.
- **Serialization & crypto:** byte-accurate CBOR serialization and cryptographic parity are long-term targets even when an implementation slice is incomplete. Generated artifacts must remain reproducible.
- **Dependencies:** new dependencies must be justified in [docs/DEPENDENCIES.md](docs/DEPENDENCIES.md) before being treated as accepted.
- **No FFI:** FFI cryptography and hidden native dependencies are forbidden.

### Upstream References

The full upstream Haskell repo + Haddock + ops-book reference list and the per-crate mapping (which Haskell module each Rust crate mirrors) live in [AGENTS.md](AGENTS.md). Anchor every parity-sensitive change to one of those upstream sources before introducing local terminology, behavior, or design.

When citing upstream paths in commits, comments, and journal entries, use the local reference path (e.g. `.reference-haskell-cardano-node/deps/ouroboros-network/.../PeerSelection/Governor.hs`) rather than github URLs — the local checkout is gitignored and stable across sessions.

## Workspace Lints

Denied in [Cargo.toml](Cargo.toml) `[workspace.lints.clippy]`: `dbg_macro`, `todo`, `unwrap_used`. `cargo lint` also enforces `-D warnings` workspace-wide. `yggdrasil-crypto` is compiled at `opt-level = 3` in both `dev` and `test` profiles to keep hashing/VRF test times reasonable.

Tests can opt out of `unwrap_used` via the per-crate `#![cfg_attr(test, allow(clippy::unwrap_used))]` attribute already present in `lib.rs`/`main.rs` files.

## Style

- Typesafe Rust with proper Rustdocs on public APIs when behavior is non-obvious.
- Prefer incremental milestones that compile and test cleanly over speculative completeness.
- Keep `AGENTS.md` files operational (actionable rules + current status), not long-form documentation.
- When a folder's `AGENTS.md` is outdated, missing, or incorrect, update it as part of the change.
- Use upstream Haskell module references in commit messages, comments, and journal entries (e.g. `Ouroboros.Network.PeerSelection.Governor`, `Cardano.Ledger.Conway.Rules.Utxo`) so parity work and fixture comparison remain tractable.
- Avoid hand-editing vendored data under `node/configuration/*`, `specs/upstream-test-vectors/`, and `.reference-haskell-cardano-node/`.
- Treat dated files under `docs/operational-runs/` as historical evidence. Update living status in `README.md`, `AGENTS.md`, `docs/archive/PARITY_PLAN.md`, `docs/PARITY_SUMMARY.md`, `docs/PARITY_PROOF.md`, `docs/UPSTREAM_PARITY.md`, and the manual/runbook rather than rewriting old run records, except to add a new run or correct a factual typo in that same record.
