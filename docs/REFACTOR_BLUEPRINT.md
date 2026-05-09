---
title: Refactor Blueprint — Upstream Module Mapping
layout: default
parent: Reference
nav_order: 9
---

# Refactor Blueprint — Upstream Module Mapping

Last updated: 2026-05-09 (R287 closure annotation)

> **Status — all R256 phases (A through G) shipped.** This document
> was authored to plan the R256 Phase C–G monolith splits. Those
> phases have all landed via R269 (state.rs split, Phase C), R270
> (governor.rs split, Phase E), R271 (runtime.rs split, Phase D-runtime),
> R272 (epoch_boundary.rs split, Phase G), R273 (subsystem submodule
> splits), and R274–R281 (strict-mirror naming-parity sweeps). R287
> annotates every phase below with its post-shipment status; the
> remaining-work table and target sub-module tables are retained as
> historical reference.
>
> Live status of individual files is tracked in
> [`docs/strict-mirror-audit.tsv`](strict-mirror-audit.tsv) (R274
> per-file verdict table) and
> [`docs/PARITY_SUMMARY.md`](PARITY_SUMMARY.md). For the current
> R-arc state see [`README.md`](../README.md).

This document maps Yggdrasil's (now-resolved) monolith Rust files to
their upstream IntersectMBO Haskell modules so the R256 Phase C–G
refactors mirror upstream organization byte-for-byte rather than
inventing local module boundaries.

## Why this exists

R256 Phase A (mempool consolidation) and Phase B (cddl-codegen tooling
relocation) shipped clean upstream-aligned moves. Phase H+ extracted
test modules from 15 monolith files (-2.0 MB of intermixed test code).
What remains — Phases C through G — are **per-rule semantic splits**
of files in the 134-505 KB range that need careful upstream-module-by-
upstream-module decomposition. This document captures that mapping so
each future R-round splits along upstream seams, not arbitrary ones.

## Reference source layout

| Upstream repo | Where | Notes |
|---|---|---|
| `cardano-node` | `/home/daniel/Cardano-node/.reference-haskell-cardano-node/cardano-node/` | Full source vendored; use for runtime/CLI/configuration mapping |
| `cardano-cli` | `/home/daniel/Cardano-node/.reference-haskell-cardano-node/cardano-tracer/`, sibling dirs | partial |
| `cardano-ledger` | not vendored | Use `node/scripts/check_upstream_drift.sh` audit baseline; consult `IntersectMBO/cardano-ledger` directly for per-rule module names |
| `ouroboros-consensus` | not vendored | Same — audit baseline at the documentary pin |
| `ouroboros-network` | not vendored | Same |
| `plutus` | not vendored | Same |

The Haskell `cardano-node` vendor is the load-bearing reference for
Phases D and F. For Phases C, E, G the upstream module layout is
documented inline below from prior parity-audit research; cross-check
against the relevant pinned SHA in `node/src/upstream_pins.rs` before
moving any file.

## Upstream `Cardano.Node` module sizes (reference scale)

The largest single module in upstream `cardano-node` is
**`Run.hs` at 46 KB / 1,036 lines**. Yggdrasil's monoliths are
4-13× that — every Phase C-G split aims at upstream-equivalent
module sizes, not "smaller than current".

| Upstream module | Lines | Size |
|---|---|---|
| `Cardano.Node.Run` | 1,036 | 46 KB |
| `Cardano.Node.Configuration.POM` | 992 | 46 KB |
| `Cardano.Node.Types` | 541 | 20 KB |
| `Cardano.Node.Parsers` | 472 | 16 KB |
| `Cardano.Node.Configuration.Logging` | 381 | 16 KB |
| `Cardano.Node.Queries` | 376 | 13 KB |
| `Cardano.Node.Configuration.Socket` | 266 | 9 KB |
| `Cardano.Node.Startup` | 265 | 9 KB |
| `Cardano.Node.Configuration.TopologyP2P` | 163 | 7 KB |
| `Cardano.Node.Configuration.NodeAddress` | 160 | 5 KB |
| `Cardano.Node.Configuration.LedgerDB` | 151 | 6 KB |

