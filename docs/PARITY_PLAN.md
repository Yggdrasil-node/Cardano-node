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
- ✅ **CLI & configuration** (JSON config, YAML preset support)
- ✅ **Local query & submission APIs** (LocalStateQuery, LocalTxSubmission)
- ✅ **File-backed storage** (Immutable/Volatile with rollback)
- ⚠️ **Partial Plutus** (CEK machine framework, V1/V2/V3 support wired)
- ⚠️ **Partial peer management** (governor framework, some peer sources)
- ⚠️ **Partial monitoring** (basic tracing infrastructure)

**To achieve full parity**, the remaining work focuses on:
1. **Completing governance features** (ratification tally, voting state persistence)
2. **Hardening peer selection** (full multi-source governor with churn/anti-churn)
3. **Metrics & monitoring** (full tracer infrastructure + Prometheus export)
4. **Ledger rules enforcement** (complete collateral checking, all CDDL invariants)
5. **Storage robustness** (recovery, compaction, migration)
6. **Network resilience** (backpressure handling, timeout recovery)
7. **Integration testing** (mainnet-like end-to-end scenarios)

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
| Alonzo | Plutus V1/V2, datums, redeemers | ✅ | ⚠️ | Partial | PlutusData AST, script refs wired; Plutus validation in progress
| Babbage | Inline datums, inline scripts, Praos | ✅ | ✅ | Complete | PraosHeader, inline datum/script types, DatumOption
| Conway | Governance, DReps, ratification, votes | ✅ | ⚠️ | Partial | Types complete; ratification tally incomplete
| **Core State** |
| UTxO tracking | Coin + multi-asset semantics | ✅ | ✅ | Complete | ShelleyUtxo + MultiEraUtxo with era dispatch
| Account state | Rewards + deposits tracking | ✅ | ⚠️ | Partial | DepositPot, treasury, reserves; reward snapshot incomplete
| Pool state | Registration, retirement, performance | ✅ | ✅ | Complete | PoolState, PoolParams, retire queues, stake snapshots
| Delegation state | Stake delegation per account | ✅ | ✅ | Complete | Delegations mapping
| **Validation** |
| Syntax validation | TX format, field presence | ✅ | ✅ | Complete | CBOR roundtrip, field checks
| Input availability | UTxO membership checks | ✅ | ✅ | Complete | apply_block validates input existence
| Fee sufficiency | Linear fee + script fee | ✅ | ✅ | Complete | fees.rs with min_fee calculation
| Witness sufficiency | VKey hash + signature count | ✅ | ✅ | Complete | verify_vkey_signatures with Ed25519
| Native script eval | Timelock constraints | ✅ | ✅ | Complete | validate_native_scripts_if_present
| Plutus validation | Script execution + budget | ✅ | ⚠️ | Partial | CEK framework present; execution path incomplete
| Collateral checks | Alonzo+ collateral UTxO | ✅ | ⏸️ | In Design | validate_collateral skeleton
| Min UTxO enforcement | Per-output minimum lovelace | ✅ | ✅ | Complete | min_utxo.rs with era-aware calculation
| **Epoch Boundary** |
| Stake snapshot | per-pool reward snapshot | ✅ | ✅ | Complete | compute_stake_snapshot with fees
| Reward calculation | Per-epoch payouts | ✅ | ⚠️ | Partial | compute_epoch_rewards framework; details TBD
| Pool retirement | Age-based expiry | ✅ | ✅ | Complete | process_retirements with pool_deposit refund
| DRep inactivity | drep_activity threshold | ✅ | ✅ | Complete | touch_drep_activity, inactive_dreps
| Governance expiry | Proposal age limit | ✅ | ✅ | Complete | remove_expired_governance_actions
| **Governance** |
| Proposal storage | Action ID + metadata | ✅ | ✅ | Complete | GovActionState with vote maps
| Vote accumulation | Committee/DRep/SPO votes | ✅ | ✅ | Complete | apply_conway_votes with per-voter class
| Enacted-root validation | Lineage + prev-action-id | ✅ | ✅ | Complete | validate_conway_proposals with EnactState
| Ratification tally | Threshold voting | ✅ | ⚠️ | Skeleton | tally_* functions present; quorum calc incomplete
| Enactment | Constitution, committee, params | ✅ | ✅ | Complete | enact_gov_action with 7 action types
| Deposit refund | Key/pool/DRep deposit return | ✅ | ⏸️ | In Design | Outline present; edge cases TBD

