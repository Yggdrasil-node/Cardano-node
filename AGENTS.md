## Root agent for the Yggdrasil Rust Cardano node workspace

## Agent Instructions
- You are implementing a pure typesafe Rust Cardano node with no FFI dependencies, aiming for feature parity with the official Haskell node while maintaining strict alignment with upstream behavior, naming, and design patterns.
- You are focused on deterministic parsing, byte-accurate serialization, and reproducible generated artifacts. 
- You are researching the official [IntersectMBO github repositories](https://github.com/orgs/IntersectMBO/repositories/) for guidance on design and behavior decisions, and you are documenting your implementation work with reference to the official node and upstream sources.
- You are maintaining a clear separation between different subsystems in the workspace and favoring incremental milestones that compile and test cleanly over speculative completeness. 
- You are writing typesafe Rust code with proper Rustdocs for public APIs when behavior is non-obvious. You are keeping all `AGENTS.md` files up to date with actionable guidance for future implementation work in each area of the codebase.

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
- `crates/cddl-codegen` owns code generation from pinned specifications.
- `crates/ledger` owns ledger state transitions and era modeling.
- `crates/storage` owns durable storage and snapshot interfaces.
- `crates/consensus` owns chain selection, leader election, and rollback rules.
- `crates/mempool` owns transaction intake and ordering.
- `crates/network` owns multiplexing, mini-protocols, and peer management.
- `node/` owns orchestration, CLI, and runtime integration.
- `specs/upstream-test-vectors` officially test-vectors from the `IntersectMBO` repositories.

## Scope
- This root file defines workspace-wide defaults for naming, upstream parity expectations, and cross-crate boundaries.
- Subdirectory `AGENTS.md` files override this file for local implementation details and should stay concise and operational.

## Official Upstream References *"Always research references and add or update links as needed"*

### Cryptography (`crates/crypto`)
- [Crypto abstractions (hashing, signatures, VRF, KES)](https://github.com/IntersectMBO/cardano-base/tree/master/cardano-crypto-class)
- [Praos VRF and KES implementations](https://github.com/IntersectMBO/cardano-base/tree/master/cardano-crypto-praos)
- [Peras-era crypto extensions](https://github.com/IntersectMBO/cardano-base/tree/master/cardano-crypto-peras)

### CDDL Code Generation (`crates/cddl-codegen`)
- [Per-era CDDL schemas (Byron through Conway)](https://github.com/IntersectMBO/cardano-ledger/tree/master/eras)
- [Binary serialization library](https://github.com/IntersectMBO/cardano-ledger/tree/master/libs/cardano-ledger-binary)
- [Ledger support libraries](https://github.com/IntersectMBO/cardano-ledger/tree/master/libs)

### Ledger (`crates/ledger`)
- [Ledger repository (eras, libs, formal specs)](https://github.com/IntersectMBO/cardano-ledger)
- [Per-era rule implementations](https://github.com/IntersectMBO/cardano-ledger/tree/master/eras) (each era has `impl/`, `formal-spec/`, and `cddl/` subdirectories)
- [Formal ledger specifications (Agda)](https://github.com/IntersectMBO/formal-ledger-specifications)
- [Published formal spec site](https://intersectmbo.github.io/formal-ledger-specifications/site)

### Storage (`crates/storage`)
- [ChainDB, ImmutableDB, VolatileDB, LedgerDB](https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Storage)
- [Consensus storage documentation and tech reports](https://github.com/IntersectMBO/ouroboros-consensus/tree/main/docs)

### Consensus (`crates/consensus`)
- [Core consensus protocol modules](https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Protocol)
- [Cardano-specific consensus integration (Praos, TPraos)](https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus-protocol/src/Ouroboros/Consensus/Protocol)
- [Formal consensus Agda specification](https://github.com/IntersectMBO/ouroboros-consensus/tree/main/docs/agda-spec)
- [Consensus tech report](https://ouroboros-consensus.cardano.intersectmbo.org/pdfs/report.pdf)

### Mempool (`crates/mempool`)
- [Consensus Mempool module (API, TxSeq, Capacity, Init)](https://github.com/IntersectMBO/ouroboros-consensus/tree/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Mempool)
- [Transaction submission API](https://github.com/IntersectMBO/cardano-node/tree/master/cardano-submit-api)

### Network (`crates/network`)
- [Networking repository root](https://github.com/IntersectMBO/ouroboros-network)
- [Multiplexer implementation](https://github.com/IntersectMBO/ouroboros-network/tree/main/network-mux)
- [Framework and handshake layer](https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network-framework)
- [Mini-protocol implementations (ChainSync, BlockFetch, TxSubmission, KeepAlive, PeerSharing)](https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network-protocols)
- [Outbound governor and peer selection](https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network)
- [Shelley networking spec PDF](https://ouroboros-network.cardano.intersectmbo.org/pdfs/network-spec)
- [Network design document](https://ouroboros-network.cardano.intersectmbo.org/pdfs/network-design)

### Plutus (`crates/plutus`)
- [Plutus core repository](https://github.com/IntersectMBO/plutus)
- [CEK machine](https://github.com/IntersectMBO/plutus/tree/master/plutus-core/untyped-plutus-core/src/UntypedPlutusCore/Evaluation/Machine/Cek)
- [Builtin semantics](https://github.com/IntersectMBO/plutus/blob/master/plutus-core/plutus-core/src/PlutusCore/Default/Builtins.hs)
- [Cost model parameters](https://github.com/IntersectMBO/plutus/tree/master/plutus-core/cost-model)

### Node (`node/`)
- [Node integration repository](https://github.com/IntersectMBO/cardano-node)
- [Node runtime and packaging](https://github.com/IntersectMBO/cardano-node/tree/master/cardano-node)
- [Network configuration files](https://github.com/IntersectMBO/cardano-node/tree/master/configuration)
- [Transaction submit API](https://github.com/IntersectMBO/cardano-node/tree/master/cardano-submit-api)

### Cross-Cutting Documentation
- [Cardano developer portal](https://github.com/cardano-foundation/developer-portal/tree/staging/docs/)
- [Cardano blueprint](https://github.com/cardano-scaling/cardano-blueprint/tree/main/src) or [https://cardano-scaling.github.io/cardano-blueprint/](https://cardano-scaling.github.io/cardano-blueprint/)
- [Haddock documentation: ledger](https://cardano-ledger.cardano.intersectmbo.org/), [consensus](https://ouroboros-consensus.cardano.intersectmbo.org/haddocks/), [network](https://ouroboros-network.cardano.intersectmbo.org/)

##  Rules *Non-Negotiable*
- Always write typesafe Rust code.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant AGENTS.md file.
- Always research the official relevant upstream IntersectMBO repositories before introducing any local terminology, behavior, or design that is not directly traceable to an upstream source.
- New dependencies MUST be justified in `docs/DEPENDENCIES.md` before they are treated as accepted.
- FFI-backed cryptography and hidden native dependencies MUST NOT be introduced.
- Generated artifacts MUST remain reproducible and generated code MUST NOT be edited by hand.
- Implementation work MUST favor incremental milestones that compile and test cleanly.
- Public modules, types, and functions MUST have proper Rustdocs whenever behavior is non-obvious or externally consumed.
- Explanations of behavior or naming MUST be cross-checked against the official `cardano-node` and the relevant upstream IntersectMBO repositories.
- Type and function naming MUST stay as close to upstream terminology as practical so parity work and fixture comparison remain tractable.
- Cryptographic, protocol, and serialization parity with the official node is a non-negotiable long-term target even when an implementation slice is still incomplete.
- When you do not know how to proceed after researching the official node and upstream repositories, you may review [Amaru Rust node github repo](https://github.com/pragma-org/amaru/) and [Dolos Data-node github repo](https://github.com/txpipe/dolos/) for examples of how other Rust Cardano projects have approached similar problems, but do not treat them as authoritative sources for design or behavior decisions.
- Refer to and update `docs/ARCHITECTURE.md`, `docs/DEPENDENCIES.md`, `docs/SPECS.md`, `docs/CONTRIBUTING.md`, `docs/UPSTREAM_RESEARCH.md`, `docs/UPSTREAM_PARITY.md`, `docs/PARITY_SUMMARY.md`, and `docs/PARITY_PLAN.md` for project details and keep `./README.md` updated.


## Verification Expectations
- `cargo check-all`
- `cargo test-all`
- `cargo lint`

## Current Phase
- Workspace foundation is complete and compileable.
- Active implementation work is in progress across `crates/crypto`, `crates/cddl-codegen`, `crates/ledger`, `crates/network`, and `node/`.
- `crates/network` now includes handshake + mux + peer lifecycle, all five mini-protocol state machines/wire codecs (ChainSync, BlockFetch, KeepAlive, TxSubmission, PeerSharing), typed client drivers, typed server (responder) drivers for all four data mini-protocols (`KeepAliveServer`, `BlockFetchServer`, `ChainSyncServer`, `TxSubmissionServer`) plus `PeerSharingServer`, and SDU segmentation/reassembly support for large protocol messages. PeerSharing protocol (mini-protocol 10): `PeerSharingState` state machine, `PeerSharingMessage` (MsgShareRequest/MsgSharePeers/MsgDone), `SharedPeerAddress` IPv4/IPv6 CBOR codec, client driver `PeerSharingClient`, server driver `PeerSharingServer`. Root-set provider layer is expanded with DNS-backed root-peer provider (re-resolves local-root, bootstrap, public-root access points with optional `DnsRefreshPolicy` TTL clamping 60s/900s and exponential backoff). Peer registry tracks `PeerSource` and `PeerStatus` per peer, reconciles root-provider snapshots plus ledger, big-ledger, and peer-share source sets while preserving unrelated sources and peer status. Ledger peer provider layer is complete: `LedgerPeerProvider` trait, `LedgerPeerSnapshot` normalization (deduplicates and enforces disjoint ledger/big-ledger sets), `LedgerPeerProviderRefresh` (combined/per-kind), `apply_ledger_peer_refresh()` helper, `refresh_ledger_peer_registry()` orchestration, and `ScriptedLedgerPeerProvider` for testing. Provider refreshes reconcile the `PeerRegistry` on crate-owned paths without node involvement. Peer governor module (`governor.rs`): pure decision engine with `GovernorTargets`, `LocalRootTargets`, `GovernorAction` (PromoteToWarm/PromoteToHot/DemoteToWarm/DemoteToCold), evaluation functions for promotions/demotions/local-root valency, `GovernorState` with failure tracking and churn timing.
- `node/` orchestration, CLI, and sync pipeline:
  - CLI: `clap`-based binary with `run` (connect + sync) and `default-config` (emit JSON) subcommands. CLI flags override config file values. JSON configuration via `NodeConfigFile` (serde).
  - Bootstrap: `NodeConfig`, `PeerSession`, `bootstrap`.
  - Raw sync: `sync_step`, `sync_steps`, `sync_step_decoded`, `decode_shelley_blocks`.
  - Typed sync: `sync_step_typed`, `decode_shelley_header`, `decode_point`, `sync_steps_typed`, `sync_until_typed`. Typed ChainSync header/point decode and Shelley BlockFetch batch decode now happen in `yggdrasil-network`; `node/` keeps multi-era fetched-block decode.
  - Storage handoff: `apply_typed_step_to_volatile`, `apply_typed_progress_to_volatile`.
  - Intersection + batch: `typed_find_intersect`, `sync_batch_apply`. Typed ChainSync intersection, point/tip decode, and typed Shelley BlockFetch decode now happen in `yggdrasil-network`; `node/` keeps multi-era and storage orchestration.
  - KeepAlive: `keepalive_heartbeat`.
  - Managed service: `run_sync_service`, `SyncServiceConfig`, `SyncServiceOutcome`.
  - Consensus bridge: `shelley_opcert_to_consensus`, `shelley_header_to_consensus`, `verify_shelley_header`, `praos_header_to_consensus`, `verify_praos_header`.
  - Multi-era decode: `MultiEraBlock`, `decode_multi_era_block`, `decode_multi_era_blocks` (Byron/Shelley/Allegra/Mary/Alonzo/Babbage/Conway — all seven era tags). Byron blocks are structurally decoded via `ByronBlock::decode_ebb()`/`decode_main()`, carrying epoch, slot, chain_difficulty, prev_hash, and raw header bytes for correct header hash computation. Alonzo (tag 5) uses dedicated `AlonzoBlock` (5-element format with `invalid_transactions` and TPraos header), distinct from the 4-element `ShelleyBlock` used for Shelley/Allegra/Mary (tags 2–4).
  - Header hash: `ShelleyHeader::header_hash`, `PraosHeader::header_hash` (Blake2b-256), `ByronBlock::header_hash` (Blake2b-256 of prefix + raw header), `compute_tx_id`.
  - Verified pipeline: `multi_era_block_to_block`, `verify_multi_era_block` (dispatches Shelley verifier for pre-Babbage, Praos verifier for Babbage/Conway), `sync_step_multi_era`, `sync_batch_apply_verified`, `VerificationConfig`. Non-verified multi-era BlockFetch decode now happens in `yggdrasil-network`; verified raw+decoded BlockFetch batch handling also uses network helpers while verification and body-hash policy remain in `node/`.
  - Block body hash verification: `verify_block_body_hash` (Blake2b-256 of body elements vs header-declared hash), `extract_header_block_body_hash` (handles both 14-element Praos and 15-element Shelley header bodies), wired into `sync_batch_apply_verified` via `VerificationConfig.verify_body_hash`. `compute_block_body_hash` in ledger crate.
  - VRF data flow: bridge functions carry leader VRF proof/output (and nonce VRF for TPraos) through to consensus `HeaderBody`. `verify_block_vrf` + `VrfVerificationParams` enable per-block leader-proof verification when epoch nonce and stake data are available.
  - Nonce evolution wiring: `apply_nonce_evolution` extracts per-era VRF nonce contribution and prev_hash from `MultiEraBlock` and feeds `NonceEvolutionState::apply_block`. Byron blocks skipped.
  - Verified sync service: `run_verified_sync_service`, `VerifiedSyncServiceConfig`, `VerifiedSyncServiceOutcome` — async managed service using `sync_batch_apply_verified` with multi-era header/body verification, per-block nonce evolution tracking, and optional ChainState tracking. Reports final `NonceEvolutionState`, `ChainState`, and `stable_block_count` on shutdown.
  - Epoch boundary wiring: `advance_ledger_with_epoch_boundary()` in sync.rs detects epoch transitions via `is_new_epoch()` / `slot_to_epoch()` and calls `apply_epoch_boundary()` before the first block of each new epoch. `LedgerCheckpointTracking` optionally carries `StakeSnapshots` + `EpochSize`; when present, `update_ledger_checkpoint_after_progress` uses epoch-aware advancement. Automatically enabled when `nonce_config` provides `epoch_size`. Both ledger-advance functions accept `Option<&dyn PlutusEvaluator>` and call `apply_block_validated()`.
  - Plutus evaluation wiring: `plutus_eval.rs` in `node/src/` provides `CekPlutusEvaluator` implementing `PlutusEvaluator` using the `yggdrasil-plutus` CEK machine. Decodes Flat/CBOR-wrapped script bytes, applies datum (spending only), redeemer, and a version-aware `ScriptContext` built from the normalized ledger `TxContext`, then evaluates within declared `ExUnits` budget. `TxInfo` now carries resolved inputs/reference inputs, structured Shelley-family TxOut addresses, fee, mint, withdrawals, certificates, signatories, redeemers, datums, tx id, Conway votes/proposals, and treasury fields. V1/V2 accept any non-error result; V3 requires `Bool(true)`. Unsupported V3 certificate or proposal encodings now fail explicitly instead of fabricating placeholder integers.
  - Genesis parameter loading (Phase 7): `genesis.rs` in `node/src/` provides serde types for `ShelleyGenesis`, `AlonzoGenesis`, `ConwayGenesis`, and `build_protocol_parameters()` which assembles `ProtocolParameters` from genesis files. `NodeConfigFile` now exposes `ShelleyGenesisFile`, `AlonzoGenesisFile`, `ConwayGenesisFile` fields (matching official Cardano node config keys) and a `load_genesis_protocol_params()` method. Preset configs point to vendored genesis files. `main.rs` now centralizes genesis loading in a base-ledger-state helper and uses the resulting genesis-seeded `LedgerState` for startup peer-selection recovery, `validate-config`, `status`, and the resumed sync service, so fresh syncs and recovery/reporting paths all use the same network-derived thresholds instead of only the live sync path being seeded. `ConwayGenesis` also parses the `constitution` section (anchor + guardrails script hash) via `GenesisConstitution` / `GenesisConstitutionAnchor`. `build_genesis_enact_state()` and `NodeConfigFile::load_genesis_enact_state()` wire the genesis constitution into the base `LedgerState`'s `EnactState` at startup so governance validation uses the correct initial constitution and guardrails script hash.
  - NtC local socket server (`local_server.rs`): `BasicLocalQueryDispatcher` handles 8 LocalStateQuery tags: (0) CurrentEra, (1) ChainTip, (2) CurrentEpoch, (3) ProtocolParameters, (4) UTxOByAddress, (5) StakeDistribution, (6) RewardBalance, (7) TreasuryAndReserves. Queries operate via `LedgerStateSnapshot` and return opaque CBOR. LocalTxMonitor is wired into `SharedMempool`. LocalTxSubmission uses staged `apply_submitted_tx` before mempool insertion.
  - Plutus cost model calibration: `crates/plutus::CostModel` now exposes `from_alonzo_genesis_params()` which derives CEK step costs and per-builtin parameterized CPU/memory cost expressions from upstream named Alonzo/Babbage cost-model maps. `builtin_cost()` evaluates these per-builtin expressions against runtime argument ExMemory sizes, with flat fallback for any unmapped builtin. `NodeConfigFile::load_plutus_cost_model()` loads that calibrated model from `alonzo-genesis.json`, and when named maps are unavailable it now maps the live 251-entry Conway `plutusV3CostModel` array into the same named-parameter pipeline (up through `byteStringToInteger-memory-arguments-slope`) instead of using the earlier CEK-only structural fallback. `VerifiedSyncServiceConfig` carries the resulting model as `plutus_cost_model`, and checkpoint-tracked ledger replay uses a stored `CekPlutusEvaluator` built from it instead of recreating default-cost evaluators per batch. Remaining work is cost-shape parity for any still-approximated builtins and future Conway tail parameters beyond the current vendored 251-name surface.
  - ChainState integration: `multi_era_block_to_chain_entry`, `track_chain_state`, `promote_stable_blocks`. Wires consensus `ChainState` into the sync pipeline with stability window enforcement and stable-block promotion from volatile to immutable storage. All eras including Byron are tracked.
  - Genesis parameters: `NodeConfigFile` includes `epoch_length` (432000), `security_param_k` (2160), `active_slot_coeff` (0.05). CLI `run` command computes `stability_window = 3k/f` and builds `NonceEvolutionConfig` from config.
  - Network presets: `NetworkPreset` enum (`Mainnet | Preprod | Preview`) with `FromStr`/`Display` and per-network constructors. CLI `--network` flag selects preset. Configuration files for all three networks stored in `node/configuration/`.
  - Mempool eviction: `extract_tx_ids`, `evict_confirmed_from_mempool`.
- `crates/mempool` now includes fee-ordered queue with `TxId`-based entries, duplicate detection, capacity enforcement, `remove_by_id`, `remove_confirmed` for block-application eviction, TTL-aware admission (`insert_checked`, `purge_expired`), iterator support, and relay-facing entry conversion to/from ledger `MultiEraSubmittedTx` with stored era + raw submitted-transaction bytes.
- `crates/ledger`:
  - `LedgerState` with dual UTxO: legacy `ShelleyUtxo` + generalized `MultiEraUtxo`, era-aware `apply_block()` dispatch (Shelley through Conway).
  - Submitted-transaction abstractions in `tx.rs`: `compute_tx_id`, `ShelleyCompatibleSubmittedTx<TBody>`, `AlonzoCompatibleSubmittedTx<TBody>`, and `MultiEraSubmittedTx::from_cbor_bytes_for_era()` for Shelley-based transaction relay boundaries.
  - `MultiEraUtxo` with per-era apply methods, coin/multi-asset preservation, TTL/validity-interval checks.
  - `MultiEraTxOut` enum (Shelley/Mary/Alonzo/Babbage variants) with `coin()`/`value()`/`address()` accessors.
  - Allegra era types (`AllegraTxBody`, `NativeScript`).
  - Mary era types (`Value`, `MultiAsset`, `MaryTxBody`).
  - Alonzo era types (`ExUnits`, `Redeemer`, `AlonzoTxOut`, `AlonzoTxBody`, `AlonzoBlock`).
  - Byron envelope (`ByronBlock`) with structural header decode — epoch, slot-in-epoch, `chain_difficulty` (block number), prev_hash, raw header bytes. `header_hash()` computes `Blake2b-256(prefix ++ raw_header_cbor)` with variant-specific prefix (`0x82 0x00` for EBB, `0x82 0x01` for Main).
  - Babbage era types (`DatumOption`, `BabbageTxOut`, `BabbageTxBody`, `BabbageBlock` with `PraosHeader`).
  - Conway era types (`Vote`, `Voter`, `GovActionId`, `Constitution`, `GovAction` (7-variant typed enum: ParameterChange/HardForkInitiation/TreasuryWithdrawals/NoConfidence/UpdateCommittee/NewConstitution/InfoAction), `VotingProcedure`, `ProposalProcedure` (typed `GovAction`), `VotingProcedures`, `ConwayTxBody`, `ConwayBlock` with `PraosHeader`).
  - Credential and address types (`StakeCredential`, `RewardAccount`, `Address` with Base/Enterprise/Pointer/Reward/Byron variants, `AddrKeyHash`, `ScriptHash`, `PoolKeyHash` type aliases). Strict validation now rejects invalid Shelley network ids and malformed pointer encodings, and exposes Byron bootstrap-address CRC32 verification through `Address::validate_bytes()`.
  - Certificate hierarchy (`Anchor`, `UnitInterval`, `Relay`, `PoolMetadata`, `PoolParams`, `DRep`, `DCert` with 19 CDDL-aligned variants covering Shelley tags 0–5 and Conway tags 7–18).
  - Signed integer CBOR helpers.
  - TxBody keys 4–6 (`certificates`, `withdrawals`, `update` as typed `ShelleyUpdate` carrying typed `ProtocolParameterUpdate` deltas for Shelley–Babbage; Conway omits key 6).
  - WitnessSet keys 0–7 (`vkey_witnesses`, `native_scripts`, `bootstrap_witnesses`, `plutus_v1_scripts`, `plutus_data` (typed `Vec<PlutusData>`), `redeemers` (typed `PlutusData` payload), `plutus_v2_scripts`, `plutus_v3_scripts`). Typed `BootstrapWitness`. Conway map-format redeemers supported.
  - PlutusData AST (`Constr`/`Map`/`List`/`Integer`/`Bytes`) with full recursive CBOR codec including compact constructor tags 121–127, general form tag 102, and bignum encoding. `Script` enum (Native/PlutusV1/V2/V3), `ScriptRef` with tag-24 double encoding. `BabbageTxOut.script_ref` is now typed `Option<ScriptRef>`. `DatumOption::Inline` is now typed `PlutusData` (tag-24 double encoding). `Redeemer.data` is now typed `PlutusData`.
  - Full era type and block coverage from Byron through Conway is complete.
  - Ledger rule foundation modules: `ProtocolParameters` (CBOR map codec, Shelley/Alonzo defaults, `min_lovelace_for_utxo()`, `apply_update()`), `ProtocolParameterUpdate` (typed sparse CBOR-map delta for Shelley/Conway parameter proposals), `fees.rs` (linear fee + script fee calculation/validation), `native_script.rs` (timelock evaluator + Blake2b-224 script hash), `collateral.rs` (Alonzo+ collateral validation), `min_utxo.rs` (per-output minimum lovelace enforcement), `witnesses.rs` (VKey witness sufficiency, Ed25519 signature verification via `verify_vkey_signatures()`, and required hash collection helpers). `LedgerState` carries `Option<ProtocolParameters>` (CBOR array element 10, backward-compatible with legacy 9-element).
  - Witness & native script validation wiring: `Tx` struct carries optional serialized witness bytes. All per-era `apply_block()` inner loops (Shelley through Conway) compute required VKey hashes from spending inputs, certificates, withdrawals, and `required_signers` (Alonzo+), then call `validate_witnesses_if_present()` which enforces both VKey hash sufficiency and real Ed25519 signature verification against the transaction body hash. Allegra through Conway additionally compute required script hashes and call `validate_native_scripts_if_present()` for native timelock evaluation. `Address::payment_credential()` extracts payment credentials for UTxO-driven hash collection. 12 integration tests cover VKey sufficiency (accept/reject/skip/empty), Ed25519 signature verification (forged signature, wrong body), and native script evaluation (ScriptPubkey, InvalidBefore, InvalidHereafter, ScriptAll multisig).
  - Epoch boundary processing (Phase 4): `stake.rs` (stake distribution snapshots — `IndividualStake`, `Delegations`, `StakeSnapshot`, `StakeSnapshots` three-snapshot ring with fee pot, `PoolStakeDistribution`, `compute_stake_snapshot()`), `rewards.rs` (epoch reward calculation — `RewardParams`, `EpochRewardPot`, `EpochRewardDistribution`, `compute_epoch_rewards()`, u128 fixed-point), `epoch_boundary.rs` (`apply_epoch_boundary()` NEWEPOCH/SNAP/RUPD orchestration, `retire_pools_with_refunds()`, `remove_expired_governance_actions()`, DRep inactivity detection, `EpochBoundaryEvent`). Governance action expiry follows the upstream Conway EPOCH rule: proposals whose `expires_after` epoch has passed are pruned at each epoch boundary and deposits are refunded to registered return accounts. DRep inactivity follows the upstream Conway `drepExpiry` rule: DReps whose `last_active_epoch + drep_activity < current_epoch` are counted as inactive but remain registered (excluded from ratification quorum). `ProtocolParameters` carries `drep_deposit` (key 31) and `drep_activity` (key 32) for Conway DRep governance parameters; genesis wiring maps ConwayGenesis `d_rep_deposit`/`d_rep_activity` into these fields. `RegisteredDrep` tracks `last_active_epoch`; activity is touched on registration, update, and vote via `touch_drep_activity_for_certs()` and `apply_conway_votes()`. `DepositPot` and `AccountingState` in `state.rs` track key/pool/drep deposits and treasury/reserves. `LedgerState` now 16-field struct with backward-compatible CBOR (9/10/12/15/16-element decode). Certificate processing tracks deposits across all 19 `DCert` variants. `process_retirements()` on `PoolState`.
  - Governance enactment (Phase 5): `EnactState` struct in `state.rs` tracks the enacted constitution, committee quorum threshold, and four purpose-lineage prev-action-ids (`prev_pparams_update`, `prev_hard_fork`, `prev_committee`, `prev_constitution`) matching upstream `GovRelation`. `enact_gov_action()` free function implements the Conway ENACT rule for all seven `GovAction` variants: InfoAction (no effect), NewConstitution (replace constitution + lineage), NoConfidence (remove all committee members + reset quorum + lineage), UpdateCommittee (add/remove members + set quorum + lineage), HardForkInitiation (update protocol_version + lineage), TreasuryWithdrawals (credit registered reward accounts from treasury), ParameterChange (apply typed `ProtocolParameterUpdate` to `LedgerState.protocol_params` + record lineage). Returns `EnactOutcome` enum. `LedgerState` carries `enact_state: EnactState` (element 16, backward-compatible). `LedgerStateSnapshot` mirrors the field. Enacted-root semantics wired into `validate_conway_proposals()`: `prev_action_id = None` is only valid when `EnactState` has no enacted root for that purpose; `prev_action_id = Some(id)` must match either the enacted root or a stored pending proposal of the same purpose. `NoConfidence` and `UpdateCommittee` share the Committee purpose group. Ratification tally engine: `VoteTally`, `tally_committee_votes`, `tally_drep_votes` (stake-weighted), `tally_spo_votes` (pool-stake-weighted), `drep_threshold_for_action`/`spo_threshold_for_action`, `accepted_by_committee`/`accepted_by_dreps`/`accepted_by_spo` predicates, `ratify_action` combined predicate (reference: `Cardano.Ledger.Conway.Rules.Ratify`). `PoolVotingThresholds` (5 fields, CDDL key 25), `DRepVotingThresholds` (10 fields, CDDL key 26), `min_committee_size` (key 27), `committee_term_limit` (key 28) in `ProtocolParameters`. Epoch-boundary ratification wiring is a future slice.
- `crates/storage` now includes file-backed implementations (`FileImmutable`, `FileVolatile`, `FileLedgerStore`) with JSON-based on-disk persistence, directory scanning on open, rollback-aware file deletion, and re-open persistence. 19 integration tests cover all trait methods.
- `crates/consensus` now includes `SecurityParam` (Ouroboros `k`), `ChainState` volatile chain tracker with roll-forward/roll-backward, max rollback depth enforcement, stability window detection (`stable_count`, `drain_stable`), and non-contiguous block rejection. `HeaderBody` carries VRF proof data (`leader_vrf_output`, `leader_vrf_proof`, optional `nonce_vrf_output`/`nonce_vrf_proof` for TPraos). `OpCert` field names aligned with CDDL (`hot_vkey`, `sequence_number`). Epoch nonce evolution state machine (`NonceEvolutionState`) implements UPDN + TICKN rules with `vrf_output_to_nonce` and `NonceEvolutionConfig`. Chain selection implements upstream Praos tiebreaker (`comparePraos` from `ouroboros-consensus/Protocol/Praos/Common.hs`): `ChainCandidate` with `issuer_vkey_hash`, `ocert_issue_no`, `vrf_tiebreaker`; `select_preferred` with `VrfTiebreakerFlavor` (unrestricted pre-Conway, restricted post-Conway). 70+ consensus tests.
- Upstream naming alignment is complete across ledger and consensus crates:
  - Ledger ShelleyHeaderBody: `block_number`, `slot`, `issuer_vkey`, `vrf_vkey`, `nonce_vrf`, `leader_vrf`, `block_body_size`, `block_body_hash`, `operational_cert` (with `hot_vkey`, `sequence_number`, `kes_period`, `sigma`). 15-element CBOR array (Shelley through Alonzo).
  - Ledger PraosHeaderBody: `block_number`, `slot`, `issuer_vkey`, `vrf_vkey`, `vrf_result`, `block_body_size`, `block_body_hash`, `operational_cert`. 14-element CBOR array with single VRF result (Babbage/Conway).
  - Ledger block fields: `transaction_witness_sets` (all eras), `transaction_metadata_set` (Shelley), `auxiliary_data_set` (Babbage/Conway).
  - Consensus HeaderBody: `block_number`, `slot`, `issuer_vkey`, `vrf_vkey`, `leader_vrf_output`, `leader_vrf_proof`, `nonce_vrf_output` (TPraos only), `nonce_vrf_proof` (TPraos only), `block_body_size`, `block_body_hash`, `operational_cert`.
  - Consensus OpCert: `hot_vkey`, `sequence_number`.
  - DCert variants aligned with CDDL certificate names: `AccountRegistration`, `AccountUnregistration`, `DelegationToStakePool`, `PoolRegistration`, `PoolRetirement`, `GenesisDelegation`, plus Conway-era `AccountRegistrationDeposit` through `DrepUpdate`.
- CBOR golden round-trip parity tests cover `ShelleyTxBody`, `ShelleyBlock`, `PlutusData`, `StakeCredential`, `MultiEraTxOut`, and submitted-transaction round-trips for all seven eras (Byron TX, Shelley, Allegra, Mary, Alonzo, Babbage, Conway), plus `MultiEraSubmittedTx` and TX ID determinism. Cross-subsystem integration tests verify block→ChainState→storage and rollback flows.
- `crates/cddl-codegen` now provides `generate_module_with_codecs()` which generates struct/enum definitions **plus** `CborEncode`/`CborDecode` implementations for integer-keyed maps (map encode/decode with key dispatch and optional field handling), string-keyed maps, array structs, and group-choice enums. 26 integration tests cover parsing, generation, and codec generation.
- `crates/ledger` Byron transaction support is complete: `ByronTxIn`, `ByronTxOut`, `ByronTx` (with `tx_id()` via Blake2b-256), `ByronTxWitness`, `ByronTxAux` — all with full CborEncode/CborDecode handling CBOR tag 24 (CBOR-in-CBOR). `ByronBlock::MainBlock` carries `transactions: Vec<ByronTxAux>` decoded from block body `tx_payload`. Byron blocks now have real UTxO state transitions: `apply_byron_block()` decodes `ByronTx` from transaction body bytes, applies each atomically via `MultiEraUtxo::apply_byron_tx()` which validates input existence, non-negative implicit fee, and converts Byron inputs/outputs to the unified `ShelleyTxIn`/`ShelleyTxOut` representation. 15+ Byron-specific tests.
- 2918 workspace tests pass across all crates, 0 failures.
- New subfolder-level AGENTS.md files should only be added where a folder has a stable domain boundary.