## Phase D — runtime.rs + sync.rs split [DONE in R271 + R269]

### runtime.rs (298 KB, 7,269 production lines)

**Target sub-modules** (all under `node/src/`, mirroring `Cardano.Node.*`):

| Yggdrasil target | Upstream | Concern |
|---|---|---|
| `node/src/run.rs` | `Cardano.Node.Run` | top-level orchestration: connect, sync session loop, governor task, block-producer dispatch, NtC server bring-up |
| `node/src/startup.rs` | `Cardano.Node.Startup` | one-time startup work: load config, resolve genesis, verify hashes, seed registry |
| `node/src/configuration/pom.rs` | `Cardano.Node.Configuration.POM` | partial-options-monoid for layered config (CLI → file → defaults) |
| `node/src/configuration/logging.rs` | `Cardano.Node.Configuration.Logging` | tracer backend setup |
| `node/src/configuration/socket.rs` | `Cardano.Node.Configuration.Socket` | NtN/NtC socket binding |
| `node/src/configuration/topology_p2p.rs` | `Cardano.Node.Configuration.TopologyP2P` | bootstrap/local/public roots parsing |
| `node/src/configuration/node_address.rs` | `Cardano.Node.Configuration.NodeAddress` | listen address resolution |
| `node/src/configuration/ledger_db.rs` | `Cardano.Node.Configuration.LedgerDB` | LedgerDB backend selection |
| `node/src/handlers/shutdown.rs` | `Cardano.Node.Handlers.Shutdown` | shutdown signal handling |
| `node/src/commands/cardano_cli.rs` | `Cardano.CLI.Environment` (upstream `cardano-cli` crate) | upstream-config + network-magic discovery for the `cardano-cli` subcommand |
| `node/src/commands/submit_tx.rs` | `Cardano.CLI.Run.Transaction.Submit` (upstream `cardano-cli` crate) | NtC `LocalTxSubmission` driver for the `submit-tx` subcommand |
| `node/src/commands/tx_mempool.rs` | `Cardano.CLI.Shelley.Run.Query.runQueryTxMempool` (upstream `cardano-cli` crate) | NtC `LocalTxMonitor` driver for the `query tx-mempool` subcommand |
| `node/src/commands/query.rs` | `Cardano.CLI.Shelley.Run.Query.runQueryCmd` (upstream `cardano-cli` crate) | NtC `LocalStateQuery` driver, query CBOR encoder/decoder, `QueryCommand` enum |
| `node/src/commands/status.rs` | (Yggdrasil-only — no direct upstream analog; closest is `cardano-node`'s startup ChainDB inspection in `Cardano.Node.Run`) | on-disk read-only inspector for sync position, block counts, ledger checkpoint state, recovered-tip cardinalities |
| `node/src/commands/validate_config.rs` | `Cardano.Node.Configuration.POM` (validation pipeline upstream `nodeProtocolModeP`) + Yggdrasil-extension `validate-config` subcommand | deep operator preflight: cross-field invariants, KES, governor targets, RequiresNetworkMagic, checkpoints integrity, peer snapshot, on-disk recovery; also hosts `node_role_report` + block-producer-credential policy enforcement |
| `node/src/commands/configuration.rs` | `Cardano.Node.Configuration.POM` (partial-options-monoid that overlays CLI flags onto file config upstream) | effective-config assembly: file-or-preset loader (`load_effective_config`), per-domain CLI override application (topology, inbound listen, block-producer credentials), checkpoint-trace namespace overlay |
| `node/src/metrics_server.rs` | `Cardano.Node.Tracing.Tracers.Startup` Prometheus endpoint | loopback HTTP server for `/metrics`, `/metrics/json`, `/health`, `/debug/*` aliases on raw tokio TCP |
| `node/src/startup.rs` | `Cardano.Node.Run` genesis-loading slice + `Ouroboros.Consensus.Node.Genesis` | `trace_genesis_hashes_verified`, `strict_base_ledger_state` (verify + seed), `best_effort_base_ledger_state`, `forged_header_protocol_version` |
| `node/src/path_resolve.rs` | `Cardano.Node.Configuration.NodeAddress` / `Cardano.Node.Configuration.POM.fromConfigPath` | `resolve_storage_dir`, `resolve_config_path` — config-relative path resolution against the directory holding the config file |
| `node/src/ledger_peers.rs` | `Ouroboros.Network.PeerSelection.LedgerPeers` | `point_slot`, `ledger_peer_snapshot_from_ledger_state`, `configured_fallback_peers` — startup fallback peer assembly with R250 split-gate eligibility |
| `node/src/run_node.rs` | `Cardano.Node.Run.run` | node runtime entry point: storage recovery, tracer/metrics startup, network setup, sync runtime, optional block producer, NtC server, shutdown wait. `RunNodeRequest` driver struct and `run_node` async fn |
| `node/src/cli.rs` | `Cardano.Node.Parsers` | top-level `clap` subcommand definitions: `Command` enum (run / validate-config / status / default-config / cardano-cli / query / tx-mempool / submit-tx) + `CardanoCliCommand` enum |
| `node/src/commands/run.rs` | `Cardano.Node.Run` (orchestration of `RunNodeRequest` from CLI args) | `RunCmdArgs` struct + `run_subcommand` function — load config, apply CLI overrides, build `RunNodeRequest`, recover storage, hand off to `run_node` |
| `node/src/local_server/sessions.rs` | `Ouroboros.Network.Protocol.{LocalTxSubmission,LocalStateQuery,LocalTxMonitor}.Server` | `run_local_*_session` runners + snapshot helpers (`acquire_snapshot`, `attach_chain_dep_state_from_sidecar`, `recover_snapshot_at_point`, `encode_rejection_reason`) |
| `node/src/local_server/accept.rs` | `Ouroboros.Network.NodeToClient` server-side accept path | `run_local_client_session`, `run_local_accept_loop` — Unix socket bind + per-connection mini-protocol task spawning |

### main.rs end state

After the R257 series, `node/src/main.rs` is **199 lines** (down from
3460 at session start, **94% reduction**). Remaining content:

- `Cli` clap entry struct (5 lines)
- `main()` dispatch match (130 lines) — each arm calls into a
  `commands::*::run_*_subcommand` function with no inline logic
- module declarations + re-exports (60 lines)

This is the natural floor for a binary's `main.rs`. The dispatch arms
for `Query`, `TxMempool`, `SubmitTx` are 4-line tokio-runtime adapters
that don't benefit from extraction. The clean separation now mirrors
upstream `cardano-node`'s `app/cardano-node.hs` (small dispatch entry)
+ `Cardano.Node.Run` / `Cardano.CLI.Run.*` (per-subcommand handlers).
| `node/src/protocol/cardano.rs` | `Cardano.Node.Protocol.Cardano` | Cardano-block protocol instantiation |
| `node/src/protocol/byron.rs` | `Cardano.Node.Protocol.Byron` | Byron-only protocol instantiation |
| `node/src/protocol/shelley.rs` | `Cardano.Node.Protocol.Shelley` | Shelley-only protocol instantiation |
| `node/src/protocol/types.rs` | `Cardano.Node.Protocol.Types` | shared protocol types |

The current `runtime.rs` is a flattened version of all 13 of these
upstream modules. Each split should land as its own commit with green
gates between.

### sync.rs (357 KB, 9,448 production lines)

This file's logic is ouroboros-consensus territory (`ChainDB`,
`BlockFetch`, `ChainSync`, `LedgerDB` apply paths). Upstream
organization:

