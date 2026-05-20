# Root agent for the Yggdrasil Rust Cardano node workspace

## Agent Instructions
- You are implementing a pure typesafe Rust Cardano node with no FFI dependencies, aiming for full feature parity with the official Haskell node while maintaining strict alignment with upstream behavior, naming, and design patterns.
- You are focused on deterministic parsing, byte-accurate serialization, and reproducible generated artifacts. 
- You are researching the official [IntersectMBO github repositories](.reference-haskell-cardano-node/deps/) for guidance on design and behavior decisions, and you are documenting your implementation work with reference to the official node and upstream sources.
- You are maintaining a clear separation between different subsystems in the workspace and favoring incremental milestones that compile and test cleanly over speculative completeness. 
- You are writing typesafe Rust code with proper Rustdocs for public APIs when behavior is non-obvious. You are keeping all `AGENTS.md` files up to date with actionable guidance, context and references for future implementation work in each area of the codebase.

## Mission
- Maintain a production-oriented Cargo workspace for a long-lived Cardano node implementation.
- Preserve deterministic behavior, byte-accurate serialization goals, and clear crate boundaries.
- Favor interfaces and tests that support staged delivery over speculative completeness.

## Spec Priority
1. Formal ledger specifications and protocol papers
2. Cardano ledger CDDL schemas
3. Accepted Cardano improvement proposals
4. Haskell implementation behavior for compatibility verification

## Workspace Boundaries
- `crates/crypto` owns cryptographic primitives and related encodings.
- `crates/ledger` owns ledger state transitions and era modeling. Per-era `CborEncode`/`CborDecode` impls under `crates/ledger/src/eras/*/cbor.rs` are hand-coded against upstream CDDL (`.reference-haskell-cardano-node/deps/cardano-ledger/eras/<era>/impl/cddl/data/`); CDDL is treated as authoritative documentation, not as input for code generation.
- `crates/storage` owns durable storage and snapshot interfaces.
- `crates/consensus` owns chain selection, leader election, and rollback rules.
- `crates/consensus/src/mempool` owns transaction intake and ordering.
- `crates/network` owns multiplexing, mini-protocols, and peer management.
- `crates/node/` owns orchestration, CLI, and runtime integration.
- `specs/upstream-test-vectors` holds official test vectors from the `IntersectMBO` repositories.

## Scope
- This root file defines workspace-wide defaults for naming, upstream parity expectations, and cross-crate boundaries.
- Subdirectory `AGENTS.md` files override this file for local implementation details and should stay concise and operational.

## Official Upstream References *"Always research references and add or update links as needed"*
### Cryptography (`crates/crypto`)
- [Crypto abstractions (hashing, signatures, VRF, KES)](.reference-haskell-cardano-node/deps/cardano-base/cardano-crypto-class)
- [Praos VRF and KES implementations](.reference-haskell-cardano-node/deps/cardano-base/cardano-crypto-praos)
- [Peras-era crypto extensions](.reference-haskell-cardano-node/deps/cardano-base/cardano-crypto-peras)