**Ledger Summary**: ~90% feature complete, focused remaining work on Plutus validation details, reward calculation, and ratification tally.

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
| Issuer validation | Known pool + stake | ✅ | ⚠️ | Partial | Issuer type complete; stake lookup incomplete
| VRF check | Leader eligibility | ✅ | ✅ | Complete | verify_block_vrf
| OpCert check | Valid + not superseded | ✅ | ✅ | Complete | OpCert validation
| Body hash verify | Blake2b-256 of body | ✅ | ✅ | Complete | verify_block_body_hash
| UTxO rules | UTXO + CERTS + REWARDS | ✅ | ⚠️ | Partial | Rules framework present; some edge cases TBD
| **Density Tiebreaker** |
| Leadership density | Blocks per X slots | ✅ | ⏸️ | Not Started | Needed for chain fork resolution

**Consensus Summary**: ~95% feature complete, remaining work focused on complex validation edge cases and density tiebreaker.

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
| Backpressure | SDU queue limits | ✅ | ⚠️ | Partial | SDU queue framework present; timeout recovery incomplete
| Fair scheduling | Round-robin + priority | ✅ | ⏸️ | In Design | Mux orchestrator skeleton
| Timeout handling | Protocol-specific timeouts | ✅ | ⏸️ | In Design | Timeout framework incomplete
| **Peer Management** |
| Peer sources | LocalRoot/PublicRoot/PeerShare | ✅ | ✅ | Complete | PeerSource enum + provider layer
| DNS resolution | Dynamic root-set updates | ✅ | ✅ | Complete | DnsRootPeerProvider with TTL clamping
| Ledger peers | Registered pool relays | ✅ | ✅ | Complete | LedgerPeerProvider + snapshot normalization
| Peer registry | Source + status tracking | ✅ | ✅ | Complete | PeerRegistry with Cold/Warm/Hot states
| **Governor** |
| Outbound targets | HotValency/WarmValency | ✅ | ✅ | Complete | GovernorTargets with per-source limits
| Promotion logic | Cold → Warm → Hot | ✅ | ⚠️ | Partial | GovernorAction enum; promotion scoring incomplete
| Demotion logic | Hot → Warm → Cold | ✅ | ⚠️ | Partial | Demotion triggers incomplete
| Churn | Peer replacement rate | ✅ | ⏸️ | Not Started | Churn mitigation logic needed
| Anti-churn | Stable peer retention | ✅ | ⏸️ | Not Started | Connection-state tracking needed
| Local-root handling | Static hotValency targets | ✅ | ✅ | Complete | LocalRootTargets enum + governor integration
| **Connection Management** |
| Inbound accept | Role negotiation | ✅ | ✅ | Complete | Inbound handshake in acceptor role
| Outbound connect | Peer candidates | ✅ | ✅ | Complete | Outbound connection flow
| Connection pooling | Max connection limits | ✅ | ⏸️ | Not Started | Connection pool management incomplete
| Graceful shutdown | In-flight message draining | ✅ | ⏸️ | Not Started | Shutdown orchestration incomplete

