<p align="center">
  <img src="docs/assets/images/Yggrasil_banner.png" alt="YggdrasilNode — A Cardano Node Project Written In Rust" width="100%"/>
</p>

# Yggdrasil Cardano Node in Rust

[![CI](https://github.com/yggdrasil-node/Cardano-node/actions/workflows/ci.yml/badge.svg)](https://github.com/yggdrasil-node/Cardano-node/actions/workflows/ci.yml)
[![Pages](https://github.com/yggdrasil-node/Cardano-node/actions/workflows/pages.yml/badge.svg)](https://github.com/yggdrasil-node/Cardano-node/actions/workflows/pages.yml)
[![Release](https://github.com/yggdrasil-node/Cardano-node/actions/workflows/release.yml/badge.svg)](https://github.com/yggdrasil-node/Cardano-node/actions/workflows/release.yml)
[![Latest release](https://img.shields.io/github/v/release/yggdrasil-node/Cardano-node?include_prereleases&sort=semver)](https://github.com/yggdrasil-node/Cardano-node/releases/latest)
[![Rust 1.95.0](https://img.shields.io/badge/rust-1.95.0-orange)](rust-toolchain.toml)
[![Tests](https://img.shields.io/badge/tests-7%2C222%20passing-brightgreen)](#current-status)

Yggdrasil is a pure Rust Cardano node workspace targeting long-term protocol and serialization parity with the upstream Cardano node.

**Documentation**: <https://yggdrasil-node.github.io/Cardano-node/> · [User Manual](https://yggdrasil-node.github.io/Cardano-node/manual/) · [Quick Start](https://yggdrasil-node.github.io/Cardano-node/manual/quick-start/) · [Releases](https://github.com/yggdrasil-node/Cardano-node/releases)

## Quick Navigation

- [Install](#install)
- [Current Status](#current-status)
- [Workspace Layout](#workspace-layout)
- [Verification](#verification)
- [Documentation](#documentation)
- [Parity Dashboard](docs/PARITY_DASHBOARD.md)
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
sudo mkdir -p /usr/local/share/yggdrasil
sudo cp -R configuration scripts /usr/local/share/yggdrasil/
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
curl -fsSL https://raw.githubusercontent.com/yggdrasil-node/Cardano-node/main/scripts/install_from_release.sh | bash
```

The installer verifies the downloaded archive against the published
`SHA256SUMS.txt`, installs the binary, and places the bundled
`configuration/` and `scripts/` trees under `/usr/local/share/yggdrasil/`.
The `--network` presets resolve from that installed configuration root by
default; set `YGGDRASIL_CONFIG_ROOT` to point at a custom root containing
`mainnet/`, `preprod/`, and `preview/`.
Full details: [Installing from Releases](https://yggdrasil-node.github.io/Cardano-node/manual/releases/).

## Current Status

### Implemented

- Cargo workspace with stable crate boundaries for crypto, ledger, storage, consensus, mempool, network, and node integration.
- **Crypto**: Blake2b-256/512, Ed25519, VRF (standard + batchcompat), SimpleKES, SumKES (depth 0–6+), with upstream vector-backed coverage and zeroize hardening.
- **Ledger**: Full era type coverage Byron through Conway. Hand-rolled CBOR codec. Multi-era UTxO (`MultiEraUtxo`, `MultiEraTxOut`) with era-aware `apply_block()` dispatch, coin/multi-asset preservation, TTL/validity-interval checks. PlutusData AST with full CBOR support. Certificate hierarchy (19 variants). Credential, address, and governance types. Epoch-boundary processing includes stake-snapshot rotation, reward distribution, governance ratification/enactment, and Shelley PPUP protocol-parameter update application with genesis-delegate quorum.
- **Network**: SDU framing, async bearer transport, mux/demux, handshake, peer lifecycle, reusable peer candidate ordering, upstream-aligned topology domain types for local and public roots, a root-provider layer that resolves, tracks, and refreshes local, bootstrap, and public roots with upstream-style precedence, a DNS-backed provider for local roots, bootstrap peers, and configured public roots with optional time-gated refresh policy (exponential backoff, upstream-aligned 60 s / 900 s clamps), a minimal peer registry that tracks peer source and cold/cooling/warm/hot status in the crate instead of `node`, including crate-owned reconciliation helpers for root, ledger, big-ledger, and peer-share source sets, and a ledger peer provider layer with `LedgerPeerProvider` trait, `LedgerPeerSnapshot` normalization (deduplicates and enforces disjoint ledger/big-ledger sets), `LedgerPeerProviderRefresh` (combined/per-kind), `apply_ledger_peer_refresh()` helper, `refresh_ledger_peer_registry()` orchestration, and `ScriptedLedgerPeerProvider` for testing. All five mini-protocol state machines + CBOR wire codecs + typed client/server drivers (ChainSync, BlockFetch, KeepAlive, TxSubmission2, PeerSharing). SDU segmentation/reassembly for large protocol messages.
- **Consensus**: Praos leader election, typed chain selection (VRF tiebreaker), epoch math, OpCert verification, KES period checks, block header verification with SumKES. `SecurityParam` (Ouroboros `k`), `ChainState` volatile chain state tracker with rollback depth enforcement and stability window detection.
- **Storage**: Trait-based `ImmutableStore`, `VolatileStore`, `LedgerStore` with in-memory and file-backed implementations, plus a minimal `ChainDb` coordinator for best-known tip recovery, volatile-prefix promotion into immutable storage, rollback-time ledger snapshot truncation, and slot-indexed ChainDepState sidecar snapshots for nonce/OpCert rollback restore.
- **Mempool**: Fee-ordered queue with `TxId`-based entries, duplicate detection, capacity enforcement, TTL-aware admission, block-application eviction.
- **Node CLI**: `clap`-based binary with `run` (connect to peer and sync), `validate-config` (operator preflight for config, peer-snapshot inputs, and any existing storage recovery state), `status` (inspect on-disk storage and report sync position, block counts, and checkpoint state), and `default-config` (emit JSON config) subcommands. JSON configuration file support with CLI flag overrides, topology/config parsing that feeds reusable network-crate topology and peer-ordering helpers, and upstream-aligned tracing fields (`TurnOnLogging`, `UseTraceDispatcher`, `TraceOptions`, `TraceOptionNodeName`, `TraceOptionForwarder`). `NodeMetrics` provides atomic operational counters wired into the hot sync loops, with `--metrics-port` exposing a Prometheus-compatible HTTP `/metrics` endpoint and a JSON `/metrics/json` endpoint on `127.0.0.1`.
- **Node sync orchestration**: Full multi-era sync pipeline from bootstrap through managed service. Multi-era block decode (all 7 era tags). Consensus header verification bridge. Block header hash computation (Blake2b-256). Ordered bootstrap relay fallback plus reconnecting verified sync on ChainSync or BlockFetch connectivity loss. Graceful shutdown via Ctrl-C signal handling. A local `NodeTracer` emits human- or machine-formatted runtime trace objects for bootstrap, reconnect, sync progress, and shutdown/failure paths. Live sync evicts confirmed and expired transactions from the shared mempool, epoch-boundary reward math uses tracked per-pool performance, and rollback recovery restores nonce/OpCert ChainDepState from sidecar history before replaying stored blocks to the selected rollback point.
- **Upstream parity**: CBOR golden round-trip tests, cross-subsystem integration tests, and wire-format field naming aligned with official Cardano CDDL specifications.
- **Validation baseline**: all four cargo gates pass as of 2026-05-26 on the pinned Rust 1.95.0 toolchain — `cargo fmt --all -- --check`, `cargo check-all`, `cargo lint`, and `cargo test-all` (**7,251 tests passing, 0 failing, 3 ignored**; 7,254 listed tests total). The strict 1:1 file-mirror drift-guard (`scripts/check-strict-mirror.py`) and the checked-in parity-flow validators (`check-parity-matrix.py`, `check-fixture-manifest.py`, `check-stale-placement.py`, `check-doc-status-headers.py`, and `.claude/scripts/filetree.py`) are clean. See [`docs/PARITY_DASHBOARD.md`](docs/PARITY_DASHBOARD.md) for the compact status board, [`docs/PARITY_SUMMARY.md`](docs/PARITY_SUMMARY.md), [`docs/PARITY_PROOF.md`](docs/PARITY_PROOF.md), and [`docs/UPSTREAM_PARITY.md`](docs/UPSTREAM_PARITY.md) for the round-by-round parity arc, and [`docs/COMPLETION_ROADMAP.md`](docs/COMPLETION_ROADMAP.md) for the remaining-work backlog.
- CI workflow and workspace cargo aliases for check/test/lint.

### Status: core parity closure in progress

As of R839, the core node has broad Rust-side implementation coverage, but final parity closure remains evidence-gated by Gap BO, Gap BP, the R178-followup LSQ response-envelope comparison, and operator soaks. The R689-R839 continuation covered tx-generator DumpToFile narrowing, db-analyser genesis-bootstrap correction, dmq-node protocol/peer-driver/registry/bundle surfaces, cardano-testnet era-aware option/parser work, typed Command payload wiring, `Testnet/Types.hs` runtime record carriers, `Testnet/Process/Cli/Keys.hs` command builders, `Testnet/Process/Cli/Transaction.hs` sign/submit/txid and spend-output txbody builders, `Testnet/Process/Cli/DRep.hs` pure key/cert/vote builders, `Testnet/Process/Cli/SPO.hs` pure certificate/vote builders, `Testnet/Process/Run.hs` flexible process wrappers, `Testnet/Process/RunIO.hs` plan-json binary-resolution/execution helpers, `Testnet/Property/Util.hs` pure harness primitives, `Testnet/Property/Assert.hs` pure plus CLI-backed stake-pool assertion helpers, and `Testnet/Property/Run.hs` pure harness-control/planning helpers, plus this workspace audit cleanup. These rounds do not close the remaining operator and wire-comparison gates tracked in [`docs/COMPLETION_ROADMAP.md`](docs/COMPLETION_ROADMAP.md): the §2-9 mainnet endurance rehearsal, the §6.5 parallel-fetch sign-off, Gap BO, Gap BP, and the R178-followup LSQ response-envelope comparison.

### Ongoing operational work

- Mainnet sync endurance testing per the runbook (Phase E.2 — 24h+ rehearsal).
- Parallel BlockFetch sign-off with live worker activation and Haskell tip comparison.
- Wire-comparison follow-up for Gap BO, Gap BP, and the R178-followup LSQ response envelope.
- Extended cardano-tracer interoperability validation.

### Remaining gates (current R839 status)

The remaining items mix unresolved parity blockers and operator-time gates:

- **Phase E.2** — 24h+ mainnet sync rehearsal. Operator-time gate; yggdrasil's mainnet sync is end-to-end working (R211+R213) and exposes the observability surface needed for sign-off.
- **Parallel BlockFetch sign-off** — runbook §6.5 remains open on current
  evidence. The current default knob is `max_concurrent_block_fetch_peers = 2`,
  and the direct bootstrap path now registers shared workers in Rust tests;
  sign-off still needs fresh live worker activation plus Haskell tip comparison
  evidence.
- **Gap BO / Gap BP / R178** - current parity blockers requiring upstream
  Haskell replay or socket comparison evidence before any full core parity
  claim.
- **Tracer interoperability** — extended `cardano-tracer` validation across the forwarder and stdout backends remains ongoing operational work.

## Workspace Layout

The workspace is a strict bottom-up dependency stack — see [crates/AGENTS.md](crates/AGENTS.md) for direction rules.

| Crate / path | Purpose |
| --- | --- |
| [crates/crypto](crates/crypto) | Blake2b, Ed25519, VRF (std + batchcompat), KES (Simple + Sum 0–6+), BLS12-381, secp256k1. |
| [crates/ledger](crates/ledger) | Era types Byron→Conway, multi-era UTxO, per-era apply rules, governance, PPUP, MIR, ratification. Per-era CBOR codecs hand-coded against upstream CDDL. |
| [crates/storage](crates/storage) | `ImmutableStore` / `VolatileStore` / `LedgerStore` traits + file-backed impls + `ChainDb` coordinator. |
| [crates/consensus](crates/consensus) | Praos leader election, KES/OpCert checks, `ChainState`, nonce evolution. |
| [crates/consensus/src/mempool](crates/consensus/src/mempool) | Fee-ordered queue with TTL, eviction, ledger revalidation. |
| [crates/network](crates/network) | Mux, mini-protocols, governor, peer registry, root + ledger-peer providers, diffusion types. |
| [crates/plutus](crates/plutus) | CEK machine, builtin semantics, cost model. |
| [crates/node](crates/node) | `yggdrasil-node` binary + extracted sub-crates — CLI, config, sync runtime, inbound server, governor loop, block producer, NtC. |
| [docs](docs) | Architecture, dependency policy, specs, parity plan, manual test runbook, user manual. |
| [specs](specs) | Pinned CDDL fixtures + vendored upstream test vectors. |

## Verification

The required gates before declaring work done are the four workflow CI runs:

```bash
cargo fmt --all -- --check   # rustfmt gate
cargo check-all              # cargo check --workspace --all-targets
cargo lint                   # cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test-all               # cargo test --workspace --all-features
```

All four must pass. The release build also runs `cargo doc --workspace --no-deps`. Aliases live in [.cargo/config.toml](.cargo/config.toml); CI lives in [.github/workflows/ci.yml](.github/workflows/ci.yml).

### Parity-flow gates

Yggdrasil tracks the **latest IntersectMBO/cardano-node release tag** as the parity target (currently `11.0.1`). Additional parity-flow gates surface parity-risk early:

```bash
python3 scripts/check-parity-matrix.py          # validates docs/parity-matrix.json
python3 scripts/check-stale-placement.py --self-test
python3 scripts/check-stale-placement.py        # flags retired paths and stale current-status gates
python3 scripts/check-doc-status-headers.py     # aligns central parity-doc headers and dashboard counts
python3 .claude/scripts/filetree.py check       # flags stale .claude/filetree manifest entries
bash    scripts/setup-reference.sh              # refresh reference snapshot + Linux/WSL install to policy tag
```

Run `check-parity-matrix.py` whenever `docs/parity-matrix.json` changes or upstream paths move between releases. Run `check-stale-placement.py` after any post-reorganization move so retired locations cannot re-enter current docs, scripts, tracked files, Cargo metadata, node-crate-local operator artifact directories, release/repro packaging, Docker packaging, or the ignored metadata-free reference snapshot, and so stale current-status claims such as obsolete cardano-cli gate wording, old subcommand counts or subset wording, and old workspace-member gaps do not re-enter living docs. It also confirms the accepted replacement placements, canonical root configuration bundles, executable root shell scripts, and root operator/reference script entrypoints remain present. Run `filetree.py check` after filename-mirror restructures (R271-style runtime split, R273-style subsystem split). Run `setup-reference.sh --sources-only` for the portable source snapshot, or the full `setup-reference.sh` under Linux/WSL when upstream ships a new release and the compiled Haskell reference install must be refreshed.

### Claude Code workflow

Slash commands wired in [.claude/commands/](.claude/commands/):

- `/four-gates` — runs the four cargo gates and reports outcomes.
- `/parity-check` — runs the parity-matrix validator.
- `/filetree-check` — runs the filetree staleness check.
- `/parity-plan <feature>` — authors a parity plan before substantive code edits.
- `/round-doc <round-id> <slug>` — authors `docs/operational-runs/YYYY-MM-DD-round-NNN-<slug>.md` for the just-completed R-arc round.
- `/setup-reference` — refresh the pinned IntersectMBO/cardano-node reference.

Sub-agents and skills under [.claude/agents/](.claude/agents/) + [.claude/skills/](.claude/skills/) encode the R-arc patterns confirmed across R271 (runtime split: 7,269 → 140 lines) and R273 (consensus + plutus subsystem splits).

For preview block-producer parity evidence, use a real registered/delegated preview pool's KES, VRF, and operational-certificate files. The generated preview harness remains useful for wallet/cert reference material and relay smoke, but its generated producer credentials are not accepted as the parity producer source.

```bash
cargo build --release --bin yggdrasil-node
RUN_ROOT=/tmp/cardano-reference PORT=3002 \
  .reference-haskell-cardano-node/install/run-node.sh preview \
  >/tmp/cardano-preview-reference.log 2>&1 &
export HASKELL_SOCK="/tmp/cardano-reference/preview/socket/node.socket"

KES_SKEY_PATH=/secure/preview/kes.skey \
VRF_SKEY_PATH=/secure/preview/vrf.skey \
OPCERT_PATH=/secure/preview/node.cert \
target/release/yggdrasil-node validate-config \
  --network preview \
  --shelley-kes-key "$KES_SKEY_PATH" \
  --shelley-vrf-key "$VRF_SKEY_PATH" \
  --shelley-operational-certificate "$OPCERT_PATH"

KES_SKEY_PATH=/secure/preview/kes.skey \
VRF_SKEY_PATH=/secure/preview/vrf.skey \
OPCERT_PATH=/secure/preview/node.cert \
HASKELL_SOCK="$HASKELL_SOCK" \
RUN_SECONDS=21600 \
TIP_COMPARE_CHECKPOINTS=900,3600,21600 \
REQUIRE_TIP_COMPARISON=1 \
EXPECT_FORGE_EVENTS=1 \
EXPECT_ADOPTED_EVENTS=1 \
scripts/run_preview_real_pool_producer.sh
```

For the epoch-1304 resume path, the wrapper below checks that the pool is
active, starts or validates the local Haskell preview relay, waits for Haskell
`syncProgress` to reach `HASKELL_SYNC_MIN_PERCENT` (default `99.00`), then runs
the same producer command with the required 6-hour comparison window:

```bash
CRED_DIR=/tmp/ygg-preview-generated-bp-... \
POOL_ID=pool1... \
scripts/run_preview_active_pool_signoff.sh
```

The runner preflights `HASKELL_SOCK` with
`cardano-cli query tip --testnet-magic 2` before starting Yggdrasil, then
requires all configured Haskell checkpoints when `REQUIRE_TIP_COMPARISON=1`.

For reference material only, `scripts/preview_producer_harness.sh` can generate a funding wallet, pool registration certificates, and pool metadata under `tmp/preview-producer/`. Defaults are ticker `RUST` and name `WORLDS FIRST RUST NODE`; override `POOL_TICKER`, `POOL_NAME`, `POOL_DESCRIPTION`, `POOL_HOMEPAGE`, or `POOL_METADATA_URL` before running `certs` when needed.

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
- Current-code §6.5 parallel BlockFetch sign-off: worker activation, Haskell
  tip comparison, and long-window soak evidence with
  `parallel_blockfetch_soak.sh`.
- Future Conway tail-parameter cost-model entries beyond the vendored 251-name surface in `crates/plutus`.

## License

Apache-2.0 — see [LICENSE](LICENSE).
