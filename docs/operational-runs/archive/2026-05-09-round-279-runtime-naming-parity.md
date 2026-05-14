# Round 279 — `node/src/runtime/` parity sweep

**Date:** 2026-05-09
**Phase:** B (targeted renames + docstrings)
**Predecessor:** R278 (`docs/operational-runs/2026-05-09-round-278-mempool-naming-parity.md`)
**Plan:** `~/.claude/plans/playful-tickling-plum.md`

## Scope

Add `## Naming parity` docstring stanzas to all 18 runtime/ files. Most
are `(c-needed)` (no upstream `.hs` mirror); a few were auto-graded
`(a) DIRECT_MIRROR` against false-positive basename matches (config →
`Cardano/Tracing/Config.hs`) and are corrected to `(c) synthesis` with
explicit annotation.

The `node/src/runtime/` tree is Yggdrasil's async-task orchestration
layer — Haskell wires its node-runtime threads inline rather than as
per-file modules, so most runtime files have no strict 1:1 upstream
counterpart by construction.

## Files affected

### New `## Naming parity` blocks (18 files, all `(c) synthesis`)

| File | Mirrors (upstream concept the loop body covers) |
|---|---|
| `runtime/block_producer_config.rs` | `Ouroboros.Consensus.Node.Forking.forkBlockForging` config + `SharedKernelState` |
| `runtime/block_producer_loop.rs` | `Ouroboros.Consensus.Node.NodeKernel.forkBlockForging` slot loop |
| `runtime/bootstrap.rs` | `Ouroboros.Network.NodeToNode.connectTo` + `Ouroboros.Network.Mux` + `Subscription.Worker` |
| `runtime/cm_actions.rs` | `Ouroboros.Network.ConnectionManager.Core` action dispatch glue |
| `runtime/forge.rs` | `Cardano.Node.Forge.{Configuration,Run}` + `Ouroboros.Consensus.Block.Forging` |
| `runtime/governor_config.rs` | `Cardano.Node.Run.checkPointsAndApplyChunkOptions` config-overlay |
| `runtime/governor_loop.rs` | `Ouroboros.Network.PeerSelection.Governor.peerSelectionGovernor` slot-tick body + `Cardano.Node.Diffusion` outer loop |
| `runtime/keep_alive.rs` | partial mirror of `Ouroboros.Network.Protocol.KeepAlive.Client` (Yggdrasil scheduler wraps the protocol's client driver) |
| `runtime/ledger_judgement.rs` | `Cardano.Node.Diffusion.Configuration.mkLedgerStateJudgement` parameters |
| `runtime/ledger_peer_source.rs` | `Cardano.Node.Diffusion.Configuration` glue around `LedgerPeers` and `PeerSnapshot` |
| `runtime/mempool_helpers.rs` | `Ouroboros.Network.TxSubmission.Inbound.Server` + `Ouroboros.Consensus.Mempool.Update` glue |
| `runtime/peer_management.rs` | runtime glue around `Ouroboros.Network.PeerSelection.PeerStateActions`, `RootPeers`, `BlockFetch.ClientRegistry` |
| `runtime/peer_session.rs` | NodeConfig + per-peer mini-protocol bundle + reconnect-request shape |
| `runtime/reconnecting.rs` | `Ouroboros.Consensus.Node.Run.runWith` reconnect-loop state |
| `runtime/reconnecting_sync.rs` | high-level reconnect-loop entry points (4 combos: run/resume × volatile-store/chaindb × default/with-tracer) |
| `runtime/sync_session.rs` | `Ouroboros.Consensus.Node.Run.runWith` shutdown traces + ChainSync intersection sync glue |
| `runtime/tracing.rs` | data side of `Cardano.Node.Tracing.Tracers.NodeToNode.*` (sync session + batch progress + reconnect events) |
| `runtime/tx_submission_service.rs` | `Ouroboros.Network.TxSubmission.Inbound.Server` request-handling loop |

### Re-graded by docstring annotations (no logic changes)

Pre-R279, four files were auto-graded `(a) DIRECT_MIRROR` against
basename-match false-positives:

- `runtime/block_producer_config.rs` matched `Cardano/Tracing/Config.hs` (wrong domain).
- `runtime/governor_config.rs` matched `Cardano/Tracing/Config.hs` (wrong domain).
- `runtime/keep_alive.rs` matched `Ouroboros/Network/KeepAlive.hs` (correct upstream module, but Yggdrasil's `KeepAliveScheduler` is a runtime-side wrapper, not a strict file mirror).
- `runtime/tracing.rs` matched `Cardano/Node/Tracing.hs` (top-level upstream tracing hub, not the data-side trace builders Yggdrasil isolates).

The new `## Naming parity` blocks declare these as syntheses, and the
audit grader (R277-strengthened) re-grades them to `(c)` since the
docstring's `**Strict mirror:** none.` declaration overrides the
basename-collision auto-grade.

## Verdict bucket counts

| Bucket | Pre-R279 | Post-R279 |
|---|---|---|
| `(a) DIRECT_MIRROR` | 57 | 53 (-4 false-positives moved to (c)) |
| `(c) NO_MIRROR_NEEDS_DOCSTRING` | 36 | 54 (+18 runtime files resolved) |
| `(c-needed)` | 36 | 25 (-11) |
| `(NEEDS-REVIEW)` | 80 | 77 (-3) |
| **TOTAL** | 209 | 209 |

runtime/ tree (18 files: 14 + 4 falsified-(a)) is fully resolved with
0 (a) and 18 (c) verdicts.

## Verification gates

```text
cargo fmt --all -- --check          clean
cargo check-all                     clean (Finished `dev` profile in 3.03s)
cargo lint                          clean (Finished `dev` profile in 6.62s)
cargo test-all                      4855 passed; 0 failed (baseline preserved)
python3 scripts/check-strict-mirror.py
                                    strict-mirror: 0 violations (clean)
```

## Diff stat

```text
node/src/runtime/block_producer_config.rs  +12 lines
node/src/runtime/block_producer_loop.rs    +9 lines
node/src/runtime/bootstrap.rs              +10 lines
node/src/runtime/cm_actions.rs             +9 lines
node/src/runtime/forge.rs                  +9 lines
node/src/runtime/governor_config.rs        +10 lines
node/src/runtime/governor_loop.rs          +9 lines
node/src/runtime/keep_alive.rs             +12 lines
node/src/runtime/ledger_judgement.rs       +9 lines
node/src/runtime/ledger_peer_source.rs     +8 lines
node/src/runtime/mempool_helpers.rs        +9 lines
node/src/runtime/peer_management.rs        +11 lines
node/src/runtime/peer_session.rs           +9 lines
node/src/runtime/reconnecting.rs           +8 lines
node/src/runtime/reconnecting_sync.rs      +11 lines
node/src/runtime/sync_session.rs           +10 lines
node/src/runtime/tracing.rs                +10 lines
node/src/runtime/tx_submission_service.rs  +10 lines
docs/strict-mirror-audit.tsv               rebuilt
docs/operational-runs/2026-05-09-round-279-... (new)
```

## Stop point — Phase B progress

| Round | Cluster | Status |
|---|---|---|
| R276 | `crates/ledger/src/state/` (24 files) | ✅ closed |
| R277 | `consensus/{nonce,opcert,diffusion_pipelining}/` (9 files) | ✅ closed |
| R278 | `consensus/mempool/` (7 files) | ✅ closed |
| R279 | `node/src/runtime/` (18 files) | ✅ closed |
| R280 | `crates/network/src/governor/` regrade | next |
| R281 | sweeper (incl. `opcert.rs` -> `ocert.rs` rename) | pending |

Phase B is 4/6 closed. Remaining work after R280: the `(c-needed)` 25
files + `(NEEDS-REVIEW)` 77 files split across network, plutus, storage,
crypto, node top-level, commands, local_server.

## References

- Plan: `~/.claude/plans/playful-tickling-plum.md`
- Predecessor: R278 (`docs/operational-runs/2026-05-09-round-278-mempool-naming-parity.md`)
- Authoring-time skill: `.claude/skills/round-extraction/SKILL.md`
- Allowlist source-of-truth: `docs/strict-mirror-audit.tsv`
