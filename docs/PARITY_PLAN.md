---
title: Parity Plan
layout: default
parent: Reference
nav_order: 2
---

# Full Parity Plan: Rust Cardano Node vs. Official Haskell Implementation

**Prepared**: March 26, 2026  
**Status**: Comprehensive planning document for achieving feature-parity with IntersectMBO Haskell Cardano node  
**Scope**: All subsystems from crypto through orchestration, covering all 7 eras (Byron → Conway)

---

## Table of Contents
1. [Executive Summary](#executive-summary)
2. [Parity Matrix: Current vs. Upstream](#parity-matrix-current-vs-upstream)
3. [Subsystem-by-Subsystem Analysis](#subsystem-by-subsystem-analysis)
4. [Phased Implementation Roadmap](#phased-implementation-roadmap)
5. [Cross-Subsystem Integration Points](#cross-subsystem-integration-points)
6. [Risk Assessment & Mitigation](#risk-assessment--mitigation)
7. [Success Criteria](#success-criteria)

---

## Executive Summary

The Rust Cardano node (Yggdrasil) has achieved:
- ✅ **Complete era-type coverage** (Byron → Conway)
- ✅ **Core network protocols** (5 mini-protocols + mux + handshake)
- ✅ **Fundamental consensus structures** (Praos validation, nonce evolution)
- ✅ **Ledger state transitions** (multi-era UTxO, certificates, governance)
- ✅ **CLI & configuration** (JSON + YAML config, genesis loading, query/submit)
- ✅ **Local query & submission APIs** (LocalStateQuery, LocalTxSubmission, LocalTxMonitor)
- ✅ **File-backed storage** (Immutable/Volatile with rollback + crash recovery)
- ⚠️ **Partial Plutus** (CEK machine framework, V1/V2/V3 support wired)
- ✅ **Peer management** (governor with dual churn, big-ledger, backoff, inbound)
- ✅ **Monitoring** (35+ metrics, Prometheus/JSON endpoints, coloured stdout, detail levels, upstream backend recognition)
- ✅ **Block production** (credential loading, VRF leader election, KES header signing, runtime slot loop, local block minting, post-forge adoption check)

**To achieve full parity**, the remaining work focuses on:
1. **Plutus CEK drift monitoring** (keep Conway/Plomin cost-model key mapping in sync with upstream changes)
2. **Integration testing** (mainnet-like end-to-end scenarios)

**Recently completed parity items**:
- ✅ **Live tip-slot plumbing into protocol-state + ledger-peer-snapshot (Round 191)** — replaces static `Origin` (`[0]` CBOR singleton) for `praosStateLastSlot` and `ledger-peer-snapshot`'s `WithOrigin SlotNo` with live `snapshot.tip().slot()`.  `cardano-cli conway query protocol-state` now reports `lastSlot: <chain tip slot>`; `query ledger-peer-snapshot` reports the live tip slot in `slotNo`.  First step of the post-audit data-plumbing arc — remaining placeholder fields (PraosState OCert counters + 6 nonces) require threading `NonceEvolutionState` and `OcertCounters` from the consensus runtime into `LedgerStateSnapshot`.
- ✅ **Comprehensive cardano-cli parity audit + tag 12/13 dispatchers (Round 190)** — systematic audit of every `cardano-cli conway query` subcommand surfaced two operational gaps: `protocol-state` (tag 13 `DebugChainDepState`, returned `null` which cardano-cli's PraosState decoder rejected) and `ledger-state` (tag 12 `DebugNewEpochState`, returned `null` from `Unknown` fallthrough — acceptable for cli but not explicitly recognised).  Added both dispatchers; `protocol-state` emits `Versioned 0` wrapped 8-element `PraosState` placeholder.  Discovered: PraosState wire is `[version, [8-record]]` not bare 8-record (initial bare emission failed with `Size mismatch when decoding Versioned. Expected 2, but found 8.`).  Confirmed 28 cardano-cli subcommands working end-to-end; remaining 3 (`kes-period-info`, `leadership-schedule`, `stake-address-info`) initially "failing" were all client-side CLI arg validation, not yggdrasil bugs.
- ✅ **Conway `ledger-peer-snapshot` end-to-end (Round 189) — closes the Conway-era LSQ wire-protocol gap entirely** — new `EraSpecificQuery::GetLedgerPeerSnapshot { peer_kind: Option<u8> }` variant covering v15+ form `[34, peer_kind]` and legacy singleton `[34]`.  Dispatcher emits V2 wire shape `[1, [[0], 0x9f 0xff]]` (discriminator 1 + Origin marker + indefinite-length empty pool list) — V23 forms (discriminators 2/3) were rejected by cardano-cli 10.16's decoder, and the pool list specifically requires indefinite-length encoding.  `cardano-cli conway query ledger-peer-snapshot` returns `{"bigLedgerPools": [], "slotNo": "origin", "version": 2}`.  Every documented Conway-era LSQ tag now has a wire-correct dispatcher.
- ✅ **Conway `gov-state` body shape end-to-end (Round 188) — closes last user-facing Conway gap** — new `encode_conway_gov_state_for_lsq` helper emits the upstream 7-element `ConwayGovState` (Proposals 2-tuple, SNothing Committee, real Constitution, Conway 31-element PParams×2, `FuturePParams` internal Sum ADT `[0]`, and `DRepPulsingState = DRComplete (PulsingSnapshot, RatifyState)` composing R187's RatifyState helper).  `cardano-cli conway query gov-state` decodes end-to-end with real Conway constitution + 31-elem PParams rendered.  Every `cardano-cli conway query` subcommand other than the operational `ledger-peer-snapshot` now decodes against yggdrasil.
- ✅ **Conway `ratify-state` body shape end-to-end (Round 187)** — closes the substantial 4-field-record body-shape gap.  New `encode_enact_state_for_lsq` helper emits the upstream 7-element `EnactState` (committee/constitution/cur-prev PParams/treasury/withdrawals/GovRelation StrictMaybe); `encode_ratify_state_for_lsq` wraps it in the 4-element `[EnactState, Seq, Set, Bool]` record.  `cardano-cli conway query ratify-state` decodes end-to-end with real Conway constitution + 31-element PParams + treasury values rendered.  EnactState encoder is the load-bearing helper for the upcoming gov-state round (used inside its DRepPulsingState field via PulsingSnapshot+RatifyState).
- ✅ **Conway tail-end LSQ dispatchers — `GetStakeDelegDeposits` (tag 22) + `GetPoolDistr2` (tag 36) (Round 186)** — two more `EraSpecificQuery` variants closing the simpler remaining Conway dispatcher gaps.  No direct cardano-cli subcommands; emits empty `0xa0` map and `[map, NonZero=1]` PoolDistr placeholders for protocol completeness.  Open Conway-era body-shape gaps now reduced to two: `gov-state` (tag 24) and `ratify-state` (tag 32).
- ✅ **Conway governance LSQ — `proposals` + `stake-pool-default-vote` end-to-end (Round 185)** — two new `EraSpecificQuery` variants (`GetProposals` tag 31, `QueryStakePoolDefaultVote` tag 35) so `cardano-cli conway query proposals --all-proposals` returns `[]` and `query stake-pool-default-vote --spo-key-hash <hash>` returns `"DefaultNo"` end-to-end.  Empty Seq `0x80` and DefaultNo (single CBOR uint 0) placeholders until governance-action and per-pool default-vote tracking land.
- ✅ **Conway governance LSQ — `drep-stake-distribution` + `spo-stake-distribution` end-to-end (Round 184)** — three new `EraSpecificQuery` variants (`GetDRepStakeDistr` tag 26, `GetFilteredVoteDelegatees` tag 28, `GetSPOStakeDistr` tag 30) so `cardano-cli conway query drep-stake-distribution --all-dreps` and `query spo-stake-distribution --all-spos` decode end-to-end.  Discovery: SPO query is a 3-call flow (tag 30 → GetCBOR(GetPoolState) → tag 28); the tag-28 dispatcher was the missing piece.  All three return empty CBOR maps (`0xa0`) until live stake plumbing lands; cardano-cli renders `{}` and `[]` respectively.
- ✅ **Conway governance LSQ — `future-pparams` end-to-end (Round 183)** — `cardano-cli conway query future-pparams` decodes end-to-end via new `EraSpecificQuery::GetFuturePParams` variant (singleton), `(1, 33)` decoder, and dispatcher arm emitting `Maybe (PParams era) = Nothing` (empty CBOR list `0x80`).  cardano-cli renders `Nothing` as `"No protocol parameter changes will be enacted at the next epoch boundary."`.  Discovery: the LSQ-facing result type is `Maybe (PParams era)` — distinct from the internal `FuturePParams` ADT in `Cardano.Ledger.Core.PParams`.
- ✅ **Conway governance LSQ — `committee-state` end-to-end (Round 182)** — `cardano-cli conway query committee-state` decodes end-to-end via new `EraSpecificQuery::GetCommitteeMembersState { cold_creds_cbor, hot_creds_cbor, statuses_cbor }` variant, `(4, 27)` decoder, and `encode_committee_members_state_for_lsq` emitting the upstream 3-element `CommitteeMembersState` record `[csCommittee_map, csThreshold (SNothing), csEpochNo]`.
- ✅ **DRepState LSQ Map shape (Round 181)** — `GetDRepState` now emits a CBOR map (`encCBOR @(Map a b)`) instead of the storage-format array-of-pairs that yggdrasil's `DrepState::encode_cbor` produces.
- ✅ **Conway governance LSQ queries — constitution, gov-state dispatcher, drep-state dispatcher, account-state (Round 180)** — four new `EraSpecificQuery` variants (`GetConstitution`, `GetGovState`, `GetDRepState`, `GetAccountState`) wired through `decode_query_if_current` and `dispatch_upstream_query` reusing existing snapshot encoders.  `cardano-cli conway query constitution` returns real Conway constitution data; `query treasury` (uses `GetAccountState`) returns 0; `query drep-state --all-dreps` returns `[]` after R181 shape fix.  `gov-state` dispatcher routes; full body shape pending.
- ✅ **Era blockage end-to-end fix (Round 179)** — three independent bugs unblocked: corrected the LSQ era-specific tag table to match upstream cardano-node 10.7.x (`GetStakePools` 13→16, `GetStakePoolParams` 14→17, `GetPoolState` 17→19, `GetStakeSnapshots` 18→20); added `GetStakeDistribution2` (tag 37) handling with `[map, NonZero Coin]` shape; added `GetCBOR` (tag 9) wrapper recursion via `dispatch_inner_era_query` helper.  All five era-gated queries now decode end-to-end against `cardano-cli 10.16` with `YGG_LSQ_ERA_FLOOR=6`.
- ✅ **`YGG_LSQ_ERA_FLOOR` env-var bypass (Round 178)** — operator opt-in floor on the LSQ-reported era so cardano-cli's client-side Babbage+ gate can be bypassed on partial-sync chains; with `YGG_LSQ_ERA_FLOOR=6` cardano-cli reports `era=Conway` and stops gating the era-locked queries.
- ✅ **`encode_filtered_delegations_and_rewards` correctness (Round 177)** — three independent bugs: non-deterministic `HashSet` iteration, O(N·M) inner search per credential (now `BTreeMap::get` O(log N)), reward-account lookup mis-matched on hash bytes alone (now `find_account_by_credential` full match).
- ✅ **Decoder strictness sweep (Rounds 174 + 176)** — five CBOR set-decoder helpers tightened to enforce CIP-21 tag 258 strictly; `Maybe Nothing` shortcut now requires bare `null` (`0xf6`) rather than any major-7 byte.
- ✅ **Mid-sync rollback epoch fixup (Round 167)** — `recover_ledger_state` post-recovery patches `current_epoch` to match the recovered tip's slot when crossing an epoch boundary, preventing PPUP validation errors on cross-epoch rollback.
- ✅ **Sync-speed unblock (Rounds 165 + 166)** — default `--batch-size 10 → 30 → 50` with R166 initial-sync rollback fast path skipping the heavy `recover_ledger_state_chaindb` replay when rollback target is `Origin` and base ledger state is empty.  Out-of-the-box preprod sync improves from ~5 to ~14 blocks/sec.
- ✅ **Observability metrics (Rounds 168, 169, 170, 175)** — bootstrap sync peer marked `PeerHot` in `PeerRegistry` (fixed `yggdrasil_active_peers` reporting 0 during active sync); new `yggdrasil_current_era` Prometheus gauge and per-era applied-block counters; cooling completion at all `mux.abort()` sites.
- ✅ **Cumulative cardano-cli operational parity arc — Rounds 144 → 164** — full operational verification of all 11 working `cardano-cli` operations against fresh preprod (Shelley era) and preview (Alonzo era) syncs.  See `docs/operational-runs/2026-04-28-round-{144..164}-*.md`.

For pre-R164 rounds, see `docs/PARITY_SUMMARY.md` audit-history table.

- ✅ **CM timeout prune-target parity + stale-terminated cleanup (Round 103)** — `ConnectionManagerState::timeout_tick()` now opportunistically removes stale `TerminatedState` entries before generating prune actions, and timeout-driven `maybe_prune()` now selects only `InboundIdleState` peers as prune candidates. This prevents terminated entries from consuming prune budget in over-limit inbound scenarios, ensuring prune actions always target real inbound-idle connections that can reduce inbound pressure while stale terminated map entries are collected locally.
- ✅ **Governor/inbound CM timeout action-scoping parity (Round 102)** — governor timeout maintenance now defers inbound-scoped CM actions (`PruneConnections`, `StartResponderTimeout`, and terminate actions for peers without outbound warm sessions) to the inbound accept loop, which owns inbound mux abort handles. The governor still applies outbound-relevant timeout actions directly. This avoids consuming inbound prune/terminate actions in the governor path where transport teardown cannot be executed, keeping CM timeout side effects aligned with the loop that can actually close inbound sessions.
- ✅ **Precise near-future boundary wait parity (Round 101)** — near-future sync waiting now targets the exact slot-start boundary derived from genesis timing (`system_start + slot * slot_length`) rather than sleeping coarse whole-slot multiples from `excess_slots`. This reduces avoidable oversleep and more closely matches upstream header-arrival timing behavior in `InFutureCheck`.
- ✅ **Inbound CM timeout cadence parity (Round 100)** — inbound runtime now advances `ConnectionManagerState::timeout_tick` on a dedicated 1-second timer in `run_inbound_accept_loop()` instead of coupling timeout progression to the 31.4s inactivity tick. This aligns timeout-driven responder/time-wait transitions with the configured CM deadlines (`PROTOCOL_IDLE_TIMEOUT = 5s`, `TIME_WAIT_TIMEOUT = 60s`) and avoids delayed timeout handling in inbound-only runtime modes.
- ✅ **Inbound CM timeout-tick progression parity (Round 99)** — `run_inbound_accept_loop()` now advances `ConnectionManagerState::timeout_tick(Instant::now())` on each inactivity tick and applies emitted `CmAction`s through the inbound CM action bridge. This ensures responder/time-wait timeout transitions continue to progress even when inbound service is running without the governor loop, matching upstream intent that CM timeout maintenance is continuously advanced while the server is active.
- ✅ **Near-future header wait parity (Round 98)** — verified sync now waits for near-future blocks (within clock-skew tolerance) before acceptance in `sync_batch_verified_with_tentative`, using slot-based delay derived from genesis slot length (`excess_slots * slot_length_secs`). This replaces immediate processing of near-future blocks and aligns runtime behavior with upstream `InFutureCheck.handleHeaderArrival` timing semantics while preserving far-future rejection (`SyncError::BlockFromFuture`).
- ✅ **Dynamic wall-slot future-block check parity (Round 97)** — `FutureBlockCheckConfig` now stores genesis timing (`system_start_unix_secs`, `slot_length_secs`) and computes `current_wall_slot` dynamically at validation time in `sync.rs`, instead of freezing a startup wall-slot snapshot. This aligns `InFutureCheck` behavior with upstream header-arrival checks that evaluate against current wall-clock progression, preventing false `BlockFromFuture` rejections during long-running sync sessions.
- ✅ **Inbound CM terminate/prune transport-side effects parity (Round 96)** — `server.rs` now executes inbound `CmAction::TerminateConnection` and `CmAction::PruneConnections` as real transport teardown by maintaining an `InboundSessionAborts` registry of per-peer mux abort handles and invoking it from the inbound CM/IG bridge (`execute_cm_actions`). This removes the previous no-op behavior where those actions were only state-tracked comments and ensures inbound session/mux tasks are actually aborted when CM transitions request termination or pruning (aligned with upstream `ConnectionManager`/`Server2` terminate intent). Added regression coverage `inbound_session_aborts_aborts_registered_mux_and_is_idempotent`.
- ✅ **Connection-manager StartConnect action handling parity (Round 95)** — `run_governor_loop()` now executes `CmAction::StartConnect` directly through the shared CM-action bridge (`apply_cm_actions`) instead of deferring it to ad-hoc caller-side handling. The bridge now performs the full upstream-style outbound transition sequence for StartConnect actions (`bootstrap` dial via `promote_to_warm` → `outbound_handshake_done` with negotiated `DataFlow` → registry status refresh, with `outbound_connect_failed` on failure). This removes the previous split path where StartConnect handling was duplicated in one branch and logged as deferred in others, and keeps CM action execution behavior consistent across establish/timeout/release flows.
- ✅ **Governor mode + handshake config parity (Round 94)** — `NodeConfigFile` now carries explicit `peer_sharing` (handshake wire value, default `1`) and `consensus_mode` (`PraosMode`/`GenesisMode`, default `PraosMode`) controls. Runtime wiring is now end-to-end: `main.rs` threads both into `RuntimeGovernorConfig` and `NodeConfig`; `run_governor_loop()` uses configured peer-sharing willingness in `compute_association_mode()` and configured consensus mode in `pick_churn_regime()`; and `bootstrap_with_attempt_state()` now advertises the configured `peer_sharing` value instead of the previous hardcoded `1`. This removes the remaining hardcoded network-mode assumptions in governor tick and handshake proposal paths while keeping default behavior unchanged.
- ✅ **NtC LocalStateQuery acquire-at-point chain-membership parity (Round 93)** — `AcquireTarget::Point` in `local_server.rs` now decodes the requested `Point` and performs deterministic chain replay to that point (immutable suffix then volatile suffix), returning `AcquireFailure::PointNotOnChain` when the point is absent. This replaces the previous behavior that only compared against tip and incorrectly accepted arbitrary points on empty chains. Added integration coverage (`ntc_local_state_query_rejects_unknown_acquire_point`) to assert unknown points are rejected.
- ✅ **Forged-invalid-block trace severity parity (Round 92)** — self-validation failure of locally forged blocks now traces at `Critical` severity (upstream `TraceForgedInvalidBlock`) instead of `Error`, with the message renamed to `forged invalid block (self-validation failed)` to match upstream semantics. This is more severe than a peer-attributable invalid block because it indicates a local mempool/validation inconsistency that produced a malformed block requiring operator investigation. Reference: cardano-node `Ouroboros.Consensus.Node.Tracers` `TraceForgedInvalidBlock` and `NodeKernel.forkBlockForging` post-forge `getIsInvalidBlock` check.
- ✅ **Forge-loop slot-immutable trace event (Round 91)** — block producer loop now emits the upstream `TraceSlotIsImmutable` event (Warning, slot + tipSlot) instead of silently `continue`-ing when `make_block_context()` returns `None` because the current slot is at or behind the chain tip. This restores parity with upstream `mkCurrentBlockContext` returning `Left ImmutableSlot` so operators can observe forge-loop slots that were rejected as immutable, rather than seeing them disappear from the trace stream.
- ✅ **Forge-loop leadership-check trace events (Round 90)** — block producer loop now emits the upstream `forkBlockForging` per-slot trace events that were previously missing: `TraceStartLeadershipCheck` (Debug, slot + blockNo) at the start of every slot's leadership check, `TraceNodeNotLeader` (Debug, slot) when the VRF check declines election (previously a silent `continue`), and `TraceNodeIsLeader` (Notice, slot + blockNo) once leader election succeeds and before block construction begins. This restores parity with the upstream `Ouroboros.Consensus.Node.Tracers` event sequence operators rely on for per-slot forge-loop liveness monitoring and for reconciling elected slots against `TraceForgedBlock` / `TraceAdoptedBlock`.
- ✅ **Observability debug-endpoint parity aliases (Round 89)** — metrics HTTP serving now accepts upstream-style debug paths in addition to existing endpoints: `GET /debug` and `GET /debug/metrics` (JSON metrics), `GET /debug/metrics/prometheus` (Prometheus text), and `GET /debug/health` (health JSON). This keeps local observability compatible with EKG/debug-oriented operator tooling while preserving existing `GET /metrics`, `GET /metrics/json`, and `GET /health` behavior. Added unit tests for all debug aliases.
- ✅ **Tracer max-frequency clone-sharing parity (Round 88)** — `NodeTracer::clone()` now shares the same namespace emit-rate state (`last_emit_ms`) across clones (`Arc::clone`) instead of deep-copying the rate-limiter map. This preserves global `maxFrequency` throttling across concurrently spawned runtime tasks that hold cloned tracers, aligning with upstream trace-dispatcher behavior where scribe rate limits are process-wide rather than per-task clone. Added regression test `clone_shares_rate_limiter_state`.
- ✅ **Forwarder backend clone parity (Round 87)** — `NodeTracer::clone()` now preserves the `Forwarder` socket transport (`Arc<TraceForwarder>`) instead of dropping it. This keeps forwarder emission active across the cloned tracers passed into spawned runtime tasks (governor, inbound, block producer, NtC), matching upstream operational behavior where backend routing remains intact across concurrent tracing callsites. Added regression test `clone_preserves_forwarder_transport_when_enabled`.
- ✅ **Conway Plutus cost-model mapping completeness guard (Round 86)** — `build_plutus_cost_model()` now enforces full cardinality mapping between accepted Conway `plutusV3CostModel` arrays and the local named-parameter table. After building the named map, it fails fast with `IncompleteConwayV3Mapping { expected, mapped }` if any accepted array entry would be dropped (for example, if local parameter-name coverage drifts below the 302-entry upstream shape), preventing silent truncation.
- ✅ **Conway Plutus cost-model array length guard (Round 85)** — `build_plutus_cost_model()` now accepts only upstream-known `plutusV3CostModel` array lengths (251 or 302 entries) when falling back from missing named Alonzo maps. Unsupported lengths now fail fast via `UnsupportedConwayV3ArrayLength` instead of silently zipping/truncating into a partial named map, preventing hidden cost-model drift and runtime `MissingBuiltinCost` surprises.
- ✅ **BBODY max block body size full-serialization parity (Round 84)** — `apply_block_validated()` now enforces `max_block_body_size` using full serialized transaction bytes (`Tx::serialized_size()`: body + witnesses + `is_valid` + aux/null) instead of body-only bytes. This aligns local BBODY accounting with upstream `validateMaxBlockBodySize` semantics and prevents undercounting when witness/aux payloads dominate. Added regression coverage in `block_body_size.rs` for both single-tx and multi-tx aggregation.
- ✅ **Storage WAL for multi-step volatile mutations (Round 83)** — `FileVolatile` now persists a delete-plan WAL (`wal.pending.json`) before multi-step delete operations (`prune_up_to`, `rollback_to`, `garbage_collect`) and replays/removes that plan on open. This closes the storage parity gap for write-ahead recovery on delete-heavy mutation paths and aligns with upstream VolatileDB-style crash-recovery intent.
- ✅ **Post-forge adoption check** — after forging, compare chain tip with forged block point and emit `TraceAdoptedBlock`/`TraceDidntAdoptBlock` (upstream `NodeKernel.forkBlockForging`)
- ✅ **Forged block self-validation** — block producer now self-validates each forged block before persistence (protocol version, body hash, body size, header identity), preventing local malformed-forge persistence drift.
- ✅ **Forged issuer-key parity** — block producer credentials now require an explicit issuer cold verification key, validate OpCert signature against that key at startup, and forge headers with that issuer key (instead of using the KES hot key as proxy).
- ✅ **Invalid block punishment** — peer-attributable validation errors (Consensus, BlockBodyHashMismatch, LedgerDecode, BlockFromFuture) now trigger reconnection to a different peer instead of killing the sync service; `ChainDB.AddBlockEvent.InvalidBlock` trace emitted (upstream `InvalidBlockPunishment`)
- ✅ **Blocks-from-the-future check** — `ClockSkew` + `FutureSlotJudgement` in consensus crate; blocks exceeding clock-skew tolerance rejected as `SyncError::BlockFromFuture` (upstream `InFutureCheck`)
- ✅ **Diffusion pipelining tentative-chain wiring (DPvDV)** — node runtime now threads shared `TentativeState` through reconnecting verified sync and inbound ChainSync serving; verified batch sync sets tentative headers on roll-forward announcements and clears adopted/trap outcomes; inbound ChainSync now serves tentative tips and rolls followers back when a served tentative header is trapped (upstream `SupportsDiffusionPipelining` / `cdbTentativeHeader` behavior)
- ✅ **Block propagation wiring** — locally forged blocks now persist multi-era raw CBOR for downstream relay, and reconnecting sync notifies chain-tip followers after each applied batch so inbound ChainSync responders can wake without polling (upstream ChainDB follower wakeup intent)
- ✅ **MaxMajorProtVer population** — `NodeConfigFile.max_major_protocol_version` (default 10, Conway era) now wires through to `VerificationConfig` in both verified and unverified sync paths; blocks with a protocol version major > configured max are rejected at verification time (upstream `Ouroboros.Consensus.Protocol.Abstract.MaxMajorProtVer`)
- ✅ **Future-block check runtime wiring** — `ShelleyGenesis.system_start` parsed at startup; `current_wall_slot()` computed from `(now - system_start) / slot_length`; `FutureBlockCheckConfig` wired into `VerificationConfig` with `ClockSkew::default_for_slot_length`; batch sync rejects far-future blocks (upstream `InFutureCheck.realHeaderInFutureCheck`)
- ✅ **OpCert counter runtime wiring** — `OcertCounters::new()` initialized at startup in both verified and unverified `VerificationConfig` paths; batch sync functions thread `&mut Option<OcertCounters>` across batches and reconnects; permissive mode accepts first-seen pools without stake-distribution lookup; per-pool monotonic sequence validation (same or +1) is enforced for all tracked pools (upstream `PraosState.csCounters` / `currentIssueNo`)
- ✅ **TriesToForgeADA** — `validate_no_ada_in_mint` rejects any transaction whose `mint` field contains the ADA policy ID (`[0u8; 28]`), implementing the formal spec predicate `adaPolicy ∉ supp mint tx` (upstream `Cardano.Ledger.Mary.Rules.Utxo`, Mary through Conway). In Haskell this is guaranteed by construction (the `MultiAsset` type cannot represent ADA); in Rust the `BTreeMap<[u8; 28], …>` representation requires a runtime check. Wired into all 4 mint-capable era UTxO apply functions covering both block-apply and submitted-tx paths; 6 unit tests.
- ✅ **Plutus cost-shape parity** — All 18 `CostExpr` variants now match upstream builtin argument shapes (Constant, LinearInX/Y/Z, AddedSizes, SubtractedSizes, MultipliedSizes, MinSize, MaxSize, LinearOnDiagonal, ConstAboveDiagonal, ConstBelowDiagonal, QuadraticInY/Z, LiteralInYOrLinearInZ, LinearInYAndZ, ConstOffDiag). ~14 builtin shape mappings fixed in `build_per_builtin_costs()`.
- ✅ **validateScriptsWellFormed** — `PlutusEvaluator::is_script_well_formed()` trait method (default `true`); `validate_script_witnesses_well_formed()` / `validate_reference_scripts_well_formed()` in `witnesses.rs` detect malformed Plutus scripts at admission time (upstream `Cardano.Ledger.Alonzo.Rules.Utxos`). Wired into all Alonzo+-era `apply_block_validated` paths.
- ✅ **OutsideForecast** — `validate_outside_forecast()` in `utxo.rs` implements upstream infrastructure from `Cardano.Ledger.Shelley.Rules.Utxo`; correctly modeled as no-op since `unsafeLinearExtendEpochInfo` makes the check always pass.
- ✅ **BlocksMade tracking** — `LedgerState.blocks_made` (element 23, upstream `NewEpochState.nesBcur`) tracks per-pool block production; `record_block_producer()` called automatically from `apply_block_validated()` for non-Byron blocks; `take_blocks_made()` clears at epoch boundary. `derive_pool_performance()` computes `UnitInterval` ratios from internal block counts + stake distribution; `apply_epoch_boundary()` uses internally derived performance when caller passes empty performance map.
- ✅ **Dynamic block-producer nonce/sigma** — Block producer loop now reads live epoch nonce and pool relative stake from `SharedBlockProducerState` (falling back to static config). Sync pipeline pushes updated values via `update_bp_state_nonce()` after every batch and `update_bp_state_sigma()` after epoch boundary stake snapshot rotation. `main.rs` computes `bp_pool_key_hash` (Blake2b-224 of cold issuer key) and threads both shared state and hash into the reconnecting sync request. Fixes leader election stalling after the first epoch transition.
- ✅ **Plutus cost-model strict mode (April 2026)** — `CostModel` now carries `strict_builtin_costs: bool`. `CostModel::from_alonzo_genesis_params()` (production-derived models from Alonzo named maps and Conway V3 array) sets it to `true`. `CostModel::builtin_cost()` returns `Result<ExBudget, MachineError>` and yields the new structural `MachineError::MissingBuiltinCost(name)` for any builtin invoked at runtime that lacks a per-builtin entry. `CekMachine` propagates the error so incomplete cost models surface as a structural failure (not collapsed to opaque `EvaluationFailure`) instead of being silently masked by the flat `builtin_cpu`/`builtin_mem` fallback. The non-strict default is retained so unit tests can keep using `CostModel::default()` for synthetic UPLC scripts. Reference: `Cardano.Ledger.Alonzo.Plutus.CostModels` — `mkCostModel` requires complete builtin coverage.
- ✅ **Protocol parameters keys 12 & 13** — `ProtocolParameters.d` (decentralization, CDDL key 12, `Option<UnitInterval>`) and `ProtocolParameters.extra_entropy` (CDDL key 13, `Option<Nonce>`) added with full CBOR map round-trip codec, `ProtocolParameterUpdate` propagation, `apply_update()` support, and Shelley genesis wiring (`decentralisationParam`, `extraEntropy` JSON). Present in Shelley–Alonzo, naturally absent in Babbage/Conway.
- ✅ **ppuWellFormed cross-field over-validation removal (Round 75, Gap AN)** — `conway_protocol_param_update_well_formed()` included three checks not in upstream `ppuWellFormed` (`Cardano.Ledger.Conway.PParams`): effective-zero checks merging proposed values with current protocol params, and a cross-field `max_tx_size > max_block_body_size` check. Upstream only validates individual proposed field values for non-zero without merging or cross-referencing. Removed the extra block and unused `protocol_params` parameter. Updated 2 existing tests, added 1 new regression test.

### Runtime Parity Hardening (March 2026)

- ✅ **P1 completed**: post-forge adoption check in block production loop (`Node.BlockProduction` traces for adopted vs not-adopted forged blocks), aligned with `cardano-node` `NodeKernel.forkBlockForging` post-forge checks.
- ✅ **P2 completed**: peer-attributable invalid-block handling now maps to reconnect-and-punish disposition with `ChainDB.AddBlockEvent.InvalidBlock` trace, aligned with upstream `InvalidBlockPunishment` behavior.
- ✅ **P3 completed**: far-future header rejection via consensus-level `judge_header_slot` (`ClockSkew`, `FutureSlotJudgement`) propagated as `SyncError::BlockFromFuture`, aligned with `InFutureCheck`.
- ✅ **P4 completed**: forged block self-validation in runtime forge flow before persistence, plus canonical forged `block_body_hash`/`block_body_size` computation from serialized body bytes.
- ✅ **Tests added**: targeted tests for adoption checks, invalid-block punishment routing, and future-slot judgement behavior.

### Functional Alignment Sprint (March–June 2026)

- ✅ **CIP-0069 PlutusV3 datum exemption (Round 70)** — `validate_unspendable_utxo_no_datum_hash()` now accepts optional `v3_script_hashes: Option<&HashSet<[u8; 28]>>`. Conway-era call sites collect V3 script hashes from both witness-set `plutus_v3_scripts` and reference-input UTxO `ScriptRef::PlutusV3`, and pass the set so V3-locked spending inputs are exempt from the datum-hash requirement. Alonzo/Babbage pass `None` (no CIP-0069). `collect_v3_script_hashes()` helper added. 4 new tests. Reference: `Cardano.Ledger.Conway.TxInfo` — `getInputDataHashesTxBody` (V3 branch absent from datum-hash obligation).
- ✅ **ScriptIntegrityHashMismatch PV split (Round 70)** — `validate_script_data_hash()` now accepts `protocol_version: Option<(u64, u64)>`. At PV >= 11 (Conway post-bootstrap), hash mismatches return the new `ScriptIntegrityHashMismatch { declared, computed }` error instead of legacy `PPViewHashesDontMatch`. All 6 call sites pass the current protocol version. 2 new PV-boundary tests. Reference: `Cardano.Ledger.Conway.Rules.Utxow` — `ScriptIntegrityHashMismatch` replaces `PPViewHashesDontMatch` at PV >= 11.
- ✅ **Withdrawal/cert ordering parity (Round 71)** — `apply_certificates_and_withdrawals_with_future()` now drains reward-account withdrawals BEFORE processing certificates, matching the upstream Conway CERTS recursive STS base case (`conwayCertsTransition` in `Cardano.Ledger.Conway.Rules.Certs`). The `Empty` branch validates and drains withdrawals, updates DRep expiries, then the inductive step processes individual certificates over the drained state. This ordering is semantically relevant: a transaction that withdraws from a reward account AND unregisters the same credential succeeds because draining sets the balance to zero before the `StakeCredentialHasRewards` check in `unregister_stake_credential()`. At PV >= 11 (`hardforkConwayMoveWithdrawalsAndDRepChecksToLedgerRule`) the drainage is lifted from the CERTS base case into LEDGER but still precedes certificate processing. 2 new tests cover the same-tx withdraw+unregister scenario for both `AccountUnregistration` and `AccountUnregistrationDeposit`.
- ✅ **PeerSharing governor-driven dispatch** — `GovernorAction::ShareRequest(peer)` now dispatches actual PeerSharing protocol: looks up the specific peer in warm peers, calls `share_peers(amount).await`, merges discovered peers into `PeerRegistry` via `sync_peer_share_peers()`, records success/failure on `GovernorState`. Removed non-upstream blanket `refresh_peer_share_sources()` fan-out to all warm peers (upstream `simplePeerSelectionPolicy` targets individual peers via governor actions, not periodic all-peer fan-out). Reference: `Ouroboros.Network.PeerSelection.Governor.KnownPeers`.
- ✅ **Tracer Forwarder backend fix** — `backends_for()` in `tracer.rs` now correctly returns `Some(TraceBackend::Forwarder)` for the `"Forwarder"` backend string. Previously, `"Forwarder"` was mapped to `None` causing trace events to be silently dropped even though the `Forwarder` handler was fully implemented. Removed dead match arm.
- ✅ **Handshake version-aware decode** — `decode_version_data()` in `handshake.rs` now handles 2–4 element CBOR arrays: V7–V10 use 2 elements `[networkMagic, initiatorOnlyDiffusionMode]`, V11–V12 use 3 elements `[..., peerSharing]`, V13+ use 4 elements `[..., peerSharing, query]`. Previously rigid 4-element decode would reject legacy protocol versions. 3 new integration tests added.
- ✅ **Mempool UTxO conflict detection** — After block application, sync pipeline now extracts all consumed inputs from applied blocks via `extract_consumed_inputs()` and calls `Mempool::remove_conflicting_inputs()` to evict any mempool transaction whose inputs overlap with inputs consumed by on-chain transactions. This complements existing `remove_confirmed()` (TxId-based) and `purge_expired()` (TTL-based) eviction. Reference: `Ouroboros.Consensus.Mempool.Impl.Update` — `syncWithLedger` re-validates mempool entries against new ledger state.
- ✅ **NtC handshake accept** — `ntc_accept()` in `ntc_peer.rs` implements full Node-to-Client handshake accept: decodes `ProposeVersions` CBOR, negotiates best common version from `NTC_SUPPORTED_VERSIONS` (V16–V9), decodes `NodeToClientVersionData` (networkMagic + query), validates magic, sends `AcceptVersion` or `RefuseVersionMismatch`. Wired into `local_server.rs` replacing raw mux start for Unix domain socket connections. 5 new integration tests. Reference: `Ouroboros.Network.Protocol.Handshake`.
- ✅ **TxSubmission server blocking-first** — Inbound `TxSubmissionServer` now uses always-blocking `request_tx_ids(true, ack, batch)` in its main collection loop, matching upstream `serverPeer` basic collection pattern. Initially attempted non-blocking probe alternation but reverted after test validation — upstream `localTxSubmissionPeerNull` and `txSubmissionServer` use blocking in standard mode. Reference: `Ouroboros.Network.TxSubmission.Inbound`.
- ✅ **Ratification guard semantics fix** — `ratify_and_enact()` now evaluates `valid_committee_term()` and `withdrawal_can_withdraw()` against the **evolving** ledger enact state on each loop iteration (current protocol params + current treasury), matching upstream `ratifyTransition` guard checks over `ensCurPParams` and `ensTreasury`. Added regression coverage for progressive treasury withdrawals in a single ratification pass. Reference: `Cardano.Ledger.Conway.Rules.Ratify`.
- ✅ **Shelley `dsFutureGenDelegs` scheduling** — `GenesisDelegation` certificates now schedule into `future_gen_delegs` at `current_slot + stability_window` (when configured), duplicate delegate/VRF checks include both active and future maps, and adoption into active `gen_delegs` occurs once slot reaches activation. Wired into both block-apply and submitted-tx paths with regression tests. Reference: `Cardano.Ledger.Shelley.Rules.Deleg` (`dsFutureGenDelegs`, `adoptGenesisDelegs`).
- ✅ **Conway DELEG deposit error hardfork split** — key-registration deposit mismatches now follow upstream `hardforkConwayDELEGIncorrectDepositsAndRefunds`: bootstrap PV9 returns legacy `IncorrectDepositDELEG`, post-bootstrap PV10+ returns `DepositIncorrectDELEG`. Applied across all four Conway registration cert shapes (`AccountRegistrationDeposit`, `AccountRegistrationDelegationToStakePool`, `...ToDrep`, `...ToStakePoolAndDrep`) with regression coverage. Reference: `Cardano.Ledger.Conway.Rules.Deleg`.
- ✅ **Reconnect peer punishment** — All three reconnecting sync functions now demote the offending peer to `PeerStatus::PeerCold` on `ReconnectAndPunish` disposition, enabling the governor's backoff/forget logic to penalize misbehaving peers. Previously the peer status was left unchanged on reconnection. Reference: upstream `InvalidBlockPunishment` intent (close connection → governor sees cold).
- ✅ **Reconnect exponential backoff** — `ReconnectingRunState` tracks `consecutive_failures` (reset on successful batch progress). `reconnect_backoff()` applies exponential delay starting at 1 s from the second consecutive failure, doubling up to 60 s cap. First reconnection (peer rotation) has zero delay. Wired into all three reconnecting sync loop outer iterations with shutdown-aware `tokio::select`. Reference: `peerBackerOff` exponential backoff in `Ouroboros.Network.PeerSelection.Governor`.
- ✅ **Circulation-based sigma in maxPool** — `compute_pool_reward()` now uses `totalStake = maxLovelaceSupply - reserves` (upstream circulation) as the sigma/pledge denominator instead of active stake. `RewardParams` carries `max_lovelace_supply: u64` wired from `ShelleyGenesis.maxLovelaceSupply`. Reference: `Cardano.Ledger.Shelley.LedgerState.PulsingReward` — `startStep` uses `circulation` for `maxP`.
- ✅ **Eta monetary expansion** — `compute_epoch_reward_pot()` now applies `deltaR1 = floor(min(1, eta) * rho * reserves)` with `eta = blocksMade / expectedBlocks` when `d < 0.8` (or `eta = 1` when `d >= 0.8`). `compute_eta()` in `epoch_boundary.rs` derives eta from `active_slot_coeff`, `slots_per_epoch`, and `blocks_made`. Reference: `Cardano.Ledger.Shelley.LedgerState.PulsingReward` — `startStep`.
- ✅ **Leader reward when f ≤ cost** — When pool apparent performance reward ≤ declared cost, the operator now receives the full apparent reward (not 0). Reference: `Cardano.Ledger.Shelley.Rewards` — `calcStakePoolOperatorReward`.
- ✅ **MIR admission validation (Round 72)** — All 7 upstream DELEG MIR checks now enforced via `MirValidationContext` in `MoveInstantaneousReward` cert processing: `MIRCertificateTooLateinEpochDELEG` (current_slot must be before `firstSlot(nextEpoch) - stabilityWindow`), `MIRNegativesNotCurrentlyAllowed` (pre-Alonzo rejects negative credential deltas), `MIRProducesNegativeUpdate` (Alonzo+ rejects combined existing+new map entries that go negative), `InsufficientForInstantaneousRewardsDELEG` (source pot must cover sum of positive credential deltas after combining with existing IR state), `MIRTransferNotCurrentlyAllowed` (pre-Alonzo rejects `SendToOppositePot`), `MIRNegativeTransfer` (inherently satisfied by u64), `InsufficientForTransferDELEG` (source pot must cover transfer amount after existing IR commitments). Era-gated via `alonzo_mir_transfers` flag matching upstream `hardforkAlonzoAllowMIRTransfer` (false for Shelley/Allegra/Mary, true for Alonzo/Babbage). Wired at all 10 Shelley–Babbage call sites; Conway passes `None` (MIR certs already rejected as `UnsupportedCertificate`). 8 new tests. Reference: `Cardano.Ledger.Shelley.Rules.Deleg` — MIR predicates.
- ✅ **VRF key uniqueness PV gating + running UTxO ref-script (Round 73)** — Two upstream parity fixes. (1) `VRFKeyHashAlreadyRegistered` POOL rule now gated on `post_pv10` (PV > 10) instead of `is_conway` (era ≥ 7), matching upstream `hardforkConwayDisallowDuplicatedVRFKeys pv = pvMajor pv > natVersion @10` — Conway bootstrap (PV9/PV10) allows duplicate VRF keys. (2) BBODY `totalRefScriptSizeInBlock` now uses running UTxO overlay at PV > 10: each tx's outputs (valid → regular outputs, invalid → collateral return at `index = len(outputs)` per upstream `mkCollateralTxIn`) are accumulated in a `HashMap` overlay so subsequent txs see prior tx outputs for ref-script size measurement. PV ≤ 10 retains static pre-block UTxO behavior. Audited POOL (6 predicates), UTXO/UTXOW (22 predicates), and BBODY/LEDGER rules (13 predicates) — all implemented. 3 new tests, 4087 total.
- ✅ **Multi-owner pool exclusion** — All pool owners (from `PoolParams.pool_owners`) are now excluded from member rewards, not just the operator. Leader absorbs the margin share of all owner-delegated stake. Reference: `Cardano.Ledger.Shelley.Rewards` — `rewardOnePoolMember` `notPoolOwner`.
- ✅ **Single-floor leader/member formulas** — Leader and member reward formulas now use single-floor rational arithmetic via `floor_mul_div()` helper, matching upstream `calcStakePoolOperatorReward` and `calcStakePoolMemberReward` exactly. Reference: `Cardano.Ledger.Shelley.Rewards`.
- ✅ **Withdrawal budget parity (Round 76 — Gap AO)** — `ratify_and_enact()` now tracks a `withdrawal_budget` that is decremented by the FULL proposed amount of each enacted `TreasuryWithdrawals` (including amounts to unregistered accounts), matching upstream `ensTreasury st <-> wdrlsAmount` in the ENACT rule. Previously the budget check used the live treasury which was only decremented for registered accounts, potentially allowing subsequent withdrawal proposals to pass when the aggregate would exceed the original treasury. Reference: `Cardano.Ledger.Conway.Rules.Enact` and `Cardano.Ledger.Conway.Rules.Ratify.withdrawalCanWithdraw`.
- ✅ **Donation ordering parity (Round 77 — Gap AP)** — `flush_donations_to_treasury()` moved from BEFORE ratification to AFTER ratification and unclaimed deposit crediting, matching upstream `Cardano.Ledger.Conway.Rules.Epoch` ordering where `casTreasuryL <>~ utxosDonationL` runs after `applyEnactedWithdrawals` / `proposalsApplyEnactment` / `returnProposalDeposits`. Previously donations inflated the `withdrawal_budget` used by `withdrawal_can_withdraw()`, potentially allowing treasury withdrawals that exceed the pre-donation treasury. New test `test_donation_not_included_in_withdrawal_budget` validates the fix.
- ✅ **Performance snapshot parity (Round 77 — Gap AQ)** — `derive_pool_performance()` now uses `snapshots.go` (matching upstream `ssStakeGo` used by `mkApparentPerformance` in `mkPoolRewardInfo` called from `startStep`) instead of `snapshots.set` for computing sigma ratios. This aligns the stake distribution used for apparent performance with the one used for reward distribution in `compute_epoch_rewards()`. Reference: `Cardano.Ledger.Shelley.LedgerState.PulsingReward` — `startStep` reads `ssStakeGo`.
- ✅ **Fee handling and pulsing phase-shift documentation (Round 77)** — Documented the inline-vs-pulsed reward model difference: our `fee_pot` approach uses identical fee values and `go` snapshot as upstream `ssFee` + `ssStakeGo`, but applies rewards one epoch earlier (inline at boundary N) versus upstream's two-phase pulsed model (compute during epoch N, apply at boundary N+1). This is a structural design choice, not a data-level discrepancy.
- ✅ **Unregistered rewards → treasury** — `distribute_rewards()` now returns the total lovelace for unregistered reward accounts; `apply_epoch_boundary()` adds this amount to treasury. Reference: `Cardano.Ledger.Shelley.LedgerState.IncrementalStake` — `applyRUpdFiltered` `frTotalUnregistered`.
- ✅ **Zero-block pool performance default** — Pools not appearing in the `blocks_made` map now receive zero rewards (performance 0/1), matching upstream `mkPoolRewardInfo` which returns `Left` (no reward) for zero-block pools. Previously defaulted to perfect performance.
- ✅ **Pledge satisfaction check** — `compute_pool_reward()` computes actual owner-delegated stake vs declared pledge; pools failing the pledge check receive zero rewards. Reference: `Cardano.Ledger.Shelley.Rewards` — `mkPoolRewardInfo` `pledgeIsMet`.
- ✅ **Member reward credential-based keying** — `distribute_rewards()` now applies member rewards via `StakeCredential` (using `credit_by_credential()`) and leader rewards via `RewardAccount` (using `leader_deltas`). Matches upstream `rewardOnePoolMember` which returns `Maybe Coin` keyed by `Credential 'Staking`. Reference: `Cardano.Ledger.Shelley.Rewards`, `applyRUpdFiltered`.
- ✅ **PPUP/UPEC ordering** — `apply_epoch_boundary()` now applies pending protocol-parameter updates AFTER SNAP and POOLREAP (step 3b), matching the upstream NEWEPOCH rule order (RUPD → MIR → EPOCH(SNAP → POOLREAP → UPEC)). Reward computation uses prevPParams from before the PPUP update. Reference: `Cardano.Ledger.Shelley.LedgerState.PulsingReward` — `startStep` reads `prevPParamsEpochStateL`.
- ✅ **Per-pool deposit tracking** — `RegisteredPool` now stores `deposit: u64` (upstream `spsDeposit` in `StakePoolState`). `register_with_deposit()` preserves the original deposit on re-registration (upstream `mkStakePoolState (currentState ^. spsDepositL)`). `retire_pools_with_refunds()` uses per-pool recorded deposit, not the current `pp_poolDeposit`. Reference: `Cardano.Ledger.Shelley.Rules.PoolReap` — `poolReapTransition`.
- ✅ **Unclaimed pool deposits → treasury** — When a retiring pool's reward account is unregistered, the deposit is now tracked as `unclaimed_pool_deposits` and credited to treasury. Reference: `Cardano.Ledger.Shelley.Rules.PoolReap` — `casTreasury = casTreasury a <+> fromCompact unclaimed`.
- ✅ **Per-credential deposit tracking (Conway `rdDeposit`)** — `StakeCredentialState` now stores `deposit: u64` (upstream `rdDeposit` in UMap). All five registration cert handlers (tags 0, 7, delegate-combo variants) record the deposit at registration time. Conway `AccountUnregistrationDeposit` (tag 8) validates the supplied refund against the stored per-credential deposit (upstream `lookupDeposit umap cred` / `checkInvalidRefund`), falling back to current `key_deposit` for legacy state with `deposit=0`. Shelley–Babbage tag 1 still uses current parameter. CBOR backward-compatible: 2-element legacy decodes as `deposit=0`. 5 new tests. Reference: `Cardano.Ledger.Conway.Rules.Deleg` — `checkInvalidRefund`, `Cardano.Ledger.UMap` — `lookupDeposit`.
- ✅ **Exact withdrawal amount enforcement (all eras)** — Withdrawal amount must exactly match reward account balance for ALL Shelley+ eras, not just Conway. Previously, Shelley–Babbage allowed partial withdrawals (withdraw less than full balance), violating the formal spec `wdrls ⊆ rewards`. Now `WithdrawalNotFullDrain` is enforced unconditionally. Reference: `Cardano.Ledger.Shelley.Rules.Utxo` — `validateWithdrawals`.
- ✅ **OutputTooBigUTxO for pre-Alonzo eras** — `validate_output_not_too_big` is now called in `validate_pre_alonzo_tx` (guarded by `max_val_size.is_some()`), extending the Mary-era output-value-size check that was previously only applied to Alonzo+. No-op for Shelley/Allegra where `max_val_size` is absent. Reference: `Cardano.Ledger.Mary.Rules.Utxo` — `validateOutputTooBigUTxO`.
- ✅ **Zero-valued multi-asset output rejection** — `validate_no_zero_valued_multi_asset()` rejects any transaction output containing a multi-asset entry with zero quantity. Wired into both `validate_pre_alonzo_tx` (Mary) and `validate_alonzo_plus_tx` (Alonzo+). 3 new tests. Reference: `Cardano.Ledger.Mary.Value` — non-zero invariant on `MaryValue`.
- ✅ **PlutusV1/V2/V3 language view encoding parity** — `encode_language_views_for_script_data_hash()` now matches upstream `getLanguageView` / `encodeLangViews` from `Cardano.Ledger.Alonzo.PParams`: PlutusV1 map key is double-serialized (CBOR byte string `0x41 0x00`), PlutusV2/V3 keys are single-serialized CBOR unsigned integers; PlutusV1 cost model value is CBOR byte-string wrapped (double-encoded), PlutusV2+ values are raw CBOR arrays; PlutusV1 cost model uses indefinite-length array encoding, V2/V3 use definite-length; map entries sorted by upstream `shortLex` (shorter tag bytes first) so V2/V3 entries precede V1 when mixed. 7 new parity tests cover key encoding, value encoding, array format, and mixed-language ordering.
- ✅ **Full tx size for maxTxSize and fee validation** — Block-apply paths now use `Tx::serialized_size()` (full on-wire CBOR: header + body + witnesses + is_valid + aux_data/null) instead of `tx.body.len()` (body-only) when computing tx size for `validateMaxTxSizeUTxO` and linear fee formula. This matches upstream which uses the full serialized transaction size. 3 new tests cover pre-Alonzo, Alonzo+, and body-vs-full size difference. Reference: `Cardano.Ledger.Shelley.Rules.Utxo` — `validateMaxTxSizeUTxO`.
- ✅ **Bootstrap witness validation for Byron inputs** — SHA3-256 + Blake2b-224 address root extraction from Byron addresses and bootstrap witness key hashes now properly generate witness obligations for Byron spending inputs. `required_vkey_hashes_from_inputs_shelley` and `_multi_era` extract 28-byte address roots from Byron addresses; `bootstrap_witness_key_hash` computes the corresponding hash from `BootstrapWitness` fields; `validate_witnesses_if_present` merges both VKey and bootstrap witness hashes into the `provided` set. Reference: `Cardano.Ledger.Keys.Bootstrap` — `bootstrapWitKeyHash`, `Cardano.Ledger.Address` — `bootstrapKeyHash`.
- ✅ **Collateral return output included in validation** — `validate_alonzo_plus_tx` now builds `all_outputs` including `collateral_return` for min-UTxO, output-size, zero-multi-asset, and boot-addr-attribute checks, matching upstream `allSizedOutputsTxBodyF`. Four `validate_output_network_ids` call sites (Babbage/Conway submitted + block-apply) also include collateral return. Reference: `Cardano.Ledger.Babbage.TxBody` — `allSizedOutputsTxBodyF`.
- ✅ **PPUP proposer witness requirements** — `required_vkey_hashes_from_ppup()` extracts proposer genesis key hashes from Shelley `ShelleyUpdate` proposals, filtered by `gen_delegs` membership, and inserts them into the required witness set. Wired into all 10 Shelley-through-Babbage paths (5 submitted-tx + 5 block-apply). Reference: `Cardano.Ledger.Shelley.UTxO` — `propWits`.
- ✅ **PPUP delegate key hash parity** — `required_vkey_hashes_from_ppup()` now inserts the genesis *delegate* key hash (`genDelegKeyHash`) instead of the genesis *owner* key hash, matching upstream `witsVKeyNeededGenDelegs` / `proposedUpdatesWitnesses` which applies `Map.intersection genDelegs pup` then `map genDelegKeyHash`. 2 new parity tests verify multi-proposer delegate hash extraction and deduplication. Reference: `Cardano.Ledger.Shelley.UTxO` — `witsVKeyNeededGenDelegs`.
- ✅ **Conway re-registration silent no-op** — Conway certificate tags 7 (AccountRegistrationDeposit), 11 (AccountRegistrationDelegationToStakePool), 12 (AccountRegistrationDelegationToDrep), and 13 (AccountRegistrationDelegationToStakePoolAndDrep) now silently skip registration and deposit charging when the credential is already registered, while still proceeding with delegation (for combo certs). Shelley tag 0 continues to error. Reference: `Cardano.Ledger.Conway.Rules.Deleg` — `when (not exists) $ do …`.
- ✅ **Pool retirement epoch lower bound** — Pool retirement now enforces `cEpoch < e` (retirement epoch must be strictly after current epoch), in addition to the existing `e <= cEpoch + eMax` upper bound. New `PoolRetirementTooEarly` error variant. Reference: `Cardano.Ledger.Shelley.Rules.Pool` — `StakePoolRetirementWrongEpochPOOL`.
- ✅ **Expired governance deposits credited to treasury** — When a governance proposal expires and its return reward account is no longer registered, the deposit now correctly accrues to the treasury instead of being silently dropped. Fixes a conservation-of-Ada leak. Reference: `Cardano.Ledger.Conway.Rules.Epoch` — `returnProposalDeposits`.
- ✅ **Per-redeemer ExUnits validation** — Each individual redeemer's `ExUnits` is now checked against `maxTxExUnits` (point-wise `<=`) in all 6 Alonzo+ paths (3 submitted + 3 block-apply), matching upstream `validateExUnitsTooBigUTxO` which checks `all pointWiseExUnits (<=) (snd <$> rdmrs) maxTxExUnits` rather than only the aggregate sum. Reference: `Cardano.Ledger.Alonzo.Rules.Utxo` — `validateExUnitsTooBigUTxO`.
- ✅ **Reference input disjointness Conway-only** — `validate_reference_input_disjointness()` is now enforced only in Conway era paths, matching upstream where Babbage allows overlapping spending+reference inputs and Conway added the disjointness constraint. Removed from Babbage block-apply and submitted-tx paths; 2 existing tests updated to assert Babbage overlap is accepted. Reference: `Cardano.Ledger.Conway.Rules.Utxo` — `disjointRefInputs`.
- ✅ **Genesis delegation validation** — `DCert::GenesisDelegation` handler now enforces three upstream checks: `GenesisKeyNotInMapping` (genesis hash must exist in `gen_delegs`), `DuplicateGenesisDelegate` (delegate must not already be used by another genesis key), `DuplicateGenesisVrf` (VRF key must not already be used by another genesis key). 4 new tests. Reference: `Cardano.Ledger.Shelley.Rules.Delegs` — `DELEG` rule.
- ✅ **Duplicate pool owner rejection** — `DCert::PoolRegistration` now rejects pools with duplicate entries in `pool_owners` via `DuplicatePoolOwner` error. Reference: `Cardano.Ledger.Shelley.Rules.Pool` — `StakePoolDuplicateOwnerPOOL`.
- ✅ **Certificate era-gating** — Conway-only certificates (tags 7–18) are rejected in pre-Conway eras with `UnsupportedCertificate("Conway certificate in pre-Conway era")`. Pre-Conway-only certificates (tags 5–6: `GenesisDelegation`, `MIRCert`) are rejected in Conway era with `UnsupportedCertificate("pre-Conway certificate in Conway era")`. Universal certificates (tags 0–4) are accepted in all eras. 3 new tests + 14 existing tests updated. Reference: `Cardano.Ledger.Conway.Rules.Cert` — era-specific `DCert` variants.
- ✅ **UpdateNotAllowedConway** — Conway-era CBOR decoder now explicitly rejects the Shelley `update` field (CDDL key 6) with `UpdateNotAllowedConway` instead of silently skipping it via catch-all. 1 new test. Reference: `Cardano.Ledger.Conway.Rules.Utxos` — `UpdateNotAllowed`.
- ✅ **TxId raw body byte preservation** — `ShelleyCompatibleSubmittedTx` and `AlonzoCompatibleSubmittedTx` now preserve the original wire CBOR bytes of the transaction body in a `raw_body` field, populated during decode. `tx_id()` hashes the original on-wire bytes instead of re-serializing from typed fields. This ensures correct TxId computation for non-canonically encoded transactions submitted via LocalTxSubmission. Reference: upstream `Cardano.Ledger.Core` — `txIdTxBody` uses `originalBytes`.
- ✅ **SNAP-before-adopt ordering** — `apply_epoch_boundary()` now takes the mark snapshot BEFORE activating `psFutureStakePoolParams` (via `adopt_future_params()`), matching upstream EPOCH rule ordering SNAP → POOLREAP. Previously the Rust code adopted future pool params first, causing the mark snapshot to contain post-adoption pool params one epoch early. 1 new parity test. Reference: `Cardano.Ledger.Shelley.Rules.Epoch` — SNAP runs before POOLREAP; `Cardano.Ledger.Shelley.Rules.PoolReap` — `poolReapTransition` activates future params.
- ✅ **Dormant epoch counter semantics** — `apply_epoch_boundary()` no longer resets `num_dormant_epochs` to 0 when governance proposals exist; it only increments when proposals are empty and leaves the counter unchanged otherwise. Matches upstream `updateNumDormantEpochs` which only calls `succ` (never resets at epoch boundary). The per-tx `updateDormantDRepExpiries` in the GOV rule is responsible for the counter reset. 1 new parity test + 1 integration test updated. Reference: `Cardano.Ledger.Conway.Rules.Epoch` — `updateNumDormantEpochs`.
- ✅ **Slot-to-POSIX time conversion in ScriptContext** — `CekPlutusEvaluator` now carries `system_start_unix_secs` and `slot_length_secs` for converting slot numbers to POSIX milliseconds in `posix_time_range`. When configured, ScriptContext validity intervals encode real POSIX timestamps instead of raw slot numbers. `slot_to_posix_ms()` in `genesis.rs` implements the conversion. `VerifiedSyncServiceConfig.build_plutus_evaluator()` wires genesis `system_start` through to the evaluator. 6 new tests. Reference: `Cardano.Ledger.Alonzo.Plutus.TxInfo` — `slotToPOSIXTime`.
- ✅ **PPUP/MIR collection gates `is_valid=false` (Round 50)** — Alonzo and Babbage `apply_block()` post-loops now skip `is_valid=false` transactions when collecting PPUP proposals and MIR certificate accumulation. Upstream `alonzoEvalScriptsTxInvalid` returns `pure pup` (no PPUP collection) and does not run DELEGS (no MIR), but our pre-fix code iterated all decoded transactions unconditionally. 2 new integration tests. Reference: `Cardano.Ledger.Alonzo.Rules.Utxos` — `alonzoEvalScriptsTxInvalid`.
- ✅ **PV9 Conway bootstrap deposit omission in PlutusV3 ScriptContext (Round 50)** — `tx_cert_data_v3()` now accepts `protocol_version` from `TxContext`; when PV major == 9 (Conway bootstrap phase), `AccountRegistrationDeposit` and `AccountUnregistrationDeposit` omit the deposit/refund field (`Nothing` instead of `Just deposit`), matching upstream `hardforkConwayBootstrapPhase` in `Cardano.Ledger.Conway.TxInfo.transTxCert` (bug #4863). PV >= 10 includes deposits normally. `TxContext.protocol_version` threaded through all 6 construction sites. 3 new tests. Reference: `Cardano.Ledger.Conway.TxInfo` — `transTxCert`.
- ✅ **Conway hardfork gate PV boundary correction (Round 68)** — Three PV boundary bugs fixed where our code applied gates at PV >= 10 (using `!bootstrap_phase`) but upstream uses PV > 10 (PV 11+):
  1. **Deposit/refund error split** — `DepositIncorrectDELEG` and `RefundIncorrectDELEG` error variants now correctly activate only at PV > 10 (`ctx.post_pv10`), not PV >= 10. PV 10 uses legacy `IncorrectDepositDELEG`/`IncorrectKeyDepositRefund`. Added `conway_post_pv10()` helper and `post_pv10: bool` field to `CertificateValidationContext`. Fixed across all 5 error-selection sites. Reference: `Cardano.Ledger.Conway.Era` — `harforkConwayDELEGIncorrectDepositsAndRefunds pv = pvMajor pv > natVersion @10`.
  2. **Unelected committee voters** — `validate_unelected_committee_voters()` now skips the check when PV <= 10 (previously skipped only at PV < 10). Reference: `Cardano.Ledger.Conway.Era` — `harforkConwayDisallowUnelectedCommitteeFromVoting pv = pvMajor pv > natVersion @10`.
  3. **Test coverage** — 4 new PV-boundary tests (PV 10 legacy deposit error, PV 10 legacy refund error, PV 10 unelected voters allowed, PV 11 rejection). Renamed existing tests from `_pv10` to `_pv11` to reflect corrected boundary. 4066 tests pass, 0 failures.
- ✅ **Conway GOV/ppuWellFormed parity alignment (Round 69)** — 6 parity gaps fixed by cross-referencing upstream `Cardano.Ledger.Conway.Rules.Gov` (18 `ConwayGovPredFailure` constructors) and `Cardano.Ledger.Conway.PParams` (`ppuWellFormed`):
  1. **Removed `ExpirationEpochTooLarge` over-validation** — Upstream GOV rule has NO committee term-limit check on `UpdateCommittee` proposals; only `ExpirationEpochTooSmall` and `ConflictingCommitteeUpdate` are checked. Removed the extra `ExpirationEpochTooLarge` error variant, the check in `validate_conway_proposals()`, and 2 tests that asserted the wrong behavior. Reference: `Cardano.Ledger.Conway.Rules.Gov` — `conwayGovTransition`.
  2. **Removed 3 ppuWellFormed over-validations** — `max_collateral_inputs == 0`, `min_committee_size == 0`, and `drep_activity == 0` are NOT in upstream's `ppuWellFormed` check list. Removed from `conway_protocol_param_update_well_formed()`. Updated 2 existing tests that asserted the wrong rejection behavior. Reference: `Cardano.Ledger.Conway.PParams` — `ppuWellFormed`.
  3. **Added `coinsPerUTxOByte == 0` rejection (post-bootstrap only)** — Upstream: `hardforkConwayBootstrapPhase pv || isValid ((/= CompactCoin 0) . unCoinPerByte) ppuCoinsPerUTxOByteL`. Added bootstrap-phase gate to `conway_protocol_param_update_well_formed()`. Function now accepts `protocol_version: Option<(u64, u64)>` parameter.
  4. **Added `nOpt == 0` rejection (PV >= 11 only)** — Upstream: `pvMajor pv < natVersion @11 || isValid (/= 0) ppuNOptL`. Added PV 11+ gate via `conway_post_pv10()`.
  5. **Test coverage** — 4 new tests: ppuWellFormed accepts fields not in upstream zero list, coinsPerUTxOByte zero rejected post-bootstrap / accepted in bootstrap, nOpt zero rejected at PV 11 / accepted at PV 10, UpdateCommittee expiration beyond term limit accepted. 4068 tests pass, 0 failures.

---

## Parity Matrix: Current vs. Upstream

### Legend
- ✅ **Complete** — Feature fully implemented and tested
- ⚠️ **Partial** — Core logic present, edge cases/optimization needed
- 🚧 **In Progress** — Active implementation
- ⏸️ **Design Only** — Skeleton/types present, behavior not implemented
- ❌ **Not Started** — Upstream feature not yet addressed

### LEDGER SUBSYSTEM

| Feature | Scope | Haskell | Rust | Status | Notes |
|---------|-------|---------|------|--------|-------|
| **Era Support** |
| Byron | Epoch/slot structure, transactions, rewards | ✅ | ✅ | Complete | ByronBlock with envelope format, tx decode, rewards
| Shelley | UTxO model, certs, pools, withdrawals | ✅ | ✅ | Complete | ShelleyBlock, certificate hierarchy, delegation
| Allegra | Native scripts, timelock | ✅ | ✅ | Complete | NativeScript evaluation, valid_from/valid_until
| Mary | Multi-asset, minting | ✅ | ✅ | Complete | Value, MultiAsset, minting policies
| Alonzo | Plutus V1/V2, datums, redeemers | ✅ | ✅ | Complete | PlutusData AST, script refs, Phase-2 Plutus validation wired in block + submitted-tx paths
| Babbage | Inline datums, inline scripts, Praos | ✅ | ✅ | Complete | PraosHeader, inline datum/script types, DatumOption
| Conway | Governance, DReps, ratification, votes | ✅ | ✅ | Complete | Types, ratification, enactment, deposit lifecycle, subtree pruning
| **Core State** |
| UTxO tracking | Coin + multi-asset semantics | ✅ | ✅ | Complete | ShelleyUtxo + MultiEraUtxo with era dispatch
| Account state | Rewards + deposits tracking | ✅ | ✅ | Complete | DepositPot, treasury, reserves; reward snapshot + RUPD distribution
| MIR (Move Instantaneous Rewards) | DCert tag 6, Shelley–Babbage | ✅ | ✅ | Complete | InstantaneousRewards accumulation per block/submitted-tx, epoch-boundary all-or-nothing payout + pot-to-pot transfers; 26 MIR tests
| MIR genesis quorum | validateMIRInsufficientGenesisSigs, Shelley–Babbage | ✅ | ✅ | Complete | genesis_update_quorum field (ShelleyGenesis.updateQuorum, default 5); gen_delg_hash_set; validate_mir_genesis_quorum_if_present + _typed wired into all 5 block-apply and 5 submitted-tx paths; 8 tests
| Pool state | Registration, retirement, performance | ✅ | ✅ | Complete | PoolState, PoolParams, retire queues, stake snapshots
| Delegation state | Stake delegation per account | ✅ | ✅ | Complete | Delegations mapping
| **Validation** |
| Syntax validation | TX format, field presence | ✅ | ✅ | Complete | CBOR roundtrip, field checks
| Input availability | UTxO membership checks | ✅ | ✅ | Complete | apply_block validates input existence
| Fee sufficiency | Linear fee + script fee | ✅ | ✅ | Complete | fees.rs with min_fee calculation
| Witness sufficiency | VKey hash + signature count | ✅ | ✅ | Complete | verify_vkey_signatures with Ed25519
| Native script eval | Timelock constraints | ✅ | ✅ | Complete | validate_native_scripts_if_present
| Plutus validation | Script execution + budget | ✅ | ✅ | Complete | CEK framework + Phase-2 validation wired in block + submitted-tx paths (Alonzo/Babbage/Conway); Phase-1 `validate_no_extra_redeemers` unconditionally before `is_valid` dispatching (upstream `hasExactSetOfRedeemers` in UTXOW)
| Missing cost model rejection | `NoCostModel` collect error by language key | ✅ | ✅ | Complete | validate_plutus_scripts checks `ProtocolParameters.cost_models` for required Plutus V1/V2/V3 keys (0/1/2) before CEK evaluation; soft-skipped when `cost_models` is absent
| Script integrity hash parity | `PPViewHashesDontMatch` / `ScriptIntegrityHashMismatch`, language views, era-specific redeemer encoding | ✅ | ✅ | Complete | `validate_script_data_hash` now mirrors upstream `mkScriptIntegrity`: language views are derived from the scripts actually needed by the transaction, including Babbage/Conway reference-input script refs when they satisfy `scriptsNeeded`, while preserving the Alonzo/Babbage legacy redeemer-array encoding and Conway map-format redeemers. Bidirectional enforcement: `MissingRequiredScriptIntegrityHash` when redeemers present but no hash declared, `UnexpectedScriptIntegrityHash` when hash declared but no redeemers present. PV split: at PV >= 11 hash mismatch returns `ScriptIntegrityHashMismatch` instead of `PPViewHashesDontMatch` (upstream `ppViewHashesDontMatch` full 4-way check + `Cardano.Ledger.Conway.Rules.Utxow` `ScriptIntegrityHashMismatch`)
| Collateral checks | Alonzo+ collateral UTxO | ✅ | ✅ | Complete | validate_collateral with VKey-locked + mandatory-when-scripts
| Min UTxO enforcement | Per-output minimum lovelace | ✅ | ✅ | Complete | min_utxo.rs with era-aware calculation
| Metadata size validation | `InvalidMetadata` / `validMetadatum` soft fork | ✅ | ✅ | Complete | validate_auxiliary_data enforces <= 64-byte bytes/text metadatum values from protocol version > (2, 0), including nested array/map entries
| Network address validation | WrongNetwork + WrongNetworkWithdrawal + WrongNetworkInTxBody | ✅ | ✅ | Complete | validate_output_network_ids, validate_withdrawal_network_ids, validate_tx_body_network_id across all 6 eras
| PPUP proposal validation | NonGenesisUpdatePPUP, PPUpdateWrongEpoch, PVCannotFollowPPUP | ✅ | ✅ | Complete | validate_ppup_proposal (Shelley.Rules.Ppup parity): genesis-delegate authorization, epoch voting-period (with optional slot-of-no-return), pvCanFollow protocol-version succession; wired into all 5 block-apply paths (Shelley–Babbage); 18 tests
| **Epoch Boundary** |
| Stake snapshot | per-pool reward snapshot | ✅ | ✅ | Complete | compute_stake_snapshot with fees
| Reward calculation | Per-epoch payouts | ✅ | ✅ | Complete | compute_epoch_rewards with upstream RUPD→SNAP ordering, delta_reserves accounting, eta monetary expansion, circulation-based sigma, single-floor leader/member formulas, multi-owner pool exclusion, zero-block pool default, unregistered rewards→treasury, credential-based member reward keying, PPUP/UPEC ordering (rewards use prevPParams)
| Pool retirement | Age-based expiry | ✅ | ✅ | Complete | retire_pools_with_refunds uses per-pool recorded deposit (spsDeposit); unclaimed deposits → treasury
| DRep inactivity | drep_activity threshold | ✅ | ✅ | Complete | touch_drep_activity, inactive_dreps
| Governance expiry | Proposal age limit | ✅ | ✅ | Complete | remove_expired_governance_actions
| Treasury donation | Conway utxosDonation accumulation + epoch flush | ✅ | ✅ | Complete | Per-tx accumulate_donation (UTXOS rule) in both block-apply and submitted-tx paths, epoch-boundary flush_donations_to_treasury (EPOCH rule), value preservation includes donation
| Treasury value check | Conway `current_treasury_value` declaration | ✅ | ✅ | Complete | `validate_conway_current_treasury_value` runs as Phase-1 UTXO rule in both block-apply and submitted-tx paths (before Plutus evaluation), matching upstream `Cardano.Ledger.Conway.Rules.Utxo` ordering
| Deposit/refund preservation | Certificate deposits + refunds in UTxO balance | ✅ | ✅ | Complete | CertBalanceAdjustment flows through all 6 per-era UTxO functions: consumed + withdrawals + refunds = produced + fee + deposits [+ donation]. Covers all 19 DCert variants
| **Governance** |
| Proposal storage | Action ID + metadata | ✅ | ✅ | Complete | GovActionState with vote maps
| Vote accumulation | Committee/DRep/SPO votes | ✅ | ✅ | Complete | apply_conway_votes with per-voter class
| Enacted-root validation | Lineage + prev-action-id | ✅ | ✅ | Complete | validate_conway_proposals with EnactState
| Ratification tally | Threshold voting | ✅ | ✅ | Complete | tally_* functions, AlwaysNoConfidence auto-yes, CC expired-member term filtering, epoch-boundary ratification+enactment+deposit lifecycle
| Enactment | Constitution, committee, params | ✅ | ✅ | Complete | enact_gov_action with 7 action types
| Deposit refund | Key/pool/DRep deposit return | ✅ | ✅ | Complete | Enacted+expired+lineage-pruned deposits refunded; unclaimed→treasury
| Lineage subtree pruning | proposalsApplyEnactment | ✅ | ✅ | Complete | remove_lineage_conflicting_proposals with purpose-root chain validation
| Unelected committee voters | PV ≥ 10 gate on voting credentials | ✅ | ✅ | Complete | validate_unelected_committee_voters + authorized_elected_hot_committee_credentials
| DRep delegation error | DelegateeNotRegisteredDELEG | ✅ | ✅ | Complete | DelegateeDRepNotRegistered error variant in delegate_drep
| Conway deregistration rewards | ConwayUnRegCert enforces reward check | ✅ | ✅ | Complete | tag 8 calls unregister_stake_credential (same reward-balance check as tag 1)
| DRep delegation clearing | clearDRepDelegations on DRep unreg | ✅ | ✅ | Complete | StakeCredentials::clear_drep_delegation in unregister_drep
| Committee future member | isPotentialFutureMember check | ✅ | ✅ | Complete | is_potential_future_member scans UpdateCommittee proposals for auth/resign
| ZeroTreasuryWithdrawals gate | Bootstrap phase bypass (PV < 10) | ✅ | ✅ | Complete | past_bootstrap guard on ZeroTreasuryWithdrawals check
| Bootstrap DRep delegation | preserveIncorrectDelegation (PV < 10) | ✅ | ✅ | Complete | delegate_drep skips checkDRepRegistered during bootstrap phase (upstream Cardano.Ledger.Conway.Rules.Deleg); 4 integration tests
| Bootstrap DRep expiry | computeDRepExpiryVersioned bootstrap gate | ✅ | ✅ | Complete | apply_conway_votes and touch_drep_activity_for_certs skip dormant epoch subtraction during bootstrap (upstream Cardano.Ledger.Conway.Rules.GovCert)
| Withdrawal DRep delegation | `ConwayWdrlNotDelegatedToDRep` post-bootstrap gate | ✅ | ✅ | Complete | validate_withdrawals_delegated checks pre-CERTS key-hash withdrawal credentials for existing DRep delegation, skips during bootstrap, and excludes script-hash reward accounts
| TriesToForgeADA | `adaPolicy ∉ supp mint tx` (Mary–Conway) | ✅ | ✅ | Complete | validate_no_ada_in_mint rejects mint maps containing the ADA policy ID ([0u8; 28]); wired into all 4 mint-capable era UTxO apply functions (Mary/Alonzo/Babbage/Conway) covering both block-apply and submitted-tx paths; 6 unit tests

**Ledger Summary**: ~96% feature complete. Phase-2 Plutus validation wired for both block and submitted-tx paths across all Alonzo+ eras.

---

### CONSENSUS SUBSYSTEM

| Feature | Scope | Haskell | Rust | Status | Notes |
|---------|-------|---------|------|--------|-------|
| **Leadership & Slot Election** |
| Leader value check | Praos VRF output → stake ratio | ✅ | ✅ | Complete | check_leader_value with Rational arithmetic
| VRF verification | Ed25519-based Praos VRF | ✅ | ✅ | Complete | verify_vrf_output with key material
| KES verification | Updating signatures | ✅ | ✅ | Complete | verify_opcert with key period + counter
| OpCert validation | Sequence number enforcement | ✅ | ✅ | Complete | Sequence gaps rejected
| **Chain State** |
| Volatility tracking | Recent blocks <3k slots | ✅ | ✅ | Complete | ChainState with tip + reachable blocks
| Immutability detection | Blocks >3k slots old | ✅ | ✅ | Complete | stable_count + drain_stable
| Rollback depth | Max 3k-slot reorg | ✅ | ✅ | Complete | enforce_max_rollback_depth
| Slot continuity | No gaps in block sequence | ✅ | ✅ | Complete | slot_continuity checks in validation
| **Nonce Evolution** |
| Epoch transition | UPDN + TICKN rules | ✅ | ✅ | Complete | NonceEvolutionState with prev_hash tracking
| VRF nonce mix | Per-block nonce contribution | ✅ | ✅ | Complete | apply_block updates epoch_nonce via VRF
| **Block Validation Sequence** |
| Header format | CBOR parsing + field extraction | ✅ | ✅ | Complete | Multi-era dispatch
| Slot/time check | Slot within epoch | ✅ | ✅ | Complete | slot < epoch_size validation
| Chain continuity | Prev hash match | ✅ | ✅ | Complete | verify_block_prev_hash
| BlockNo sequence | Incrementing | ✅ | ✅ | Complete | blockNo validation
| Issuer validation | Known pool + stake | ✅ | ✅ | Complete | verify_block_vrf_with_stake wired in production sync (verify_vrf defaults true)
| VRF check | Leader eligibility | ✅ | ✅ | Complete | verify_block_vrf
| OpCert check | Valid + not superseded | ✅ | ✅ | Complete | OpCert validation; OcertCounters runtime wiring (permissive-mode counter tracking threads across sync batches and reconnects)
| OpCert counter enforcement | Per-pool monotonic seq no | ✅ | ✅ | Complete | OcertCounters.validate_and_update; threaded through batch/service/runtime loops via `&mut Option<OcertCounters>`
| Body hash verify | Blake2b-256 of body | ✅ | ✅ | Complete | verify_block_body_hash
| Body size verify | Declared vs actual body size | ✅ | ✅ | Complete | validate_block_body_size (upstream WrongBlockBodySizeBBODY)
| Protocol version check | Era/version consistency + MaxMajorProtVer guard | ✅ | ✅ | Complete | validate_block_protocol_version (hard-fork combinator era transitions); MaxMajorProtVer config-wired (default 10, Conway)
| Header size check | Header CBOR ≤ maxBlockHeaderSize | ✅ | ✅ | Complete | `LedgerError::HeaderTooLarge`; `Block::header_cbor_size` set from wire bytes in sync.rs for all Shelley-family eras; checked in `apply_block_validated` (upstream `Cardano.Ledger.Shelley.Rules.Bbody` `bHeaderSize`)
| UTxO rules | UTXO + CERTS + REWARDS | ✅ | ✅ | Complete | Full era-specific UTxO rules with cert/reward processing
| **Density Tiebreaker** |
| Leadership density | Blocks per X slots | ✅ | ✅ | Complete | select_preferred implements full comparePraos VRF tiebreaker; Genesis density is peer-management (network crate)

**Consensus Summary**: 100% feature complete. Praos chain selection, VRF tiebreaker, issuer stake verification, and Genesis density primitive (`crates/consensus/src/genesis_density.rs`, Slice GD commit `682dfa8`) all implemented. Body hash optimization is a non-blocking future refinement.

---

### NETWORK SUBSYSTEM

| Feature | Scope | Haskell | Rust | Status | Notes |
|---------|-------|---------|------|--------|-------|
| **Mini-Protocols** |
| Handshake | Role + version negotiation | ✅ | ✅ | Complete | HandshakeMessage state machine
| ChainSync (client) | Block sync backbone | ✅ | ✅ | Complete | ChainSyncClient state machine + pipelined Find Intersect
| ChainSync (server) | Responder to sync requests | ✅ | ✅ | Complete | ChainSyncServer dispatches on stored points
| BlockFetch (client) | Block batch download | ✅ | ✅ | Complete | BlockFetchClient with pipelined requests
| BlockFetch (server) | Block batch provider | ✅ | ✅ | Complete | BlockFetchServer from storage
| TxSubmission (client) | TX relay → peer | ✅ | ✅ | Complete | TxSubmissionClient with TxId advertising
| TxSubmission (server) | TX intake from peer | ✅ | ✅ | Complete | TxSubmissionServer with duplicate detection
| KeepAlive | Heartbeat | ✅ | ✅ | Complete | KeepAliveServer/Client with epoch/slot
| PeerSharing | Peer candidate exchange | ✅ | ✅ | Complete | PeerSharingClient/Server with AddressInfo
| **Multiplexing** |
| Protocol switching | Per-protocol state machines | ✅ | ✅ | Complete | Mux dispatch via protocol ID
| Backpressure | SDU queue limits + egress soft limit | ✅ | ✅ | Complete | Per-protocol ingress byte limit (2 MB) + egress soft limit (262 KB) + 30 s bearer read timeout (SDU_READ_TIMEOUT)
| Fair scheduling | Weighted round-robin + priority | ✅ | ✅ | Complete | Per-protocol egress channels with dynamic WeightHandle; hot peers get ChainSync=3/BlockFetch=2
| CBOR reassembly | Multi-SDU message handling | ✅ | ✅ | Complete | MessageChannel with cbor_item_length detection, transparent segmentation/reassembly
| Timeout handling | Protocol-specific timeouts | ✅ | ✅ | Complete | CM responder/time-wait + SDU bearer read timeout + per-protocol recv deadline on both server and client sides: N2N server drivers enforce PROTOCOL_RECV_TIMEOUT (60 s, upstream shortWait); client drivers enforce per-state limits from protocol_limits.rs matching upstream ProtocolTimeLimits (ChainSync ST_INTERSECT 10 s / ST_NEXT_CAN_AWAIT 10 s, BlockFetch BF_BUSY/BF_STREAMING 60 s, KeepAlive CLIENT 97 s, PeerSharing ST_BUSY 60 s, TxSubmission ST_IDLE waitForever)
| **Peer Management** |
| Peer sources | LocalRoot/PublicRoot/PeerShare | ✅ | ✅ | Complete | PeerSource enum + provider layer
| DNS resolution | Dynamic root-set updates | ✅ | ✅ | Complete | DnsRootPeerProvider with TTL clamping
| Ledger peers | Registered pool relays | ✅ | ✅ | Complete | LedgerPeerProvider + snapshot normalization
| Peer registry | Source + status tracking | ✅ | ✅ | Complete | PeerRegistry with Cold/Warm/Hot states
| **Governor** |
| Outbound targets | HotValency/WarmValency | ✅ | ✅ | Complete | GovernorTargets + sanePeerSelectionTargets validation
| Promotion logic | Cold → Warm → Hot | ✅ | ✅ | Complete | Tepid deprioritization, local-root-first, big-ledger disjoint
| Demotion logic | Hot → Warm → Cold | ✅ | ✅ | Complete | Non-local-root first, big-ledger disjoint, in-flight tracking
| Churn | Peer replacement rate | ✅ | ✅ | Complete | Two-phase churn cycle (DecreasedActive → DecreasedEstablished → Idle)
| Anti-churn | Stable peer retention | ✅ | ✅ | Complete | Tepid flag + failure backoff + churn_decrease(v) = max(0, v - max(1, v/5))
| Bootstrap-sensitive | Trustable-only mode | ✅ | ✅ | Complete | PeerSelectionMode::Sensitive with trustable filtering
| In-flight tracking | Duplicate action prevention | ✅ | ✅ | Complete | Promotions + demotions tracked; filter_backed_off filters both
| Peer sharing requests | Gossip-based discovery | ✅ | ✅ | Complete | ShareRequest action + budget tracking (inProgressPeerShareReqs)
| Failure backoff | Exponential retry delay | ✅ | ✅ | Complete | Time-based decay + max_connection_retries forget
| Local-root handling | Static hotValency targets | ✅ | ✅ | Complete | LocalRootTargets enum + governor integration
| **Connection Management** |
| Inbound accept | Role negotiation | ✅ | ✅ | Complete | Inbound handshake in acceptor role
| Outbound connect | Peer candidates | ✅ | ✅ | Complete | Outbound connection flow
| Connection pooling | Max connection limits | ✅ | ✅ | Complete | AcceptedConnectionsLimit (512 hard/384 soft/5s delay), prune_for_inbound eviction, inbound duplex reuse for outbound
| Graceful shutdown | In-flight message draining | ✅ | ✅ | Complete | Outbound CM-drain with ControlMessage::Terminate + bounded timeout; inbound JoinSet drain

**Network Summary**: 100% feature complete. All mini-protocols, mux with weighted fair scheduling, peer governor (including `HotPeerScheduling` per-mini-protocol weight surface from Slice D, commit `b1ec7cd`, and density-biased demotion through `PeerMetrics.density`), connection manager with rate limiting and pruning, graceful shutdown, and per-protocol recv deadlines (server-side PROTOCOL_RECV_TIMEOUT 60 s + client-side per-state ProtocolTimeLimits from protocol_limits.rs) all implemented. Genesis-density primitive lives in `crates/consensus/` (Slice GD); the ChainSync `observe_header(slot)` runtime hook (commit `36bdbef`) feeds per-peer `DensityWindow` instances consumed by the governor.

---

### MEMPOOL SUBSYSTEM

| Feature | Scope | Haskell | Rust | Status | Notes |
|---------|-------|---------|------|--------|-------|
| **Queue Management** |
| Fee ordering | By effective fee (size + exec) | ✅ | ✅ | Complete | FeeOrderedQueue with TxId index
| Duplicate detection | By TxId | ✅ | ✅ | Complete | HashSet-based dedup
| Size limits | Max MB + TX count | ✅ | ✅ | Complete | Capacity enforcement
| TTL tracking | Age-based expiry | ✅ | ✅ | Complete | TTL-aware purge_expired
| Eviction policy | Fee-based + age-based | ✅ | ✅ | Complete | Evict low-fee/old TXs on overflow
| **TX Validation** |
| Syntax check | CBOR format + fields | ✅ | ✅ | Complete | ShelleyCompatibleSubmittedTx decode
| Duplicate reject | Already in mempool | ✅ | ✅ | Complete | Pre-insertion check
| Fee check | ≥ minimum linear fee | ✅ | ✅ | Complete | Enforce min_fee
| UTxO check | Inputs available | ✅ | ✅ | Complete | Full UTxO existence check in apply_submitted_tx
| Collateral check | Collateral > fee | ✅ | ✅ | Complete | validate_collateral via apply_submitted_tx
| Script budget | Enough ExUnits | ✅ | ✅ | Complete | validate_tx_ex_units via apply_submitted_tx
| **Block Application** |
| TX confirmation | Remove on block | ✅ | ✅ | Complete | evict_confirmed_from_mempool
| Snapshot creation | TXs for block producer | ✅ | ✅ | Complete | Mempool iterator support
| Epoch revalidation | Re-check params on epoch | ✅ | ✅ | Complete | purge_invalid_for_params; fee/size/ExUnits re-validated at every epoch boundary; wired into node/src/runtime.rs at both verified sync paths; reference: `Ouroboros.Consensus.Mempool.Impl.Update` — `syncWithLedger`
| **Relay Semantics** |
| TxId advertising | Before full TX | ✅ | ✅ | Complete | TxSubmissionClient announces IDs first
| TX request flow | Solicit after ID seen | ✅ | ✅ | Complete | TxSubmissionServer responds to requests
| Duplicate filtering | Peer + global | ✅ | ✅ | Complete | SharedTxState cross-peer dedup: filter_advertised/mark_in_flight/mark_received per-peer + global known ring (16 384)

**Mempool Summary**: ~98% feature complete. Collateral, ExUnits, conflict detection, and cross-peer TxId dedup all wired. SharedTxState integrated into run_txsubmission_server and run_inbound_accept_loop.
**Mempool Summary**: ~99% feature complete. Collateral, ExUnits, conflict detection, cross-peer TxId dedup, and epoch revalidation all wired. SharedTxState integrated into run_txsubmission_server and run_inbound_accept_loop. Epoch reconciliation (`purge_invalid_for_params`) sweeps all mempool entries against new protocol parameters at every epoch boundary.

---

### STORAGE SUBSYSTEM

| Feature | Scope | Haskell | Rust | Status | Notes |
|---------|-------|---------|------|--------|-------|
| **Block Store** |
| Immutable store | Blocks >3k slots old | ✅ | ✅ | Complete | FileImmutable with CBOR persistence (legacy JSON read compatibility)
| Volatile store | Recent blocks <3k slots | ✅ | ✅ | Complete | FileVolatile with rollback
| Atomicity | All-or-nothing writes | ✅ | ✅ | Complete | Atomic write-to-temp + fsync + rename in file-backed stores\n| Fsync durability | Data reaches durable storage | ✅ | ✅ | Complete | sync_all() on temp file before rename + directory sync after rename in all three stores
| **Ledger State** |
| Snapshot storage | Checkpoint every N blocks | ✅ | ✅ | Complete | FileLedgerStore raw-byte snapshots (`.dat`) for typed CBOR checkpoints
| State recovery | From last checkpoint | ✅ | ✅ | Complete | Open + replay pattern
| Rollback support | Revert to prior checkpoints | ✅ | ✅ | Complete | Checkpoint time-travel
| **Garbage Collection** |
| Immutable trimming | Delete blocks >retention | ✅ | ✅ | Complete | trim_before_slot + ChainDb::gc_immutable_before_slot
| Volatile compaction | GC + orphan cleanup | ✅ | ✅ | Complete | garbage_collect(slot) + compact() + gc_volatile_before_slot
| Checkpoint pruning | Keep recent snapshots | ✅ | ✅ | Complete | retain_latest + persist_ledger_checkpoint
| **Index & Lookup** |
| Point → block | By block hash | ✅ | ✅ | Complete | Storage scanning on open
| Slot → block | By slot number | ✅ | ✅ | Complete | get_block_by_slot with binary search (FileImmutable)
| **Recovery & Crash Handling** |
| Dirty ledger detection | Incomplete state write | ✅ | ✅ | Complete | Active recovery on stale dirty.flag: removes leftover .tmp files, clears sentinel after successful scan; all three file stores (FileVolatile, FileImmutable, FileLedgerStore)
| Corruption resilience | Skip/repair bad blocks | ✅ | ✅ | Complete | FileImmutable + FileVolatile + FileLedgerStore skip corrupted files on open
| Dirty sentinel | Unclean-shutdown detection | ✅ | ✅ | Complete | dirty.flag written before every mutation, removed on success; open() actively cleans .tmp files + clears sentinel after successful recovery scan in all three file stores

**Storage Summary**: ~98% feature complete. GC, slot-based indexing, corruption-tolerant open, checkpoint pruning, dirty-flag crash detection, active crash recovery (tmp cleanup + sentinel clear after successful recovery scan), and fsync durability (sync_all on writes + directory sync after rename) all complete.

---

### CLI & CONFIGURATION

| Feature | Scope | Haskell | Rust | Status | Notes |
|---------|-------|---------|------|--------|-------|
| **Configuration** |
| YAML parsing | Config file format | ✅ | ✅ | Complete | `load_effective_config` accepts JSON first with YAML fallback for equivalent `NodeConfigFile` shape
| Environment overrides | CLI flag precedence | ✅ | ✅ | Complete | clap-based override model
| Topology file loading | `--topology` / `TopologyFilePath` | ✅ | ✅ | Complete | `load_topology_file()` reads upstream P2P JSON format; `apply_topology_to_config()` overrides inline topology; CLI flag takes priority over config key
| Database path override | `--database-path` | ✅ | ✅ | Complete | CLI flag overrides `storage_dir` on `run`, `validate-config`, `status`
| Port / host-addr | `--port` / `--host-addr` | ✅ | ✅ | Complete | CLI flags override listen address on `run`
| Genesis loading | ShelleyGenesis + AlonzoGenesis | ✅ | ✅ | Complete | load_genesis_protocol_params
| BP credential paths | ShelleyKesKey/VrfKey/OpCert/IssuerVkey | ✅ | ✅ | Complete | `--shelley-kes-key`, `--shelley-vrf-key`, `--shelley-operational-certificate`, `--shelley-operational-certificate-issuer-vkey` CLI flags + config file keys; text envelope parsing (VRF/KES/OpCert/issuer key) via `load_block_producer_credentials()` with OpCert-vs-issuer signature check
| **Subcommands** |
| run | Sync + validate | ✅ | ✅ | Complete | Main sync loop wired
| validate-config | Verify config file | ✅ | ✅ | Complete | Basic validation
| status | Tip + epoch info | ✅ | ✅ | Complete | Status query framework
| query | LocalStateQuery wrapper | ✅ | ✅ | Complete | run_query: Unix socket → LocalStateQueryClient, 18 query types, JSON output
| submit-tx | LocalTxSubmission wrapper | ✅ | ✅ | Complete | run_submit_tx: Unix socket → LocalTxSubmissionClient, JSON accept/reject result
| **Query API (LocalStateQuery)** |
| CurrentEra | Active era | ✅ | ✅ | Complete | BasicLocalQueryDispatcher tag 0
| ChainTip | Best block info | ✅ | ✅ | Complete | Tag 1
| CurrentEpoch | Epoch number | ✅ | ✅ | Complete | Tag 2
| ProtocolParameters | Active params | ✅ | ✅ | Complete | Tag 3
| UTxOByAddress | Address UTxO lookup | ✅ | ✅ | Complete | Tag 4
| StakeDistribution | Per-pool stake | ✅ | ✅ | Complete | Tag 5
| RewardBalance | Account rewards | ✅ | ✅ | Complete | Tag 6
| TreasuryAndReserves | Governance pots | ✅ | ✅ | Complete | Tag 7
| GetConstitution | Enacted constitution | ✅ | ✅ | Complete | Tag 8 — from EnactState
| GetGovState | Pending governance proposals | ✅ | ✅ | Complete | Tag 9 — GovActionId → GovernanceActionState map
| GetDRepState | DRep registrations | ✅ | ✅ | Complete | Tag 10 — full DrepState
| GetCommitteeMembersState | Committee member info | ✅ | ✅ | Complete | Tag 11 — full CommitteeState
| GetStakePoolParams | Pool params by hash | ✅ | ✅ | Complete | Tag 12 — pool_hash param, RegisteredPool or null
| GetAccountState | Treasury + reserves + deposits | ✅ | ✅ | Complete | Tag 13 — [treasury, reserves, total_deposits]
| GetUTxOByTxIn | UTxO lookup by TxIn | ✅ | ✅ | Complete | Tag 14 — query UTxO entries by specific transaction inputs
| GetStakePools | All registered pool IDs | ✅ | ✅ | Complete | Tag 15 — returns all registered pool key hashes
| GetFilteredDelegationsAndRewardAccounts | Delegation + rewards by credential | ✅ | ✅ | Complete | Tag 16 — per-credential delegated pool + reward balance
| GetDRepStakeDistr | DRep stake distribution | ✅ | ✅ | Complete | Tag 17 — DRep → total delegated stake map
| **Submission API (LocalTxSubmission)** |
| TX validation | Syntax + fee | ✅ | ✅ | Complete | apply_submitted_tx checks
| TX relay readiness | Mempool admission | ✅ | ✅ | Complete | LocalTxSubmission routes through staged ledger validation (`add_tx_to_shared_mempool` → `apply_submitted_tx`) before `insert_checked`; invalid txs rejected without mutating ledger/mempool
| Feedback | Acceptance or error | ✅ | ✅ | Complete | Display format (human-readable LedgerError messages via #[error]) sent in rejection CBOR; Debug format replaced

**CLI Summary**: ~99% feature complete. CLI `query` and `submit-tx` subcommands are fully wired using NtC LocalStateQuery and LocalTxSubmission client drivers. TX rejection feedback now uses Display format for human-readable LedgerError messages. Config-file loading accepts both JSON and YAML. External topology file loading via `--topology` CLI flag and `TopologyFilePath` config key with upstream P2P JSON format support. `--database-path`, `--port`, `--host-addr` CLI flags for runtime overrides.

---

### CRYPTOGRAPHY & ENCODING

| Feature | Scope | Haskell | Rust | Status | Notes |
|---------|-------|---------|------|--------|-------|
| **Hashing** |
| Blake2b-256 | Standard hash | ✅ | ✅ | Complete | blake2 crate
| Blake2b-224 | Script hashing | ✅ | ✅ | Complete | blake2 crate with output truncation
| SHA-256 | Genesis + VRF | ✅ | ✅ | Complete | sha2 crate
| SHA-512 | VRF output hash | ✅ | ✅ | Complete | sha2 crate
| SHA3-256 | Plutus V2+ builtin | ✅ | ✅ | Complete | sha3 crate
| Keccak-256 | Plutus V3 builtin | ✅ | ✅ | Complete | sha3 crate
| Ripemd-160 | Plutus V3 builtin | ✅ | ✅ | Complete | ripemd crate
| **Signatures** |
| Ed25519 | VKey witness signatures | ✅ | ✅ | Complete | ed25519-dalek with verify_vkey_signatures
| Schnorr/secp256k1 | PlutusV2 builtin | ✅ | ✅ | Complete | k256 crate with schnorr feature
| ECDSA/secp256k1 | PlutusV2 builtin | ✅ | ✅ | Complete | k256 crate with ecdsa feature
| **VRF** |
| Praos VRF proof gen | Slot leader selection | ✅ | ✅ | Complete | check_slot_leadership with VRF prove + leader check
| Praos VRF proof verify | Slot leader validation | ✅ | ✅ | Complete | verify_vrf_output with Ed25519
| **Elliptic Curves** |
| Curve25519 | Ed25519 + KES ops | ✅ | ✅ | Complete | curve25519-dalek
| BLS12-381 | CIP-0381 V3 builtins | ✅ | ✅ | Complete | bls12_381 crate (G1/G2/pairing)
| secp256k1 | Plutus signature ops | ✅ | ✅ | Complete | k256 crate
| **KES (Key Evolving Signatures)** |
| KES signature scheme | Operational cert | ✅ | ✅ | Complete | KES OpCert validation
| KES period validation | Block slot alignment | ✅ | ✅ | Complete | Check slot ∈ [kes_period*x, (kes_period+1)*x)
| KES key evolution | Per-period key rotation | ✅ | ✅ | Complete | evolve_kes_key, forge_block_header with SumKES signing
| **CBOR Codec** |
| Major types | 0-7 encoding | ✅ | ✅ | Complete | CborEncode/CborDecode traits
| Compact constructor tags | 121-127 for PlutusData | ✅ | ✅ | Complete | Constr compact encoding
| General constructor tags | Tag 102 for PlutusData | ✅ | ✅ | Complete | Constr general form
| Map encoding | Integer-keyed + string-keyed | ✅ | ✅ | Complete | cddl-codegen generates codecs
| Bignum encoding | Tags 2-3 for large integers | ✅ | ✅ | Complete | PlutusData Integer support
| Tag-24 double encoding | inline datums/scripts | ✅ | ✅ | Complete | DatumOption::Inline, ScriptRef
| **Bech32 & Base58** |
| Bech32 addresses | Shelley-family | ✅ | ✅ | Complete | Address encoding
| Base58 Byron addresses | Byron-family | ✅ | ✅ | Complete | Byron address envelope
| CRC32 validation | Byron address checksum | ✅ | ✅ | Complete | Address::validate_bytes

**Cryptography Summary**: 100% feature complete. Block producer credential loading (text envelope VRF/KES/OpCert), VRF leader election, KES header signing, KES key evolution, and header forging are implemented in `node/src/block_producer.rs`.

---

### MONITORING & TRACING

| Feature | Scope | Haskell | Rust | Status | Notes |
|---------|-------|---------|------|--------|-------|
| **Trace Events** |
| Structured events | Typed trace messages | ✅ | ✅ | Complete | NodeTracer with namespace/severity dispatch and upstream-style trace objects
| Namespace hierarchy | net., chain., ledger., etc | ✅ | ✅ | Complete | Longest-prefix namespace routing for `TraceOptions` (severity/backends/maxFrequency)
| Epoch boundary events | NEWEPOCH/SNAP/RUPD lifecycle | ✅ | ✅ | Complete | `trace_epoch_boundary_events()` emits 14-field structured events (rewards, pools retired, governance, DReps, treasury)
| Inbound tracing | Inbound accept/reject events | ✅ | ✅ | Complete | `run_inbound_accept_loop` traces session start, rate-limit soft delay, hard-limit rejection with peer/DataFlow context
| Filtering & routing | Selector expressions | ✅ | ✅ | Complete | Severity hierarchy threshold filtering + `maxFrequency` + prefix matching + `TraceDetail` (DMinimal/DNormal/DDetailed/DMaximum) per-namespace detail level; upstream backend strings (EKGBackend, Forwarder, PrometheusSimple, Stdout HumanFormatColoured) all recognised
| **Transports** |
| Stdout | Console output | ✅ | ✅ | Complete | NodeTracer stdout dispatch with human/machine formats
| JSON | Structured output | ✅ | ✅ | Complete | GET /metrics/json endpoint + JSON MetricsSnapshot serialization
| Socket | Remote tracer | ✅ | ✅ | Complete | `Forwarder` backend emits trace events as CBOR to Unix domain socket (`TraceForwarder`) via `trace_option_forwarder.socket_path`; compatible with upstream cardano-tracer
| **Metrics** |
| EKG integration | Live metrics endpoint | ✅ | ✅ | Complete | 35+ atomic counters/gauges in NodeMetrics
| Prometheus export | /metrics endpoint | ✅ | ✅ | Complete | MetricsSnapshot::to_prometheus_text() with Prometheus text exposition
| Key metrics | Block height, peers, mempool size | ✅ | ✅ | Complete | blocks_synced, current_slot, block_no, peers (6 variants), checkpoint, rollbacks, uptime_ms, mempool tx/bytes, CM counters, inbound accept/reject
| Health endpoint | Orchestrator liveness | ✅ | ✅ | Complete | GET /health with status, uptime, blocks_synced, current_slot
| Mempool metrics | Mempool tx count & bytes | ✅ | ✅ | Complete | mempool_tx_count, mempool_bytes gauges updated each governor tick; mempool_tx_added, mempool_tx_rejected counters
| Connection manager counters | Full/duplex/uni/in/out | ✅ | ✅ | Complete | ConnectionManagerState::counters() folds actual per-connection state; exported to Prometheus each governor tick
| Inbound counters | Accept/reject totals | ✅ | ✅ | Complete | inbound_connections_accepted, inbound_connections_rejected counters
| **Profiling** |
| CPU profiling | Bottleneck identification | ✅ | ⏸️ | Not Started | Profiling integration
| Memory profiling | Heap analysis | ✅ | ⏸️ | Not Started | Allocation tracking
| Latency tracing | Operation timing | ✅ | ⏸️ | Not Started | Latency measurement

**Monitoring Summary**: ~98% feature complete. NodeMetrics (35+ counters/gauges), Prometheus/JSON/health endpoints, mempool + CM + inbound counters, epoch boundary + inbound session tracing, ANSI-coloured stdout backend (`Stdout HumanFormatColoured`), per-namespace `TraceDetail` levels (DMinimal/DNormal/DDetailed/DMaximum), upstream backend string recognition (EKGBackend/Forwarder/PrometheusSimple), `Forwarder` CBOR socket transport (cardano-tracer compatible), and NodeTracer with severity-threshold + namespace-prefix filtering all implemented. Remaining: profiling.

---

## Subsystem-by-Subsystem Analysis

### 1. CRYPTOGRAPHY & ENCODING (`crates/crypto`)

**Current State**: ✅ Complete — validation and block production

**What's Done**:
- Ed25519 witness verification via `verify_vkey_signatures()`
- VRF proof verification with Praos leader-value check
- VRF proof generation for slot leader election (`VrfSecretKey::prove()`)
- Blake2b-256/224 hashing for blocks, TXs, and scripts
- SHA-256/512 for VRF and genesis
- SHA3-256, Keccak-256, RIPEMD-160 for Plutus builtins
- BLS12-381 for CIP-0381 V3 builtins (G1/G2 ops, pairing, hash-to-curve)
- secp256k1 ECDSA + Schnorr for PlutusV2
- Curve25519 for KES
- SumKES key generation, signing, evolution (`gen_sum_kes_signing_key`, `sign_sum_kes`, `update_sum_kes`)
- Full CBOR roundtrip parity testing

**What's Missing**:
- ℹ️ Performance optimization for large batches
- ℹ️ Hardware acceleration detection

**Parity Status**: **100% complete** — All validation and block-production cryptography is implemented and tested. Block producer credential loading, VRF leader election, KES header signing, and KES key evolution are in `node/src/block_producer.rs`.

---

### 2. LEDGER STATE MANAGEMENT (`crates/ledger`)

**Current State**: ⚠️ Mostly complete, with Plutus edge cases pending

**What's Done**:
- **Multi-era types** (Byron → Conway) with CBOR codecs
- **UTxO model** with coin preservation and multi-asset tracking
- **State transitions** via `apply_block()` dispatch
- **Era types** with all certificate variants (19 DCert types)
- **Transaction validation**: syntax, fees, witnesses, native scripts
- **Epoch boundary**: stake snapshots, pool retirement, DRep inactivity, proposal expiry, dormant epoch counter
- **Governance**: proposal storage, vote accumulation, enacted-root validation, enactment
- **Conway deposit/refund parity**: certificate deposits (DELEG/GOVCERT), proposal deposits in value preservation, exact-drain withdrawals
- **Conway dormant epoch tracking**: `numDormantEpochs` increment/reset, per-tx DRep expiry bump on proposals, dormant-adjusted DRep activity
- **PlutusData**: Full AST with compact/general constructor encoding
- **Addresses**: Base/Enterprise/Pointer/Reward/Byron with validation

**What's Missing**:
- ✅ **Collateral validation** (VKey-locked enforcement, mandatory when redeemers, Babbage return/total checks)
- ✅ **Exact redeemer set checks** (missing + extra Plutus redeemer pointers now enforced in shared collector; Phase-1 UTXOW unconditional `validate_no_extra_redeemers` call in all 3 block-apply paths — Alonzo/Babbage/Conway — before `is_valid` dispatching, matching upstream `hasExactSetOfRedeemers` in `alonzoUtxowTransition`)
- ✅ **Reward calculation** (upstream RUPD→SNAP ordering, delta_reserves-only reserves accounting, fee pot not subtracted from reserves)
- ✅ **Plutus script execution** (CEK machine framework wired; Phase-2 validation in block + submitted-tx paths)
- ✅ **Ratification tally** (voting functions complete incl. AlwaysNoConfidence auto-yes; epoch-boundary ratification+enactment+deposit lifecycle)
- ✅ **Deposit refunds** (enacted+expired+lineage-pruned deposits refunded via returnProposalDeposits; unclaimed→treasury)
- ✅ **Lineage subtree pruning** (proposalsApplyEnactment: remove_lineage_conflicting_proposals with purpose-root chain validation)
- ✅ **Treasury value check ordering** (`validate_conway_current_treasury_value` now Phase-1 in submitted-tx path, matching block-apply and upstream UTXO rule layering)
- ✅ **Submitted-tx reference-input validation** (cleaned duplicate calls in Babbage/Conway — early direct-UTxO check retained, redundant staged-clone check removed)

**Parity Status**: **~96% complete** — All era types, core rules, Conway governance lifecycle, deposit/refund validation, dormant epoch tracking, exact redeemer-set checks, and Phase-2 Plutus validation (block + submitted-tx). Remaining work on CEK builtin coverage and edge cases.

---

### 3. CONSENSUS & CHAIN SELECTION (`crates/consensus`)

**Current State**: ✅ Mostly complete, with leader election density tiebreaker pending

**What's Done**:
- **Praos validation** with VRF leader-value check
- **OpCert validation** with sequence number enforcement
- **Chain state tracking** with volatility + stability window
- **Rollback depth** enforcement (max 3k slots)
- **Nonce evolution** via UPDN + TICKN rules
- **Header format** parsing (all 7 eras)
- **Slot continuity** checks
- **Block numbering** validation

**What's Missing**:
- ✅ **Density tiebreaker** (VRF tiebreaker in select_preferred; Genesis density is network-layer)
- ✅ **Issuer validation** (verify_block_vrf_with_stake with stake distribution lookup)
- ⏸️ **Body hash optimization** (currently full body hash every block)

**Parity Status**: **~98% complete** — All critical validations present including VRF leader check and Praos VRF tiebreaker. Genesis density is a future network-layer milestone.

---

### 4. NETWORK & PEER MANAGEMENT (`crates/network`)

**Current State**: ✅ Protocols and governor complete, connection lifecycle hardened

**What's Done**:
- **5 mini-protocols** fully wired (ChainSync, BlockFetch, TxSubmission, KeepAlive, PeerSharing)
- **Mux** with weighted round-robin fair scheduling + dynamic `WeightHandle` per protocol
- **SDU segmentation/reassembly** via `MessageChannel` with CBOR-aware boundary detection
- **Backpressure**: per-protocol ingress byte limits (2 MB) + egress soft limit (262 KB)
- **Bearer timeout**: 30 s SDU read timeout (`SDU_READ_TIMEOUT`) in demux_loop
- **Handshake** with role negotiation
- **Typed client/server drivers** for all protocols
- **Per-protocol timeouts (server)**: PROTOCOL_RECV_TIMEOUT (60 s) on all 5 N2N server drivers
- **Per-protocol timeouts (client)**: per-state ProtocolTimeLimits from protocol_limits.rs − ChainSync (ST_INTERSECT 10 s, ST_NEXT_CAN_AWAIT 10 s, ST_NEXT_MUST_REPLY_TRUSTABLE waitForever), BlockFetch (BF_BUSY 60 s, BF_STREAMING 60 s), KeepAlive (CLIENT 97 s), PeerSharing (ST_BUSY 60 s), TxSubmission (ST_IDLE waitForever)
- **Root providers**: local, bootstrap, public, DNS-backed with TTL clamping
- **Peer registry**: Cold/Warm/Hot states + 6 peer sources
- **Governor framework**: targets, promotions/demotions, churn, bootstrap-sensitive, tepid, backoff
- **Ledger peer provider**: normalization + refresh orchestration
- **Local-root handling**: hotValency + warmValency targets
- **Connection manager**: AcceptedConnectionsLimit (512/384/5s), prune_for_inbound, duplex reuse
- **Inbound governor**: state tracking, matured duplex peers, inactivity timeout
- **Graceful shutdown**: outbound ControlMessage::Terminate drain + bounded timeout; inbound JoinSet drain
- **Rate limiting**: RateLimitDecision applied in accept loop
- **Hot-peer scheduling**: ChainSync weight 3, BlockFetch weight 2 on promote; reset on demote

**What's Missing**:
- _(no remaining gaps)_ — Genesis density tracking primitive landed in Slice GD (`crates/consensus/src/genesis_density.rs`, commit `682dfa8`); ChainSync observation hook + governor-side density-biased demotion are wired in (commit `36bdbef`).

**Parity Status**: **100% feature-complete** — All protocols, mux, governor, connection lifecycle, per-protocol server + client timeouts, peer management, and genesis-density primitive fully implemented and tested.

---

### 5. MEMPOOL (`crates/mempool`)

**Current State**: ✅ Functional, with script budget checking pending

**What's Done**:
- **Fee-ordered queue** by effective fee
- **Duplicate detection** by TxId
- **Capacity enforcement** (size + count limits)
- **TCopytL tracking** and expiry
- **Block application eviction** via `evict_confirmed_from_mempool`
- **Snapshot support** for block producers
- **Relay semantics**: TxId advertising before full TX
- **Collateral validation** via validate_alonzo_plus_tx → validate_collateral during apply_submitted_tx
- **Script budget** via validate_tx_ex_units during apply_submitted_tx
- **Transaction conflict detection** via claimed_inputs HashMap in FeeOrderedQueue

**What's Missing**:
- All critical mempool features implemented

**Parity Status**: **~98% complete** — Core queue, conflict detection, collateral, ExUnits, and cross-peer TxId dedup all wired via SharedTxState.

---

### 6. STORAGE (`crates/storage`)

**Current State**: ✅ Functional with robust crash handling

**What's Done**:
- **Immutable store** for blocks >3k slots old (CBOR persistence, legacy JSON read compat)
- **Volatile store** with rollback on reorg
- **Ledger checkpoints** for state recovery (raw-byte `.dat` snapshots)
- **Point lookup** by block hash
- **Slot-based indexing** via `get_block_by_slot` with binary search
- **Atomicity** via tmp+rename writes in all three file-backed stores
- **Garbage collection**: `trim_before_slot` + `ChainDb::gc_immutable_before_slot`
- **Checkpoint pruning**: `retain_latest` + `persist_ledger_checkpoint`
- **Crash detection**: `dirty.flag` sentinel in all three stores (written before mutations, removed on success)
- **Corruption resilience**: skip/repair bad blocks on open

**What's Missing**:
- Ongoing endurance validation against long-running mainnet-style churn and restarts

**Parity Status**: **~97% complete** — Core functionality, GC (immutable + volatile), compaction, crash detection, corruption resilience, and WAL-backed multi-step delete recovery are implemented.

---

### 7. CLI & CONFIGURATION (`node/`)

**Current State**: ✅ Functional with query and submit-tx subcommands complete

**What's Done**:
- **Configuration loading** (JSON + YAML preset support)
- **CLI subcommands**: run, validate-config, status, default-config, query, submit-tx
- **`query`**: Unix socket → LocalStateQueryClient, 18 query types (CurrentEra, Tip, Epoch, ProtocolParams, UTxO, StakeDistribution, Rewards, Treasury, Constitution, GovState, DRepState, CommitteeMembersState, StakePoolParams, AccountState, UTxOByTxIn, StakePools, DelegationsAndRewards, DRepStakeDistr), JSON output
- **`submit-tx`**: Unix socket → LocalTxSubmissionClient, hex-encoded TX input, JSON accept/reject result
- **LocalStateQuery** server: BasicLocalQueryDispatcher with 14 tags (including Conway governance: GetConstitution, GetGovState, GetDRepState, GetCommitteeMembersState, GetStakePoolParams, GetAccountState)
- **LocalTxSubmission** server: staged `apply_submitted_tx` before mempool insertion
- **LocalTxMonitor** server: wired into SharedMempool
- **Genesis loading** (ShelleyGenesis, AlonzoGenesis, ConwayGenesis)
- **Network presets** (Mainnet, Preprod, Preview)
- **Tracing config** alignment with upstream

**What's Missing**:
- All critical CLI & configuration features implemented

**Parity Status**: **~92% complete** — All subcommands wired with full NtC client drivers.

---

### 8. MONITORING & TRACING (`node/`)

**Current State**: ✅ Functional with comprehensive metrics, structured tracing, coloured output, and detail-level control

**What's Done**:
- **NodeTracer** with namespace/severity dispatch and upstream-style trace objects
- **Namespace hierarchy**: net., chain., ledger., etc with longest-prefix `TraceOptions` matching and per-namespace `maxFrequency` filtering
- **Structured JSON output**: `GET /metrics/json` endpoint + JSON MetricsSnapshot serialization
- **Stdout transport**: human/machine format dispatch + ANSI-coloured output (`Stdout HumanFormatColoured`)
- **Detail levels**: per-namespace `TraceDetail` (DMinimal/DNormal/DDetailed/DMaximum) matching upstream `DetailLevel`; `NodeTracer::detail_for()` accessor + `trace_runtime_detailed()` entry point for detail-gated events
- **Upstream backend recognition**: `EKGBackend`, `Forwarder`, `PrometheusSimple` all parsed; non-stdout backends silently accepted for forward compatibility
- **NodeMetrics**: 35+ atomic counters/gauges (blocks_synced, current_slot, block_no, peers×6, checkpoint, rollbacks, uptime_ms, mempool_tx_count, mempool_bytes, mempool_tx_added, mempool_tx_rejected, cm_full_duplex_conns, cm_duplex_conns, cm_unidirectional_conns, cm_inbound_conns, cm_outbound_conns, inbound_connections_accepted, inbound_connections_rejected)
- **Prometheus export**: `MetricsSnapshot::to_prometheus_text()` with text exposition at `GET /metrics`
- **Health endpoint**: `GET /health` with status, uptime, blocks_synced, current_slot
- **Epoch boundary tracing**: `trace_epoch_boundary_events()` emits 14-field structured events for each NEWEPOCH transition (new_epoch, rewards, pools_retired, governance, DReps, treasury)
- **Inbound server tracing**: `run_inbound_accept_loop` traces session start, rate-limit soft delay, hard-limit rejection with peer/DataFlow/PeerSharing context
- **Mempool gauges**: mempool tx count and bytes updated from `SharedMempool` every governor tick
- **Connection manager counters**: `ConnectionManagerState::counters()` derives accurate full/duplex/uni/in/out counts from actual per-connection state machine entries (upstream `connection_state_to_counters`)
- **Inbound accept/reject counters**: tracked on hard-limit rejection and successful session start

**What's Missing**:
- ⏸️ **Profiling** (hardware CPU/memory metrics)
- ⏸️ **CPU/memory/latency profiling** (performance instrumentation)

**Parity Status**: **~95% complete** — Full operational metrics, Prometheus/JSON/health endpoints, epoch boundary + inbound lifecycle tracing, mempool + CM counters, ANSI-coloured stdout, per-namespace `TraceDetail` levels, and upstream backend string recognition all implemented. Remaining: socket transport, profiling.

---

## Phased Implementation Roadmap

### Phase 1: Ledger Rules Completion (Weeks 1-3)

**Goal**: Close ledger validation gaps to enable testnet sync.

**Tasks**:
1. ✅ **Collateral validation** (`collateral.rs` complete edge cases)
   - Scope: Alonzo+ collateral UTxO sufficiency checks
   - Upstream reference: `Cardano.Ledger.Alonzo.Rules.Utxo`
   - Tests: 5-10 integration tests for edge cases

2. ✅ **Reward calculation** (upstream RUPD→SNAP ordering + reserves accounting)
   - Scope: Per-pool + per-account reward math; RUPD before SNAP ordering; delta_reserves-only reserves debit
   - Upstream reference: `Cardano.Ledger.Shelley.Rules.NewEpoch` (RUPD→EPOCH), `Ledger.Reward`
   - Tests: Mainnet rewards reconciliation + 5 new epoch boundary tests

3. ✅ **Ratification tally completion** (thresholds + quorum + AlwaysNoConfidence)
   - Scope: Conway voting thresholds per action type; AlwaysNoConfidence auto-yes for NoConfidence and UpdateCommittee-in-no-confidence
   - Upstream reference: `Cardano.Ledger.Conway.Rules.Ratify`
   - Tests: 20+ governance scenarios + 4 new AlwaysNoConfidence tests

**Success Criteria**:
- ✅ apply_block() passes all collateral-related tests
- ✅ Epoch boundary computes correct reward amounts
- ✅ Conway voting thresholds match upstream within testnet

---

### Phase 2: Plutus Execution Integration (Weeks 3-5)

**Goal**: Execute Plutus scripts with CEK machine and budget tracking.

**Tasks**:
1. **CEK machine completion** (`crates/plutus`)
   - Scope: All 36 builtins (V1/V2/V3)
   - Upstream reference: `Language.PlutusCore.Evaluation.Machine.Cek`
   - Tests: 100+ builtin roundtrip tests

2. **Plutus validation wiring** (`node/plutus_eval.rs`)
   - Scope: Script execution in apply_block() path
   - Tests: Script-bearing blocks from testnet

3. **Mempool script budget checking** (`mempool`)
   - Scope: ExUnits validation before admission
   - Tests: Rejection of over-budget TXs

**Success Criteria**:
- ✅ All Plutus V1/V2/V3 scripts execute correctly
- ✅ Budget overage properly rejected
- ✅ Plutus-bearing testnet TX apply without error

---

### Phase 3: Peer Governance & Governor (Weeks 5-7)

**Goal**: Complete peer selection policy for multi-peer stable sync.

**Tasks**:
1. **Promotion scoring** (`governor.rs`)
   - Scope: Score-based peer ranking
   - Upstream reference: `Ouroboros.Network.PeerSelection.GovernorState`
   - Tests: 30+ peer selection scenarios

2. **Demotion triggers** 
   - Scope: Timeout + error-based demotion thresholds
   - Tests: Peer failure recovery

3. **Churn & anti-churn**
   - Scope: Peer replacement + connection stability
   - Tests: Long-lived sync stability

4. **Connection pooling** (`bearer.rs`)
   - Scope: Inbound/outbound connection limits
   - Tests: Pool exhaustion scenarios

5. **Multi-peer concurrent BlockFetch** (`node/src/sync.rs`, `node/src/runtime.rs`,
   `node/src/blockfetch_worker.rs`)
   - **Status**: complete (Slice E foundation `55b66d1` → workers
     `E-Workers` → production spawn `900ce3e` → governor migration
     `1249f7f` → sync-loop dispatch branch `9f87447` → metrics
     `b3a6080`). The runtime now maintains a shared
     `Arc<tokio::sync::RwLock<FetchWorkerPool<MultiEraBlock>>>` mirroring
     upstream `Ouroboros.Network.BlockFetch.ClientRegistry`. On
     warm-to-hot promotion, `OutboundPeerManager::migrate_session_to_worker`
     hands the per-peer `BlockFetchClient` to a dedicated worker task
     (`FetchWorkerHandle::spawn_with_block_fetch_client`); on demotion
     the worker is unregistered and the client dropped. The verified
     sync loop calls `effective_block_fetch_concurrency` per batch and,
     when ≥2, routes through `execute_multi_peer_blockfetch_plan`
     (per-peer `tokio::spawn` + `mpsc`/`oneshot` channels +
     `ReorderBuffer<MultiEraBlock>` for chain-order delivery).
     Tentative-header timing is locked in `dispatch_range_with_tentative`
     (announce before dispatch, clear trap on any chunk failure).
   - Foundation in place (`crates/network/src/blockfetch_pool.rs`):
     `BlockFetchPool` (per-peer in-flight + bytes/blocks accounting),
     `split_range(lower, upper, n)`, `ReorderBuffer<B>` keyed by lower
     slot, `BlockFetchInstrumentation` (Arc<Mutex<>> handle).
   - Slice E primitives (`node/src/sync.rs`):
     `effective_block_fetch_concurrency(max_knob, n_peers)`,
     `BlockFetchAssignment { peer, lower, upper }`,
     `partition_fetch_range_across_peers(lower, upper, peers, max_knob)`,
     `execute_multi_peer_blockfetch_plan{,_inline}`,
     `dispatch_range_with_tentative`,
     `VerifiedSyncServiceConfig.max_concurrent_block_fetch_peers` field
     + `shared_fetch_worker_pool` handle.
   - Worker primitive (`node/src/blockfetch_worker.rs`):
     `FetchWorkerHandle<B>::spawn_with_block_fetch_client`,
     `FetchWorkerPool<B>::{register, unregister, dispatch_plan,
     prune_closed}`. Prometheus metrics
     (`yggdrasil_blockfetch_workers_registered`,
     `yggdrasil_blockfetch_workers_migrated_total`) exposed via
     `NodeMetrics`.
   - Upstream references:
     - [`Ouroboros.Network.BlockFetch.Decision`](https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network/src/Ouroboros/Network/BlockFetch/Decision.hs)
     - [`Ouroboros.Network.BlockFetch.ClientRegistry`](https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network/src/Ouroboros/Network/BlockFetch/ClientRegistry.hs)
   - Operator rehearsal: `MANUAL_TEST_RUNBOOK.md` §6.5 (parallel-fetch
     rehearsal across 6.5a–6.5f) covers the operator wallclock validation;
     the default knob ships at 1 and is operator-flippable to ≥2 once
     sign-off is recorded.

**Success Criteria**:
- ✅ Stable mainnet peer set without thrashing
- ✅ Connection limits enforced
- ✅ Failed peers properly demoted
- ✅ Concurrent BlockFetch from N warm peers with reorder + rebalance (commits `55b66d1` → `b3a6080`)

---

### Phase 4: Storage Robustness (Weeks 7-9)

**Goal**: Crash recovery and garbage collection for long-term stability.

**Tasks**:
1. **Garbage collection policy** (`storage`)
   - Scope: Immutable block trimming + checkpoint pruning
   - Tests: Multi-week retention scenarios

2. **Crash recovery** (`storage`)
   - Scope: Dirty state detection + checkpoint rollback
   - Tests: Simulated crash + recovery

3. **Slot-based indexing** (storage optimization)
   - Scope: Efficient slot → block mapping
   - Tests: Lookup performance on 1M+ blocks

**Success Criteria**:
- ✅ Storage doesn't grow unbounded
- ✅ Crash recovery < 10 seconds
- ✅ Slot lookup < 1ms

---

### Phase 5: Monitoring & Telemetry (Weeks 9-11)

**Goal**: Production-grade tracing, metrics, and observability.

**Tasks**:
1. **Structured logging** (`tracer.rs`)
   - Scope: JSON output + namespace hierarchy
   - Tests: Log filtering + parsing

2. **Metrics collection** 
   - Scope: EKG + Prometheus endpoints
   - Tests: Metrics completeness + accuracy

3. **Trace points** (all subsystems)
   - Scope: 50+ named trace events
   - Tests: Trace event coverage

**Success Criteria**:
- ✅ Full JSON tracing output
- ✅ Prometheus /metrics endpoint stable
- ✅ All key operations traced

---

### Phase 6: Integration & Mainnet Testing (Weeks 11-13)

**Goal**: End-to-end validation and mainnet compatibility.

**Tasks**:
1. **Mainnet sync testing** 
   - Scope: Full blockchain sync from genesis
   - Tests: Mainnet genesis → tip (1500+ epochs)

2. **Testnet stress testing** 
   - Scope: High-throughput TX relay
   - Tests: Sustained 1000 TX/s + mempool eviction

3. **Fork recovery** 
   - Scope: Deep reorg handling
   - Tests: 3k-block rollback scenarios

4. **Interop testing** 
   - Scope: Sync with official Haskell nodes
   - Tests: Identical chain tip + state

**Success Criteria**:
- ✅ Mainnet sync completes without error
- ✅ State matches official node on testnet
- ✅ Can sustain fork recovery

---

## Cross-Subsystem Integration Points

### Data Flow During Block Application

```
ChainSync (network)
  ↓ [Point + Tip]
MultiEraBlock::decode()  (consensus bridge)
  ↓ [Decoded block]
verify_multi_era_block()  (consensus)
  ↓ [Header verified]
apply_block()  (ledger)
  ├─ validate_witnesses()  (crypto)
  ├─ validate_native_scripts()  (ledger)
  ├─ execute_plutus_scripts()  (plutus)
  ├─ update_utxo()  (ledger)
  └─ apply_epoch_boundary()  (ledger)
    ├─ compute_stake_snapshot()  (ledger)
    ├─ compute_epoch_rewards()  (ledger::rewards)
    ├─ execute_governance()  (ledger::governance)
    └─ apply_enactments()  (ledger::governance)
  ↓ [State updated]
apply_to_ledger_state()  (storage)
  ↓ [Checkpoint written]
track_chain_state()  (consensus)
  ↓ [ChainState advanced]
evict_confirmed_from_mempool()  (mempool)
  ↓ [Mempool cleaned]
=> ChainSync server ready for next block
```

### Peer Selection During Sync

```
Runtime Bootstrap
  ├─ Load root topology  (config)
  ├─ Resolve DNS  (network::DnsRootPeerProvider)
  ├─ Load snapshot  (network::LedgerPeerProvider)
  └─ Fetch from DB  (storage)
    ↓
Peer Registry
  ├─ Merge sources  (LocalRoot + PublicRoot + Ledger + Bootstrap)
  └─ Initialize as Cold
    ↓
Governor Loop  (network::governor)
  ├─ Score candidates  (promote logic)
  ├─ Promote to Warm  (if target < warm_count)
  ├─ Promote to Hot  (if target < hot_count)
  └─ Demote Hot  (on timeout/error)
    ↓
Connection Manager  (network::bearer)
  ├─ Outbound connect  (for Hot/Warm)
  ├─ Inbound accept  (if slot available)
  └─ Run protocols  (ChainSync + BlockFetch + TxSubmission)
    ↓
Sync Loop  (node::sync)
  ├─ ChainSync find-intersect  (network)
  ├─ Block fetch batches  (network)
  ├─ Apply to ledger  (ledger)
  └─ Repeat until tip
    ↓
=> Mainnet sync complete
```

### Mempool Lifecycle

```
TX arrives via TxSubmission (network)
  ↓
Syntax check  (deserialize)  (ledger::tx)
  ↓
Duplicate detect  (mempool::FeeOrderedQueue)
  ↓
Fee check  (>=min_fee)  (ledger::fees)
  ↓
UTxO available?  (temporary check against last applied block)  (ledger)
  ↓
Collateral sufficient?  (Alonzo+)  (ledger::collateral)
  ↓
Script budget estimate  (< protocol max)  (ledger::plutus)
  ↓
Insert to queue  (by effective fee)  (mempool)
  ↓
Advertise TxId  (TxSubmission)  (network)
  ↓
Block producer:
  ├─ Take mempool snapshot  (mempool)
  ├─ Build TX list  (ordered by fee)
  └─ Validate final state  (apply_block)
    ├─ Re-check UTxO  (may have changed since mempool insertion)
    ├─ Execute scripts  (plutus::CekPlutusEvaluator)
    └─ Consume inputs  (ledger::apply_block)
  ↓
Block distributed:
  ├─ Evict confirmed TXs  (evict_confirmed_from_mempool)  (mempool)
  ├─ Mempool size shrinks
  └─ New TXs arrive (repeat)
    ↓
TX expires (TTL):
  ├─ purge_expired()  (mempool)
  └─ Slot limit exceeded → remove
    ↓
=> Mempool in steady state
```

---

## Risk Assessment & Mitigation

### High Risk: Plutus Execution Correctness

**Risk**: Script budget mismatch or execution divergence → testnet TX failures

**Mitigation**:
- ✅ Use upstream CEK machine as reference implementation
- ✅ Generate test vectors from official node
- ✅ Cross-check budget calculations
- ✅ Integration test all V1/V2/V3 builtins

**Timeline**: Phase 2 (weeks 3-5)  
**Owner**: `crates/plutus` maintenance

---

### High Risk: Governance State Consistency

**Risk**: Vote tally mismatch or ratification divergence → fork on governance action

**Mitigation**:
- ✅ Trace upstream ratification logic exactly
- ✅ Implement all 7 `GovAction` types identically
- ✅ Test against mainnet governance history
- ✅ Verify genesis-derived EnactState

**Timeline**: Phase 1 (weeks 1-3)  
**Owner**: `crates/ledger` maintenance

---

### Medium Risk: Storage Crash Recovery

**Risk**: Incomplete checkpoint or orphaned volatile blocks → sync restart needed

**Mitigation**:
- ✅ Atomic ledger checkpoints (write manifest last)
- ✅ Immutable block verification on open
- ✅ Checkpoint versioning for upgrades
- ✅ Dual redundancy option (TBD)

**Timeline**: Phase 4 (weeks 7-9)  
**Owner**: `crates/storage` maintenance

---

### Medium Risk: Peer Selection Thrashing

**Risk**: Unstable peer set → constant reconnects → poor sync performance

**Mitigation**:
- ✅ Implement upstream governor scoring
- ✅ Anti-churn + successful-peer persistence
- ✅ Gradual demotion thresholds
- ✅ Load-test with 50+ peers

**Timeline**: Phase 3 (weeks 5-7)  
**Owner**: `crates/network` maintenance

---

### Medium Risk: Bytes Parity on CBOR Round-Trip

**Risk**: Serialized blocks don't match Haskell → relay rejection

**Mitigation**:
- ✅ Full CBOR roundtrip golden tests (already passing)
- ✅ Bytes-level comparison vs. mainnet blocks
- ✅ Era-specific encode edge cases
- ✅ Canonical CBOR ordering

**Timeline**: Ongoing in all phases  
**Owner**: `crates/ledger` + `crates/cddl-codegen`

---

### Low Risk: CLI Subcommand Gaps

**Risk**: Missing `query` or `submit-tx` subcommand → end-user friction

**Mitigation**:
- ✅ Implement wrappers after core APIs stable
- ✅ Match Haskell node CLI signatures
- ✅ Test with existing cardano-cli scripts

**Timeline**: Phase 1 (weeks 1-3)  
**Owner**: `node/` CLI work

---

## Success Criteria

### Validation Milestones

**Milestone 1: Ledger Rules Complete** (end of Phase 1)
- ✅ `cargo test-all -- --list` currently discovers 4210 tests
- ✅ Collateral validation handles 100% of Alonzo+ blocks
- ✅ Reward calculation matches mainnet within 1 lovelace
- ✅ Governance proposals ratify correctly for 50+ actions

**Milestone 2: Plutus Execution Live** (end of Phase 2)
- ✅ All 36 Plutus builtins execute correctly
- ✅ Script budget rejection matches Haskell node
- ✅ 1000+ mainnet Alonzo+ blocks apply without error

**Milestone 3: Multi-Peer Stable** (end of Phase 3)
- ✅ 50+ peer connections maintain without churn
- ✅ Blocks pulled from multiple peers simultaneously
- ✅ 3k-block rollback recovers without restart

**Milestone 4: Storage Hardened** (end of Phase 4)
- ✅ Simulated crashes recover cleanly
- ✅ Garbage collection doesn't corrupt state
- ✅ Immutable block trim > 1 year old blocks

**Milestone 5: Observability Complete** (end of Phase 5)
- ✅ Full JSON tracing to stdout
- ✅ Prometheus /metrics with 20+ key metrics
- ✅ EKG /debug endpoint live

**Milestone 6: Mainnet Sync** (end of Phase 6)
- ✅ Sync from mainnet genesis to current tip
- ✅ Final state matches official Haskell node
- ✅ Fork recovery works for deep reorgs
- ✅ Can sustain 1000+ TX/s in mempool

### Regression Prevention

**Continuous**:
- ✅ `cargo test-all` runs on every commit (currently 4210 discovered tests)
- ✅ `cargo lint` is clippy `-D warnings` clean across all crates and targets
- ✅ CBOR roundtrip parity tests golden comparisons

**Weekly**:
- ✅ Testnet mainnet sync from genesis
- ✅ Upstream compatibility check (blocks from known testnet chains)

**Monthly**:
- ✅ Formal spec review (check CDDL alignment)
- ✅ Performance baseline (block apply time, mempool latency)

---

## Appendix: Upstream Source References

### Ledger Rules
- Formal spec (Agda): https://github.com/IntersectMBO/formal-ledger-specifications
- Byron spec: https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/byron
- Shelley spec: https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/shelley
- Alonzo spec: https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/alonzo
- Babbage spec: https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/babbage
- Conway spec: https://github.com/IntersectMBO/cardano-ledger/tree/master/eras/conway

### Consensus & Cryptography
- Ouroboros Praos paper: https://eprint.iacr.org/2017/573.pdf
- Chain selection: https://github.com/IntersectMBO/ouroboros-consensus/blob/main/docs
- VRF verification: https://github.com/IntersectMBO/cardano-base/tree/master/cardano-crypto-praos
- BLS12-381 (CIP-0381): https://github.com/cardano-foundation/CIPs/pull/226

### Network & Peer Management
- Network spec: https://ouroboros-network.cardano.intersectmbo.org/pdfs/network-spec
- Mini-protocol impl: https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network/src/Ouroboros/Network/Protocol
- Peer selection: https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network/src/Ouroboros/Network/PeerSelection

### Configuration & CLI
- Config spec: https://github.com/IntersectMBO/cardano-node/blob/master/cardano-node/docs/configuration.md
- LocalStateQuery: https://cardano-docs.readthedocs.io/en/latest/explore-cardano/cardano-node/local-state-query-protocol.html
- Genesis format: https://github.com/cardano-foundation/developer-portal/tree/staging/docs/_build

---

**Document prepared by**: Research & Planning Agent  
**Target Review Date**: Week of March 31, 2026  
**Expected Final Delivery**: Mid-June 2026 (13-week roadmap)