| Yggdrasil target | Upstream | Concern |
|---|---|---|
| `node/src/sync/run.rs` | `Ouroboros.Consensus.Node.Run` | reconnecting verified-sync service |
| `node/src/sync/chain_db.rs` | `Ouroboros.Consensus.Storage.ChainDB.Impl` | volatile→immutable promotion, rollback recovery |
| `node/src/sync/blockfetch_dispatch.rs` | `Ouroboros.Network.BlockFetch.Decision` | per-peer FetchWorkerPool |
| `node/src/sync/chain_recovery.rs` | `Ouroboros.Consensus.Storage.ChainDB.Init` | seed `ChainState` from volatile, restore-and-replay |
| `node/src/sync/tx_spans.rs` | `Cardano.Ledger.Binary.Decoding` | `BlockTxRawSpans` extraction (R85, R251) |
| `node/src/sync/checkpoint.rs` | `Ouroboros.Consensus.Storage.LedgerDB.Snapshots` | persist+prune+rollback |
| `node/src/sync/overlay_schedule.rs` | `Cardano.Protocol.TPraos.Rules.Overlay` | TPraos active-overlay classification (R248, R253) |
| `node/src/sync/phase2_eval.rs` | `Cardano.Ledger.Alonzo.Rules.Bbody` | `phase2_evaluator_or_trust_block` (R249) |