### Ledger (`crates/ledger`)
- [Ledger repository (eras, libs, formal specs)](.reference-haskell-cardano-node/deps/cardano-ledger/)
- [Per-era rule implementations](.reference-haskell-cardano-node/deps/cardano-ledger/eras) (each era has `impl/`, `formal-spec/`, and `impl/cddl/data/<era>.cddl`)
- [Per-era CDDL schemas (Byron through Conway)](.reference-haskell-cardano-node/deps/cardano-ledger/eras) — authoritative for the wire-format hand-coded under `crates/ledger/src/eras/*/cbor.rs`.
- [Binary serialization library](.reference-haskell-cardano-node/deps/cardano-ledger/libs/cardano-ledger-binary)
- [Ledger support libraries](.reference-haskell-cardano-node/deps/cardano-ledger/libs)
- [Formal ledger specifications (Agda)](https://github.com/IntersectMBO/formal-ledger-specifications)
- [Published formal spec site](https://intersectmbo.github.io/formal-ledger-specifications/site)

### Storage (`crates/storage`)
- [ChainDB, ImmutableDB, VolatileDB, LedgerDB](.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Storage)
- [Consensus storage documentation and tech reports](.reference-haskell-cardano-node/deps/ouroboros-consensus/docs)
- [LedgerDB Haddock `openDB` restore/replay semantics](https://ouroboros-consensus.cardano.intersectmbo.org/haddocks/ouroboros-consensus/Ouroboros-Consensus-Storage-LedgerDB.html)
- [Caught-up node storage model](https://ouroboros-consensus.cardano.intersectmbo.org/docs/explanations/node_tasks/)
- [UTxO-HD rollback/snapshot design](https://ouroboros-consensus.cardano.intersectmbo.org/docs/references/miscellaneous/utxo-hd/utxo-hd_in_depth/)

### Consensus (`crates/consensus`)
- [Core consensus protocol modules](.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Protocol)
- [Cardano-specific consensus integration (Praos, TPraos)](.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus-protocol/src/ouroboros-consensus-protocol/Ouroboros/Consensus/Protocol)
- [Formal consensus Agda specification](.reference-haskell-cardano-node/deps/ouroboros-consensus/docs/agda-spec)
- [Consensus tech report](https://ouroboros-consensus.cardano.intersectmbo.org/pdfs/report.pdf)

### Mempool (`crates/consensus/src/mempool`)
- [Consensus Mempool module (API, TxSeq, Capacity, Init)](.reference-haskell-cardano-node/deps/ouroboros-consensus/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Mempool)
- [Transaction submission API](.reference-haskell-cardano-node/cardano-submit-api)

### Network (`crates/network`)
- [Networking repository root](.reference-haskell-cardano-node/deps/ouroboros-network/)
- [Multiplexer implementation](.reference-haskell-cardano-node/deps/ouroboros-network/network-mux)
- [Framework and handshake layer](.reference-haskell-cardano-node/deps/ouroboros-network/ouroboros-network/framework/lib/Ouroboros/Network/)
- [Mini-protocol implementations (ChainSync, BlockFetch, TxSubmission, KeepAlive, PeerSharing)](.reference-haskell-cardano-node/deps/ouroboros-network/ouroboros-network/protocols/lib/Ouroboros/Network/Protocol/)
- [Outbound governor and peer selection](.reference-haskell-cardano-node/deps/ouroboros-network/ouroboros-network)
- [Shelley networking spec PDF](https://ouroboros-network.cardano.intersectmbo.org/pdfs/network-spec)
- [Network design document](https://ouroboros-network.cardano.intersectmbo.org/pdfs/network-design)

### Plutus (`crates/plutus`)
- [Plutus core repository](.reference-haskell-cardano-node/deps/plutus/)
- [CEK machine](.reference-haskell-cardano-node/deps/plutus/plutus-core/untyped-plutus-core/src/UntypedPlutusCore/Evaluation/Machine/Cek)
- [Builtin semantics](.reference-haskell-cardano-node/deps/plutus/plutus-core/plutus-core/src/PlutusCore/Default/Builtins.hs)
- [Cost model parameters](.reference-haskell-cardano-node/deps/plutus/plutus-core/cost-model)

### Node (`crates/node/`)
- [Node integration repository](.reference-haskell-cardano-node/cardano-node/)
- [Node runtime and packaging](.reference-haskell-cardano-node/cardano-node)
- [Network configuration files](.reference-haskell-cardano-node/configuration)
- [Transaction submit API](.reference-haskell-cardano-node/cardano-submit-api)

### Cross-Cutting Documentation
- [Cardano developer portal](https://github.com/cardano-foundation/developer-portal/tree/staging/docs/)
- [Cardano blueprint](https://github.com/cardano-scaling/cardano-blueprint/tree/main/src) or [https://cardano-scaling.github.io/cardano-blueprint/](https://cardano-scaling.github.io/cardano-blueprint/)
- [Haddock documentation: ledger](https://cardano-ledger.cardano.intersectmbo.org/), [consensus](https://ouroboros-consensus.cardano.intersectmbo.org/haddocks/), [network](https://ouroboros-network.cardano.intersectmbo.org/)

##  Rules *Non-Negotiable*
- Always write typesafe Rust code.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant `AGENTS.md` file.
- Always research the official relevant upstream IntersectMBO repositories before introducing any local terminology, behavior, or design that is not directly traceable to an upstream source.
- New dependencies MUST be justified in `docs/DEPENDENCIES.md` before they are treated as accepted.
- FFI-backed cryptography and hidden native dependencies MUST NOT be introduced.
- Generated artifacts MUST remain reproducible and generated code MUST NOT be edited by hand.
- Implementation work MUST favor incremental milestones that compile and test cleanly.
- Public modules, types, and functions MUST have proper Rustdocs whenever behavior is non-obvious or externally consumed.
- Explanations of behavior or naming MUST be cross-checked against the official `cardano-node` and the relevant upstream IntersectMBO repositories.
- Type and function naming MUST stay as close to upstream terminology as practical so parity work and fixture comparison remain tractable.
- Cryptographic, protocol, and serialization parity with the official node is a non-negotiable long-term target even when an implementation slice is still incomplete.
- When you do not know how to proceed after researching the official node and upstream IntersectMBO repositories, you may review [Amaru Rust node github repo](https://github.com/pragma-org/amaru/) and [Dolos Data-node github repo](https://github.com/txpipe/dolos/) for examples of how other Rust Cardano projects have approached similar problems, but do not treat them as authoritative sources for design or behavior decisions.
- Refer to and update `docs/ARCHITECTURE.md`, `docs/DEPENDENCIES.md`, `docs/SPECS.md`, `docs/CONTRIBUTING.md`, `docs/archive/UPSTREAM_RESEARCH.md`, `docs/UPSTREAM_PARITY.md`, `docs/PARITY_SUMMARY.md`, `docs/PARITY_PROOF.md`, `docs/COMPLETION_ROADMAP.md`, `docs/archive/PARITY_PLAN.md`, and `docs/MANUAL_TEST_RUNBOOK.md` for project details and keep `./README.md` updated.
- The reference parity target is **always the latest IntersectMBO/cardano-node release tag** (currently `11.0.1`). When upstream ships a new tag, bump in lockstep across `scripts/setup-reference.sh` (`CARDANO_NODE_VERSION`), `scripts/check-parity-matrix.py` (`REFERENCE_TAG` + `ALLOWED_STATUS`), `docs/parity-matrix.json` (`reference.tag` + every `haskell_reference.path`), and prose mentions in `CLAUDE.md` / this file. Pinning to a stale tag silently invalidates parity claims.


## Verification Expectations

Cargo gates (every round MUST pass all four before declaring work done):

- `cargo fmt --all -- --check`
- `cargo check-all`
- `cargo lint`
- `cargo test-all`

Parity-flow gates (run when the touched area is in scope):

- `python3 scripts/check-parity-matrix.py` — validates `docs/parity-matrix.json` schema + every `haskell_reference.path` and `rust_surface.path` exists on disk. Required when matrix entries, status, or paths change. **CI gate** since R303.
- `python3 scripts/check-strict-mirror.py` — strict 1:1 file-mirror drift-guard. Walks production `.rs` files and flags any new file lacking either an upstream `.hs` mirror (by snake_case-of-PascalCase basename match) or a `## Naming parity` docstring stanza. Reads `docs/strict-mirror-audit.tsv` as the allowlist. Warn-only since R275 (`continue-on-error: true` in CI); flips to fail-build at R288.
- `python3 scripts/check-stale-placement.py` — post-reorganization path and status guard. Fails if current code, CI, generated navigation, commands, resolved Cargo metadata, current operational-run notes, `[Unreleased]` changelog entries, living docs, or exact filesystem paths point back at the legacy node-local crate, nested `crates/node/*/{configuration,scripts}` operator-artifact directories, root/yggdrasil-node shorthand metadata, tests, configuration, script, tool-crate, or Claude-skill placements. It also rejects stale current-status claims from the cleanup arc, including obsolete node-local LSQ wording, old cardano-cli subcommand counts or three-command subset wording, old cardano-cli gate wording for tx-generator/cardano-testnet, and the closed workspace-member gap. It requires the accepted replacement placements to exist: `crates/node/cardano-node/`, the canonical root `configuration/` operator bundles, root operator/reference scripts, and `.claude/skills/cardano-haskell-node/`; every tracked root `scripts/*.sh` must remain executable in the Git index; release/repro workflows, Docker packaging, and the release installer must stage/copy/install root `configuration/` and `scripts/` from their accepted locations. It bucket-checks Cargo metadata so the shipped `yggdrasil-node` package stays at `crates/node/cardano-node/`, node support packages stay under `crates/node/`, and sister-tool packages stay under `crates/tools/`. It also fails if the vendored Haskell reference snapshot contains nested `.git` metadata, is not ignored by Git, is declared/tracked as a Git submodule, or has any regular file in the Git index. Tagged changelog history, old operational-run records, and run logs are excluded. When editing the guard, run `python3 scripts/check-stale-placement.py --self-test` as well.
- `python3 scripts/check-fixture-manifest.py` — cross-checks the `cardano-base` SHA pin across `crates/node/config/src/upstream_pins.rs::UPSTREAM_CARDANO_BASE_COMMIT`, `specs/upstream-test-vectors/cardano-base/<SHA>/`, `docs/SPECS.md`, and `docs/UPSTREAM_PARITY.md`; verifies every required upstream-vendored fixture corpus is present + non-empty. **CI gate** since R303.
- `python3 scripts/check-reference-artifacts.py` — validates the vendored Haskell `.reference-haskell-cardano-node/install/` tree: required binaries are present + executable, every per-network share dir carries the canonical operator-config bundle, and `cardano-node --version` reports the policy tag from `docs/parity-matrix.json::reference.tag`. Linux/WSL local/operator gate (CI does not carry the 1.3 GB install tree).
- `python3 scripts/audit-strict-mirror.py` — rebuilds `docs/strict-mirror-audit.tsv` after Phase B graduates rows. Required when the audit-table verdict for a Rust file changes (e.g., `git mv` rename, new `## Naming parity` block, or a new file lands).
- `python3 .claude/scripts/filetree.py check` — flags stale `.claude/filetree/manifest.json` description entries. Required when filename-mirror restructures (R271-style) move tracked files.
- `bash scripts/setup-reference.sh [--force]` — refreshes `.reference-haskell-cardano-node/` to the policy IntersectMBO tag. Required when the tag bumps or the local reference snapshot drifts.

**Strict 1:1 file-mirror policy (R274 onward).** Every production `.rs`
under `crates/<crate>/src/` and `crates/node/*/src/` either mirrors a single
canonical upstream `.hs` file by snake_case basename (with directory-
prefix fallback for sibling collisions like `Rules/OCert.hs` →
`rules_ocert.rs`), OR carries a `## Naming parity` docstring stanza
ending in `**Strict mirror:** none.` plus the upstream symbol(s)/
file(s) the helper surfaces and the reason a strict file mirror does
not exist. The authoring-time skill is at
[`.claude/skills/round-extraction/SKILL.md`](.claude/skills/round-extraction/SKILL.md);
the CI counterpart is `python3 scripts/check-strict-mirror.py`. The
allowlist source-of-truth is
[`docs/strict-mirror-audit.tsv`](docs/strict-mirror-audit.tsv).

Parity-flow surfaces:

- [`docs/parity-matrix.json`](docs/parity-matrix.json) — Rust ↔ Haskell parity inventory; `reference.tag` tracks the latest IntersectMBO/cardano-node release.
- [`docs/strict-mirror-audit.tsv`](docs/strict-mirror-audit.tsv) — per-file strict 1:1 verdict table (a/c verified, c-needed scheduled, NEEDS-REVIEW pending hand-grade). The CI drift-guard reads this as its allowlist.
- [`docs/upstream-haskell-files.txt`](docs/upstream-haskell-files.txt) — flat-file index of every `.hs` under the vendored upstream tree, rebuilt by `scripts/setup-reference.sh`.
- [`.claude/agents/haskell-reference-auditor.md`](.claude/agents/haskell-reference-auditor.md) — read-heavy parity-comparison subagent; delegate before claiming parity, before recommending implementation, or when a fix needs upstream evidence cited.
- [`.claude/agents/round-extractor.md`](.claude/agents/round-extractor.md) — filename-mirror extraction specialist for one R-arc round.
- [`.claude/skills/round-extraction/SKILL.md`](.claude/skills/round-extraction/SKILL.md), [`.claude/skills/parity-plan/SKILL.md`](.claude/skills/parity-plan/SKILL.md), [`.claude/skills/continuous-agent-loop/SKILL.md`](.claude/skills/continuous-agent-loop/SKILL.md) — encoded recipes from R271/R273 arcs.
- Slash commands: `/four-gates`, `/parity-check`, `/filetree-check`, `/parity-plan <feature>`, `/round-doc <round-id> <slug>`, `/setup-reference`.

Codespace bootstrap:

- Claude Code on the web: [`.claude/hooks/session-start.sh`](.claude/hooks/session-start.sh) (registered in [`.claude/settings.json`](.claude/settings.json)) provisions the pinned 1.95.0 toolchain and pre-fetches workspace dependencies before the agent starts. Gated on `$CLAUDE_CODE_REMOTE`; local sessions run unchanged.

## Current Phase

**Yggdrasil 1.0 — code-level parity closure.** The core node (crypto, ledger,
storage, consensus, mempool, network, plutus, and the `crates/node/*` runtime
crates) is feature-complete for syncing and validating the official Cardano
networks. The strict 1:1 upstream file-mirror policy is in force and CI-gated.
The active development arc is the sister-tools port under `crates/tools/`
(13 operator binaries) plus the `cardano-cli` subcommand migration.

Current detail lives in the per-area docs, not in this file:

- Per-crate capabilities — each `crates/*/AGENTS.md`, `crates/node/*/AGENTS.md`,
  and `crates/tools/*/AGENTS.md`.
- Parity status — [`docs/PARITY_SUMMARY.md`](docs/PARITY_SUMMARY.md),
  [`docs/PARITY_PROOF.md`](docs/PARITY_PROOF.md),
  [`docs/UPSTREAM_PARITY.md`](docs/UPSTREAM_PARITY.md),
  [`docs/parity-matrix.json`](docs/parity-matrix.json).
- Remaining work to fully complete the project —
  [`docs/COMPLETION_ROADMAP.md`](docs/COMPLETION_ROADMAP.md).
- Known consolidation debt — [`docs/TECH-DEBT.md`](docs/TECH-DEBT.md).

**Open parity gaps** — these close only against a running upstream Haskell node
for wire comparison: Gap BO (preprod TPraos VRF, slot ~429460), Gap BP (preview
Plutus V2 cost-budget overrun ≈0.0185%), and the R178-followup (Conway HFC LSQ
response envelope shape).

**Remaining production gates are operator-side**: the §2–9 mainnet endurance
rehearsal and the §6.5 parallel-fetch sign-off in
[`docs/MANUAL_TEST_RUNBOOK.md`](docs/MANUAL_TEST_RUNBOOK.md).

**Verification baseline** (2026-05-17, Rust 1.95.0): `cargo fmt --all -- --check`,
`cargo check-all`, `cargo lint`, and `cargo test-all` all pass —
**6,519 tests passing, 0 failing, 3 ignored**.

The full round-by-round implementation journal (≈R104 → R503) is archived at
[`docs/archive/AGENTS_JOURNAL.md`](docs/archive/AGENTS_JOURNAL.md); it is
historical evidence, not current status.

New subfolder-level `AGENTS.md` files should only be added where a folder has a
stable domain boundary.