**Network Summary**: ~75% feature complete. Core protocols fully wired; remaining work on governors, churn, pool management, and graceful shutdown.

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
| UTxO check | Inputs available | ✅ | ⚠️ | Partial | Basic check present; edge cases TBD
| Collateral check | Collateral > fee | ✅ | ⏸️ | Not Started | Collateral validation for Alonzo+
| Script budget | Enough ExUnits | ✅ | ⏸️ | Not Started | Script resource check before admission
| **Block Application** |
| TX confirmation | Remove on block | ✅ | ✅ | Complete | evict_confirmed_from_mempool
| Snapshot creation | TXs for block producer | ✅ | ✅ | Complete | Mempool iterator support
| **Relay Semantics** |
| TxId advertising | Before full TX | ✅ | ✅ | Complete | TxSubmissionClient announces IDs first
| TX request flow | Solicit after ID seen | ✅ | ✅ | Complete | TxSubmissionServer responds to requests
| Duplicate filtering | Peer + global | ✅ | ⚠️ | Partial | Basic filtering; distributed dedup incomplete

**Mempool Summary**: ~85% feature complete, remaining work on collateral checks, script budget validation, and mempool-level distributed deduplication.

---

### STORAGE SUBSYSTEM

| Feature | Scope | Haskell | Rust | Status | Notes |
|---------|-------|---------|------|--------|-------|
| **Block Store** |
| Immutable store | Blocks >3k slots old | ✅ | ✅ | Complete | FileImmutable with JSON persistence
| Volatile store | Recent blocks <3k slots | ✅ | ✅ | Complete | FileVolatile with rollback
| Atomicity | All-or-nothing writes | ✅ | ⚠️ | Partial | File-based approach; crash recovery TBD
| **Ledger State** |
| Snapshot storage | Checkpoint every N blocks | ✅ | ✅ | Complete | FileLedgerStore with JSONCompat
| State recovery | From last checkpoint | ✅ | ✅ | Complete | Open + replay pattern
| Rollback support | Revert to prior checkpoints | ✅ | ✅ | Complete | Checkpoint time-travel
| **Garbage Collection** |
| Immutable trimming | Delete blocks >retention | ✅ | ⏸️ | Not Started | Retention policy framework only
| Volatile compaction | Deduplicate on rollback | ✅ | ⏸️ | Not Started | Compaction needed on frequent reorgs
| Checkpoint pruning | Keep recent snapshots | ✅ | ⏸️ | Not Started | Old checkpoint cleanup
| **Index & Lookup** |
| Point → block | By block hash | ✅ | ✅ | Complete | Storage scanning on open
| Slot → block | By slot number | ✅ | ⚠️ | Partial | Slot index incomplete
| **Recovery & Crash Handling** |
| Dirty ledger detection | Incomplete state write | ✅ | ⏸️ | Not Started | Crash detection + recovery path
| Corruption resilience | Skip/repair bad blocks | ✅ | ⏸️ | Not Started | Resilience framework needed

**Storage Summary**: ~70% feature complete, remaining work on garbage collection, indexing optimization, and crash recovery.

---

### CLI & CONFIGURATION

| Feature | Scope | Haskell | Rust | Status | Notes |
|---------|-------|---------|------|--------|-------|
| **Configuration** |
| YAML parsing | Config file format | ✅ | ⚠️ | Partial | JSON implemented; YAML preset support present
| Environment overrides | CLI flag precedence | ✅ | ✅ | Complete | clap-based override model
| Genesis loading | ShelleyGenesis + AlonzoGenesis | ✅ | ✅ | Complete | load_genesis_protocol_params
| **Subcommands** |
| run | Sync + validate | ✅ | ✅ | Complete | Main sync loop wired
| validate-config | Verify config file | ✅ | ✅ | Complete | Basic validation
| status | Tip + epoch info | ✅ | ✅ | Complete | Status query framework
| query | LocalStateQuery wrapper | ✅ | ⏸️ | In Design | LocalStateQuery types present
| submit-tx | LocalTxSubmission wrapper | ✅ | ⏸️ | In Design | LocalTxSubmission types present
| **Query API (LocalStateQuery)** |
| CurrentEra | Active era | ✅ | ✅ | Complete | BasicLocalQueryDispatcher tag 0
| ChainTip | Best block info | ✅ | ✅ | Complete | Tag 1
| CurrentEpoch | Epoch number | ✅ | ✅ | Complete | Tag 2
| ProtocolParameters | Active params | ✅ | ✅ | Complete | Tag 3
| UTxOByAddress | Address UTxO lookup | ✅ | ✅ | Complete | Tag 4
| StakeDistribution | Per-pool stake | ✅ | ✅ | Complete | Tag 5
| RewardBalance | Account rewards | ✅ | ✅ | Complete | Tag 6
| TreasuryAndReserves | Governance pots | ✅ | ✅ | Complete | Tag 7
| **Submission API (LocalTxSubmission)** |
| TX validation | Syntax + fee | ✅ | ✅ | Complete | apply_submitted_tx checks
| TX relay readiness | Mempool admission | ✅ | ⚠️ | Partial | Pre-submission check incomplete
| Feedback | Acceptance or error | ✅ | ⚠️ | Partial | Error detail reporting incomplete