## Phase E — governor.rs split [DONE in R270]

`crates/network/src/governor.rs` (134 KB, 3,488 production lines).
Upstream `Ouroboros.Network.PeerSelection.Governor.*`:

| Yggdrasil target | Upstream module |
|---|---|
| `crates/network/src/governor/types.rs` | `Governor.Types` |
| `crates/network/src/governor/known_peers.rs` | `Governor.KnownPeers` |
| `crates/network/src/governor/established_peers.rs` | `Governor.EstablishedPeers` |
| `crates/network/src/governor/active_peers.rs` | `Governor.ActivePeers` |
| `crates/network/src/governor/big_ledger_peers.rs` | `Governor.BigLedgerPeers` |
| `crates/network/src/governor/root_peers.rs` | `Governor.RootPeers` |
| `crates/network/src/governor/policy.rs` | `Governor.Policy` |
| `crates/network/src/governor/churn.rs` | `Governor.Monitor.Churn` |
| `crates/network/src/governor/decision.rs` | `Governor.Monitor` (decision dispatch) |

## Phase F — local_server.rs split [DONE in R270 / partially R273]

`node/src/local_server.rs` (158 KB, 3,672 production lines).
Upstream `Ouroboros.Network.Protocol.{LocalStateQuery, LocalTxSubmission, LocalTxMonitor}`:

| Yggdrasil target | Upstream module |
|---|---|
| `node/src/local/server.rs` | `Ouroboros.Network.Server.Local` (NtC accept loop) |
| `node/src/local/handshake.rs` | `Ouroboros.Network.Protocol.Handshake.Type` (NtC version negotiation) |
| `node/src/local/state_query.rs` | `Ouroboros.Network.Protocol.LocalStateQuery.Server` |
| `node/src/local/tx_submission.rs` | `Ouroboros.Network.Protocol.LocalTxSubmission.Server` |
| `node/src/local/tx_monitor.rs` | `Ouroboros.Network.Protocol.LocalTxMonitor.Server` |
| `node/src/local/query_dispatch.rs` | `Cardano.Node.Queries` (query routing across eras) |

The query-dispatcher (`BasicLocalQueryDispatcher`, ~25 query tags)
is the largest sub-module — likely 60-80 KB after the split,
matching upstream's per-tag dispatcher table size.

## Phase G — epoch_boundary.rs split [DONE in R272]

`crates/ledger/src/epoch_boundary.rs` (75 KB, 1,812 production lines).
Upstream rules from `cardano-ledger`:

| Yggdrasil target | Upstream module |
|---|---|
| `crates/ledger/src/epoch/boundary.rs` | `Cardano.Ledger.Shelley.Rules.NewEpoch` (TICK + NEWEPOCH) |
| `crates/ledger/src/epoch/snap.rs` | `Cardano.Ledger.Shelley.Rules.Snap` |
| `crates/ledger/src/epoch/rupd.rs` | `Cardano.Ledger.Shelley.Rules.Rupd` |
| `crates/ledger/src/epoch/mir.rs` | `Cardano.Ledger.Shelley.Rules.Mir` |
| `crates/ledger/src/epoch/poolreap.rs` | `Cardano.Ledger.Shelley.Rules.PoolReap` |
| `crates/ledger/src/epoch/ratify.rs` | `Cardano.Ledger.Conway.Rules.Ratify` |
| `crates/ledger/src/epoch/enact.rs` | `Cardano.Ledger.Conway.Rules.Enact` |
| `crates/ledger/src/epoch/conway_epoch.rs` | `Cardano.Ledger.Conway.Rules.Epoch` |

## Phase C — state.rs split (the big one) [DONE in R269 a–w + R276]

`crates/ledger/src/state.rs` (505 KB, 12,630 production lines).
This is the deepest split. Upstream organization is per-era +
per-state-component, mirrored here:

### Per state-component types (split from current monolith)

| Yggdrasil target | Upstream |
|---|---|
| `crates/ledger/src/state/types.rs` | `Cardano.Ledger.Shelley.LedgerState.Types` |
| `crates/ledger/src/state/snapshot.rs` | (LedgerStateSnapshot — Yggdrasil-specific read-side) |
| `crates/ledger/src/state/checkpoint.rs` | (LedgerStateCheckpoint — Yggdrasil-specific) |
| `crates/ledger/src/state/pool.rs` | `Cardano.Ledger.Shelley.PoolParams` + state |
| `crates/ledger/src/state/reward_accounts.rs` | `Cardano.Ledger.Shelley.RewardUpdate` + accounts |
| `crates/ledger/src/state/stake_credentials.rs` | `Cardano.Ledger.Shelley.LedgerState` (delegation state) |
| `crates/ledger/src/state/drep.rs` | `Cardano.Ledger.Conway.Governance.DRep` |
| `crates/ledger/src/state/committee.rs` | `Cardano.Ledger.Conway.Governance.CommitteeState` |
| `crates/ledger/src/state/governance.rs` | `Cardano.Ledger.Conway.Governance.Proposals` + `EnactState` |

### Per-era apply paths (split from current monolith)

| Yggdrasil target | Upstream |
|---|---|
| `crates/ledger/src/rules/byron/apply.rs` | `Cardano.Chain.Block.Validation` |
| `crates/ledger/src/rules/shelley/{bbody,ledger,utxo,utxow,pool,deleg,delegs,ppup}.rs` | `Cardano.Ledger.Shelley.Rules.*` |
| `crates/ledger/src/rules/allegra/{utxo,utxow}.rs` | `Cardano.Ledger.Allegra.Rules.*` |
| `crates/ledger/src/rules/mary/{utxo,utxow}.rs` | `Cardano.Ledger.Mary.Rules.*` |
| `crates/ledger/src/rules/alonzo/{ledger,utxo,utxow,utxos,bbody}.rs` | `Cardano.Ledger.Alonzo.Rules.*` |
| `crates/ledger/src/rules/babbage/{ledger,utxo,utxow,utxos}.rs` | `Cardano.Ledger.Babbage.Rules.*` |
| `crates/ledger/src/rules/conway/{ledger,utxo,utxow,utxos,gov,certs,deleg,delegs,govcert,enact,ratify,epoch}.rs` | `Cardano.Ledger.Conway.Rules.*` |

This single file dispatches roughly **40 distinct upstream rule
modules**. Splitting it is genuinely 1-2 weeks of focused R-round
work — each rule needs verified-against-upstream-Haskell module
boundaries, careful `pub(crate)` visibility, and incremental test
partitioning so green gates stay green between every move.

## Verification protocol per phase

Each phase MUST:

1. Land as its own R-round commit (R256 = Phase A, R257 = Phase B,
   ..., R262 = Phase G), with explicit `node/src/upstream_pins.rs`
   audit-cadence rationale.
2. Be done one upstream module at a time — don't move 3 modules in
   one diff.