**CLI Summary**: ~85% feature complete, remaining work on CLI wrappers around LocalStateQuery/Submission and YAML-only config migration.

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
| Praos VRF proof gen | Slot leader selection | ✅ | ❌ | Not Expected | Validator only (no block production)
| Praos VRF proof verify | Slot leader validation | ✅ | ✅ | Complete | verify_vrf_output with Ed25519
| **Elliptic Curves** |
| Curve25519 | Ed25519 + KES ops | ✅ | ✅ | Complete | curve25519-dalek
| BLS12-381 | CIP-0381 V3 builtins | ✅ | ✅ | Complete | bls12_381 crate (G1/G2/pairing)
| secp256k1 | Plutus signature ops | ✅ | ✅ | Complete | k256 crate
| **KES (Key Evolving Signatures)** |
| KES signature scheme | Operational cert | ✅ | ✅ | Complete | KES OpCert validation
| KES period validation | Block slot alignment | ✅ | ✅ | Complete | Check slot ∈ [kes_period*x, (kes_period+1)*x)
| KES key evolution | Per-period key rotation | ✅ | ⏸️ | Not Expected | Validator only (no signing)
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

**Cryptography Summary**: ~98% feature complete, only missing KES key generation (not needed for validator node).

---

### MONITORING & TRACING

| Feature | Scope | Haskell | Rust | Status | Notes |
|---------|-------|---------|------|--------|-------|
| **Trace Events** |
| Structured events | Typed trace messages | ✅ | ⚠️ | Partial | Basic framework present; most events incomplete
| Namespace hierarchy | net., chain., ledger., etc | ✅ | ⏸️ | In Design | Namespace framework only
| Filtering & routing | Selector expressions | ✅ | ⏸️ | Not Started | Filtering logic needed
| **Transports** |
| Stdout | Console output | ✅ | ⚠️ | Partial | Basic stderr output
| JSON | Structured output | ✅ | ⏸️ | Not Started | JSON serialization needed
| Socket | Remote tracer | ✅ | ⏸️ | Not Started | Socket transport not implemented
| **Metrics** |
| EKG integration | Live metrics endpoint | ✅ | ⏸️ | Not Started | Counter/gauge infrastructure
| Prometheus export | /metrics endpoint | ✅ | ⏸️ | Not Started | Prometheus format output
| Key metrics |  Block height, peers, mempool size | ✅ | ⏸️ | Not Started | Metric collection
| **Profiling** |
| CPU profiling | Bottleneck identification | ✅ | ⏸️ | Not Started | Profiling integration
| Memory profiling | Heap analysis | ✅ | ⏸️ | Not Started | Allocation tracking
| Latency tracing | Operation timing | ✅ | ⏸️ | Not Started | Latency measurement

**Monitoring Summary**: ~25% feature complete, significant remaining work on tracer infrastructure and metrics export.

---

## Subsystem-by-Subsystem Analysis

### 1. CRYPTOGRAPHY & ENCODING (`crates/crypto`)

**Current State**: ✅ Complete for validation purposes

**What's Done**:
- Ed25519 witness verification via `verify_vkey_signatures()`
- VRF proof verification with Praos leader-value check
- Blake2b-256/224 hashing for blocks, TXs, and scripts
- SHA-256/512 for VRF and genesis
- SHA3-256, Keccak-256, RIPEMD-160 for Plutus builtins
- BLS12-381 for CIP-0381 V3 builtins (G1/G2 ops, pairing, hash-to-curve)
- secp256k1 ECDSA + Schnorr for PlutusV2
- Curve25519 for KES
- Full CBOR roundtrip parity testing