3. Run all four gates between every module move:
   - `cargo fmt --all -- --check`
   - `cargo check-all`
   - `cargo test-all`
   - `cargo lint`
4. Cross-check the new file's module path against upstream Haskell
   module name for byte-for-byte naming parity.
5. Update the relevant `AGENTS.md` in the affected directory after
   the move.

## What's already done (R256 baseline)

- **Phase A** ✅ — `crates/mempool/` → `crates/consensus/src/mempool/{queue,tx_state}.rs` per upstream `Ouroboros.Consensus.Mempool.*`. 287 mempool tests still pass.
- **Phase B** ✅ — `crates/cddl-codegen/` → `tools/cddl-codegen/`. Build-time tooling segregated from runtime crates. (Subsequently removed entirely in 2026-05-06: hand-coded per-era CBOR codecs in `crates/ledger/src/eras/*/cbor.rs` decisively replaced codegen because real upstream parity needs Byron / array-vs-map / optional-field semantics that CDDL underspecifies.)
- **Phase H** ✅ — test-module extraction across 15 monolith files. Production code per file reduced 20-73 %. All 4,903 tests still pass, 0 failures. This is the structural prep that makes Phases C-G tractable: per-rule splits operate on production code only, with the test code already isolated in sibling `<file>/tests.rs`.

## What's already done (R269 — R281 closure)

All R256 Phase C–G refactors plus the strict-mirror naming-parity
sweep landed across the R269–R281 R-arc:

| Phase | Closing round(s) | Notes |
|---|---|---|
| **D-runtime (`node/src/runtime.rs`)** | R271 a–s + R279 sweep | runtime.rs from 7,269 → ~140 lines (a thin re-export shell over 18 sub-modules under `runtime/`). All 18 sub-modules carry `## Naming parity` docstrings (R279). |
| **D-sync (`node/src/sync.rs`)** | R281 (residuals + parity block) | sync.rs annotated as a synthesis covering the verified-sync service body. The module-by-module split into `runtime/*` already absorbed the bulk of the runtime concern. |
| **F (`node/src/local_server.rs`)** | R270 a–e (sub-modules under `local_server/`) + R281 | accept loop + sessions registry split into sub-modules; parent file carries the LSQ dispatcher. |
| **E (`crates/network/src/governor.rs`)** | R270 a–e | 134 KB / 3,488 lines → 76 lines (a thin re-export shell over 5 sub-modules: `types`, `state`, `churn`, `peer_metric`, `counters`). All sub-modules annotated with strict-mirror docstrings (R280). |
| **G (`crates/ledger/src/epoch_boundary.rs`)** | R272 | epoch_boundary.rs split into per-rule sub-files; R272 also covers Pre-Conway era rules. |
| **C (`crates/ledger/src/state.rs`)** | R269 a–w + R276 sweep | state.rs from 12,704 → ~6,147 lines (24 sibling sub-modules under `state/` + `state/eras/`). All 24 state sub-modules carry `## Naming parity` docstrings (R276). |
| **R273 — subsystem submodule splits** | R273 a–i + R273-rename | praos/, opcert/, plutus/types/, plutus/cost_model/, plutus/flat/ each split + renamed strict-mirror in R273-rename + R281. |
| **R274–R281 — strict-mirror naming-parity sweep** | R274..R281 | Every production `.rs` file across the workspace either mirrors a single upstream `.hs` file by snake_case basename (52 files) or carries a `## Naming parity` docstring stanza explicitly declaring its synthesis story (157 files). The CI drift-guard (`scripts/check-strict-mirror.py`) enforces this going forward. |

Per-file verdicts are in [`docs/strict-mirror-audit.tsv`](strict-mirror-audit.tsv).
Per-round operational records are in [`docs/operational-runs/`](operational-runs/).

R262 (the C-state-split) was the deepest split as predicted; it landed
as the R269 a–w arc spread across 23 individual rounds + the R276
naming-parity closure. R261 (D-sync) folded into the broader
runtime.rs work via R271-arc and R281's parity-block annotation.