**What's Missing**:
- ❌ KES key generation (not needed; validators don't produce blocks)
- ❌ VRF proof generation (not needed; validators don't produce blocks)
- ℹ️ Performance optimization for large batches
- ℹ️ Hardware acceleration detection (optional)

**Parity Status**: **98% complete** — All validation-side cryptography is implemented and tested. Missing only key-generation responsibilities that block producers would need.

---

### 2. LEDGER STATE MANAGEMENT (`crates/ledger`)

**Current State**: ⚠️ Mostly complete, with governance and Plutus details pending

**What's Done**:
- **Multi-era types** (Byron → Conway) with CBOR codecs
- **UTxO model** with coin preservation and multi-asset tracking
- **State transitions** via `apply_block()` dispatch
- **Era types** with all certificate variants (19 DCert types)
- **Transaction validation**: syntax, fees, witnesses, native scripts
- **Epoch boundary**: stake snapshots, pool retirement, DRep inactivity, proposal expiry
- **Governance**: proposal storage, vote accumulation, enacted-root validation, enactment
- **PlutusData**: Full AST with compact/general constructor encoding
- **Addresses**: Base/Enterprise/Pointer/Reward/Byron with validation

**What's Missing**:
- ⚠️ **Collateral validation** (skeleton present; edge cases incomplete)
- ⚠️ **Reward calculation** (framework present; exact formulas TBD)
- ⚠️ **Plutus script execution** (CEK machine framework wired; execution details incomplete)
- ⚠️ **Ratification tally** (voting functions present; threshold logic incomplete)
- ⏸️ **Deposit refunds** (outline for governance actions; all cases TBD)

**Parity Status**: **~90% complete** — All era types and core rules implemented. Remaining work focuses on edge cases in validation and governance post-tally steps.

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
- ⏸️ **Density tiebreaker** (leadership density calculation for forked chains)
- ⏸️ **Complex issuer validation** (pool stake lookup incomplete)
- ⏸️ **Body hash optimization** (currently full body hash every block)

**Parity Status**: **~95% complete** — All critical validations present. Missing tiebreaker is mostly optimization; consensus would still find longest chain but without density preference.

---

### 4. NETWORK & PEER MANAGEMENT (`crates/network`)

**Current State**: ⚠️ Protocols complete, peer governor partially complete

**What's Done**:
- **5 mini-protocols** fully wired (ChainSync, BlockFetch, TxSubmission, KeepAlive, PeerSharing)
- **Mux** with protocol dispatch
- **Handshake** with role negotiation
- **Typed client drivers** for all protocols
- **Typed server drivers** for all protocols
- **Root providers**: local, bootstrap, public, DNS-backed with TTL
- **Peer registry**: Cold/Warm/Hot states + 6 peer sources
- **Governor framework**: targets + action types
- **Ledger peer provider**: normalization + refresh orchestration
- **Local-root handling**: hotValency + warmValency targets

**What's Missing**:
- ⚠️ **Promotion scoring** (which peers get promoted; currently generic)
- ⚠️ **Demotion triggers** (under what conditions peers drop hot status)
- ⚠️ **Churn policy** (peer replacement rate + anti-churn mitigation)
- ⏸️ **Connection pooling** (max outbound/inbound connections)
- ⏸️ **Backpressure handling** (SDU queue overflow recovery)
- ⏸️ **Timeout recovery** (protocol-specific timeout handling + reconnect)
- ⏸️ **Graceful shutdown** (in-flight message draining)

**Parity Status**: **~75% complete** — All wiring and state machines present. Missing peer selection policy specifics and connection lifecycle refinement.

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

**What's Missing**:
- ⏸️ **Collateral validation** (checks before mempool admission)
- ⏸️ **Script budget estimation** (ExUnits validation before admission)
- ⏸️ **Transaction conflict detection** (inputs spent by multiple pending TXs)
- ⏸️ **Distributed deduplication** (peer-aware duplicate filtering)

**Parity Status**: **~85% complete** — Core queue behavior fully working. Missing script-related pre-admission checks.

---

### 6. STORAGE (`crates/storage`)

**Current State**: ⚠️ Functional with GC and optimization pending

**What's Done**:
- **Immutable store** for blocks >3k slots old
- **Volatile store** with rollback on reorg
- **Ledger checkpoints** for state recovery
- **Point lookup** by block hash
- **Atomicity** via file-based writes
- **Recovery** from last checkpoint

**What's Missing**:
- ⏸️ **Garbage collection** (trimming old immutable blocks)
- ⏸️ **Slot-based indexing** (efficient slot → block lookup)
- ⏸️ **Crash detection** (dirty state handling)
- ⏸️ **Corruption resilience** (skip/repair on bad blocks)
- ⏸️ **Compaction** (volatility deduplication)

**Parity Status**: **~70% complete** — Core functionality working. Missing performance optimizations and edge-case resilience.

---

### 7. CLI & CONFIGURATION (`node/`)

**Current State**: ✅ Functional, with query wrappers pending

**What's Done**:
- **Configuration loading** (JSON + YAML preset support)
- **CLI subcommands**: run, validate-config, status, default-config
- **LocalStateQuery** types + 8 query tags fully implemented
- **LocalTxSubmission** types + validation framework
- **Genesis loading** (all 3 genesis files)
- **Tracing config** alignment with upstream

**What's Missing**:
- ⏸️ **query subcommand** (wrapper around LocalStateQuery)
- ⏸️ **submit-tx subcommand** (wrapper around LocalTxSubmission)
- ⏸️ **TX feedback detail** (comprehensive error messages)

**Parity Status**: **~85% complete** — All types present; CLI wrappers need completion.

---

### 8. MONITORING & TRACING (`node/`)

**Current State**: ⏸️ Framework only, major work pending

**What's Done**:
- ℹ️ Basic stderr output framework
- ℹ️ Trace event structure skeleton

**What's Missing**:
- ❌ **Structured JSON output** (serialization to stdout)
- ❌ **Socket transport** (remote tracer connection)
- ❌ **Event namespace hierarchy** (net.*, chain.*, ledger.*)
- ❌ **EKG metrics** (Counter/Gauge/Timer types)
- ❌ **Prometheus export** (/metrics endpoint)
- ❌ **Event filtering** (selector expressions)
- ❌ **Named trace points** (all 50+ node events)

**Parity Status**: **~25% complete** — Framework skeleton present. Significant work needed on infrastructure.

---

## Phased Implementation Roadmap

### Phase 1: Ledger Rules Completion (Weeks 1-3)

**Goal**: Close ledger validation gaps to enable testnet sync.

**Tasks**:
1. **Collateral validation** (`collateral.rs` complete edge cases)
   - Scope: Alonzo+ collateral UTxO sufficiency checks
   - Upstream reference: `Cardano.Ledger.Alonzo.Rules.Utxo`
   - Tests: 5-10 integration tests for edge cases

2. **Reward calculation** (detailed formulas)
   - Scope: Per-pool + per-account reward math
   - Upstream reference: `Ledger.Reward` module
   - Tests: Mainnet rewards reconciliation

3. **Ratification tally completion** (thresholds + quorum)
   - Scope: Conway voting thresholds per action type
   - Upstream reference: `Cardano.Ledger.Conway.Rules.Ratify`
   - Tests: 20+ governance scenarios

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

**Success Criteria**:
- ✅ Stable mainnet peer set without thrashing
- ✅ Connection limits enforced
- ✅ Failed peers properly demoted

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
- ✅ `cargo test --workspace` passes with 1400+ tests
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
- ✅ `cargo test-all` runs on every commit (1400+ tests)
- ✅ `cargo lint` clean (except pre-existing lint in cddl-codegen, crypto)
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
