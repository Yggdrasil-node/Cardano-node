---
title: Upstream Research
layout: default
parent: Reference
nav_order: 10
---

# Comprehensive Upstream Research: Cardano Haskell Implementation

**Last Updated**: March 26, 2026  
**Focus**: Detailed analysis of official IntersectMBO repositories to identify implementation guidance for the Rust Cardano node.

---

## Overview

This document summarizes the architecture, major components, types, functions, and design patterns found in the official Cardano Haskell implementation across five key repositories:

- **cardano-node** — Primary entry point and orchestration
- **ouroboros-consensus** — Chain selection, block validation, and consensus layer
- **cardano-ledger** — State transitions, ledger rules, transaction processing
- **ouroboros-network** — Network protocols, peer management, multiplexing
- **cardano-base** — Cryptographic and encoding primitives

---

## 1. CORE NODE COMPONENTS

### 1.1 cardano-node Repository Structure

**Primary Purpose**: Integration point for ledger, consensus, and network layers.

**Key Binaries**:
- `cardano-node` — Main daemon for participating in Cardano network
- `cardano-cli` — Command-line client for wallet and node queries
- `cardano-submit-api` — REST API for transaction submission
- `cardano-tracer` — Tracing/monitoring infrastructure
- `cardano-testnet` — Testing framework

**Dependency Graph**:
```
cardano-node
  ├── cardano-api
  │   ├── ouroboros-consensus
  │   ├── ouroboros-network
  │   └── cardano-ledger
  ├── trace-dispatcher / iohk-monitoring-framework
  └── configuration
```

### 1.2 Core Types & Structures

**Node Configuration** (`cardano-node` package):
- `NodeConfigFile` — YAML-based configuration containing:
  - `RequiresNetworkMagic` (enum: RequiresNoMagic | RequiresMagic)
  - `NetworkId` (production vs testnet magic number)
  - `EnableP2P` (boolean for P2P networking)
  - `EnableInboundGovernor` (boolean)
  - `EnableEKG` (monitoring endpoint on localhost:12788)
  - `EnablePrometheus` (metrics export)
  - `ShelleyGenesisFile`, `AlonzoGenesisFile`, `ConwayGenesisFile` (paths)
  - `EnableLogging` (boolean)
  - Database paths for Immutable and Volatile stores
  - `SocketPath` for local query socket

**Genesis Loading** (`cardano-api` / `Gen`):
- `ShelleyGenesis` — Initial Shelley-era parameters
- `AlonzoGenesis` — Plutus cost models  
- `ConwayGenesis` — Conway governance settings
- `GenesisHash` — 32-byte Blake2b-256 hash of genesis block
- Protocol parameters extracted at startup

### 1.3 Node Startup & Orchestration

**Key Responsibilities**:
1. **Configuration Load** — Parse YAML + environment overrides
2. **Genesis File Load** — Deserialize JSON genesis files
3. **Storage Initialization** — Open Immutable/Volatile ChainDB
4. **Network Bootstrap** — Connect to root peers (local/bootstrap/public)
5. **Sync Orchestration** — ChainSync protocol to catch up to tip
6. **Consensus Rules** — Validate headers/blocks via Praos/TPraos
7. **Ledger State** — Apply blocks and track UTxO/epoch state
8. **Tracing** — Emit structured trace events for monitoring

**Trace Events**:
- `Startup` — Node initialization
- `Shutdown` — Graceful termination
- `PeerSelection` — Peer governor decisions
- `ChainSync` — Sync progress (points, slots, blocks)
- `BlockFetch` — Block fetch protocol events
- `Mempool` — Transaction intake/eviction
- `Consensus` — Leader slot, block validation
- `Ledger` — State transitions, validation errors
- `LocalTxSubmission` — User TX submissions

---

## 2. LEDGER IMPLEMENTATION

### 2.1 Repository Structure (cardano-ledger)

**Organization by Era** (Byron → Shelley → Allegra → Mary → Alonzo → Babbage → Conway):

Each era has:
- `formal-spec/` — LaTeX formal specification
- `executable-spec/` — Agda executable model (subset)
- `implementation/` — Haskell code + CDDL serialization
- `test-suite/` — Property-based and property tests

**Key Library Subdirectories** (`libs/`):
- `cardano-ledger-api` — Public types + interfaces
- `cardano-ledger-core` — Shared types across eras
- `cardano-ledger-binary` — CBOR encoding/decoding infrastructure
- `cardano-ledger-blake2b-221` — Blake2b-256 hashing (ledger-specific variant)
- `cardano-ledger-test` — Shared test infrastructure
- Set of per-era libraries (byron, shelley, allegra, mary, alonzo, babbage, conway)

### 2.2 Core Ledger Types

**Block Types** (per era):
- `Block era` — Wrapper type parameterized by era
- `Header era` — Block header (hash, slot, issuer, VRF, nonce, proof)
- `TxSeq era` — Transactions (finger-tree for efficient slicing)
- `AuxiliaryData` — Metadata (Shelley) / Auxiliary scripts (Babbage/Conway)

**Transaction Types** (per era):
- `Tx era` — Full transaction with body + witness set
- `TxBody era` — Transaction inputs/outputs, fees, certificates, withdrawals, metadata, validity interval
- `TxIn` — Transaction input (hash + index)
- `TxOut era` — Transaction output (address + coin + optional datum + optional script reference)
- `WitnessSet era` — VKey witnesses, bootstrap witnesses, scripts, redeemers, datums
- `TxId` — Blake2b-256 of transaction bytes (3-byte prefix + serialized body)

**UTxO Model**:
- `UTxO era` — Map from `TxIn` to `TxOut era`
- Spending (Shelley) → eUTXO (Alonzo+) with Plutus script evaluation

**Address Types**:
- Shelley Address: `Base` (payment + stake), `Enterprise` (payment only), `Pointer` (pointer to delegation)
- Byron Address: Bootstrap address with checksum
- Reward Address: Stake address for rewards

**Certificate Types** (19 variants in CDDL):
- Shelley: `StakePoolRegistration`, `StakePoolRetirement`, `StakeDelegation`, `StakeRegisterationCert`, `StakeUnregistrationCert`
- Conway (tags 7–18): `AuthCommitteeHotKey`, `ResignCommitteeHotKey`, `DRepRegistration`, `DRepUnregistration`, `DRepUpdate`, `VotingProcedures` (if treated as cert), etc.

**Governance (Conway)**:
- `GovAction` — 7 variants: `ParameterChange`, `HardForkInitiation`, `TreasuryWithdrawals`, `NoConfidence`, `UpdateCommittee`, `NewConstitution`, `InfoAction`
- `ProposalProcedure` — Proposer, deposit, return account, anchor, gov action
- `VotingProcedure` — Per-action, per-voter vote
- `Constitution` — Anchor + optional guardrails script hash

### 2.3 Ledger State & Transitions

**LedgerState Structure**:
```haskell
data LedgerState era = LedgerState
  { lsUTxOState :: !UTxOState era
  , lsAccountState :: !AccountState
  , lsCertState :: !CertState era
  , lsEpochBoundaryState :: !EpochBoundaryState
  , ...
  }
```

**UTxOState era**:
- `utxo :: UTxO era` — Current UTXO map
- `deposited :: Coin` — Key & pool deposits
- `fees :: Coin` — Accumulated fees
- `ppups :: Map StakePoolId (ProtocolParameters)` — Update proposals

**AccountState**:
- `treasury :: Coin` — Consolidated funds for governance
- `reserves :: Coin` — Reserve pot

**CertState era**:
- `dState :: DState era` — Delegation state
- `pState :: PState era` — Pool state
- `vState :: VState era` — Voting state (Conway)

**DState era**:
- Delegations: `Map StakeCredential PoolKeyHash`
- Registrations: `Set StakeCredential`
- Rewards: `Map StakeCredential Coin`
- Anchors: `Map StakeCredential (Anchor C_Elem)` (Conway)
- DReps: `Map DRep Coin` (Conway)
- DRep activity tracking (Conway)

**PState era**:
- Pools: `Map StakePoolId PoolParams`
- Retirements: `Map StakePoolId EpochNo`
- Deposits: `Map StakePoolId Coin`

**VState era** (Conway):
- Enacted constitution + committee quorum
- Governance actions under review
- enacted-root tracking per purpose (Purpose lineage)

### 2.4 Ledger Rules (State Transitions)

**Small-Step Semantics Framework**: Used across all eras to model state transitions.

**Key Rules** (per formal-ledger-specifications):

**UTXO Rule**:
- Input validation: UTxOs exist and are unspent
- Output preservation: coin and assets conserved (fees deducted)
- Script validation: Plutus/native scripts evaluated
- Minimum lovelace enforcement: `minUTxO` rule (Alonzo onwards)
- Collateral handling: Locked inputs for script failure coverage (Alonzo onwards)

**CERTS Rule** (Certificates):
- Delegation cert: Update delegation map
- Pool registration: Insert into pool map with deposits
- Pool retirement: Mark pool as retiring after epoch N
- DRep registration/update/unregistration (Conway)
- Committee hot key management (Conway)

**WITHDRAWALS Rule**:
- Validate registered accounts have rewards
- Drain reward account to specified address

**DELEGATION Rule**:
- Update stakes for all delegated credentials
- Compute stake per pool for ranking

**REWARD Rule**:
- Per-epoch rewards calculation using stake snapshots
- Fees returned to pool operators
- Treasury/reserve pot operations

**ENACTMENT Rule** (Conway):
- Proposal ratification → enact gov action
- Purpose lineage tracking (prevents orphans/out-of-order)
- Committee, constitution, parameters updated

**RATIFICATION Rule** (Conway):
- Tally votes (committee threshold, DRep stake weight, SPO proportional)
- Check proposal is accepted by required thresholds

### 2.5 Important Invariants

1. **UTxO Conservation**: All lovelace + assets remain in system (minus fees)
2. **Deposit Tracking**: Key/pool/DRep deposits match LedgerState counters
3. **Delegation Consistency**: Delegation map reflects all stake movements
4. **Slot Ordering**: Blocks processed in ascending slot order within an epoch
5. **Epoch Transitions**: Rewards snapshot, pool retirements, governance actions resolved
6. **Minimum UTxO**: All outputs >= `minUTxO` computed from size + asset type
7. **Plutus Budget**: No script exceeds declared `ExUnits` (CPU + memory)
8. **Witness Sufficiency**: All required keys have valid signatures
9. **Native Script Validity**: Timelocks (InvalidBefore/After) satisfied
10. **Constitution Compliance**: Governance actions satisfy guardrails script (if present)

---

## 3. CONSENSUS LAYER

### 3.1 ouroboros-consensus Repository Structure

**Sublibraries** (dependency order):
1. `ouroboros-consensus-protocol` — Abstract protocol definitions
2. `unstable-ouroboros-consensus-protocol` — Additional protocol machinery
3. `ouroboros-consensus` — Core consensus logic
4. `unstable-ouroboros-consensus` — Unstable consensus APIs
5. `ouroboros-consensus-cardano` — Cardano specialization
6. `ouroboros-consensus-diffusion` — Diffusion + consensus bridge (part of network integration)

**Key Executables**:
- `db-analyser` — Inspect ChainDB state, validate blocks, dump snapshots
- `db-synthesizer` — Generate synthetic chains for benchmarking
- `db-truncater` — Convert volatile suffix to immutable
- `immdb-server` — Serve blocks over node-to-node protocol
- `gen-header` — Generate + validate Praos headers
- `snapshot-converter` — Convert between storage backends (LMDB ↔ InMemory ↔ LSM)

### 3.2 Consensus Core Types

**SecurityParam (k)**: Ouroboros parameter determining rollback window (typically k=2160 slots = ~1 day).

**SlotNo**: Absolute slot number from genesis (0-indexed).

**EpochNo**: Epoch number since genesis.

**BlockNo**: Absolute block number from genesis (0-indexed).

**ChainHash**:
- `GenesisHash` (initial state hash)
- `BlockHash` (Blake2b-256 of header bytes)

**Point**:
- `GenesisPoint` — Block 0 reference
- `BlockPoint BlockNo ChainHash` — (BlockNo, BlockHash) pair for chain reference

**Tip**:
- Holds current chain tip: `Point`, `BlockNo`, `SlotNo`, `HeaderHash`

**HeaderHash**:
- Blake2b-256 hash of header bytes
- Different computation per era (Shelley 15 elements vs Babbage/Conway 14)

**Header era**:
- `headerHash :: Header era → HeaderHash`
- `headerSlot :: Header era → SlotNo`
- `headerBlockNo :: Header era → BlockNo`
- `headerPrevHash :: Header era → ChainHash`
- `headerIssuer :: Header era → StakePoolId`
- VRF output + proof
- Operational certificate (hot key, sequence number, KES period, signature)

**Block era**:
- Header + transactions + auxiliary data
- `blockHash = headerHash . blockHeader`

### 3.3 Chain Selection & Protocol Rules

**Ouroboros Praos** (Shelley through Babbage):
- **Leader Selection**: Stake-weighted VRF lottery per slot
- **VRF** (Verifiable Random Function):
  - Private key: pool's operational key
  - Output: 64-byte value
  - Path: VRF_output < threshold(stake_fraction) → block leader
  - Threshold: `active_slot_coeff * relative_stake`
- **Block Production**: Leader creates block with:
  - VRF output + proof
  - Header nonce (Blake2b-256 of VRF output)
  - Operational certificate (KES signature)
  - Body (transactions)

**Ouroboros TPraos** (Praos with Threshold + Timelock, Babbage/Conway):
- Single VRF result (not separate nonce/leader VRF)
- Simpler block issuance

**Block Validation Rules**:
1. **Slot Ordering**: `blockSlot(B) > blockSlot(prev_block)`
2. **Chain Continuity**: `blockPrevHash(B) = headerHash(prev_header)`
3. **Issuer Check**: Pool stake > 0 at snapshot
4. **VRF Check**: VRF proof verifies; output < threshold
5. **OpCert Check**: Operational certificate valid for KES period
6. **Header Signature**: Operational key signature over header bytes
7. **Body Hash**: Declared body hash matches computed hash
8. **Minimum UTxO**: All outputs >= `minUTxO`
9. **Fee Sufficiency**: Declared fees >= computed fees
10. **Plutus Budget**: All scripts within declared `ExUnits`

**Chain Selection Rule** (Ouroboros algorithm):
- **Longest Chain**: Select fork with most blocks
- **Tiebreaker (Density)**: If equal length, select fork with densest blocks in `2k` window
- **Stable Prefix**: Blocks older than `3k` slots cannot fork (immutable)
- **Rollback**: Limited to `k` blocks maximum

### 3.4 Epoch Nonce Evolution

**Nonce State Machine** (`UPDN` rule):
- Maintains nonce for leader selection in current/next epoch
- Updates with VRF outputs from block issuers
- Hash accumulator: `sha256(prev_nonce || vrf_output)`

**Key State**:
- `non_evol_nonce` — Non-evolving nonce (constant per epoch, set at boundary)
- `evol_nonce` — Evolving nonce (updated per block)
- Epoch transition: `non_evol_nonce ← evol_nonce`

### 3.5 Ledger-Consensus Bridge

**Verification Workflow**:
1. Consensus validates header (VRF, OpCert, signature)
2. Ledger receives full block
3. Ledger validates transactions (UTxO, fees, scripts)
4. Ledger applies block (updates UTxO, epoch state, rewards)
5. New ledger state returned with governance/epoch updates

**HeaderBody** (data structure bridging ledger ↔ consensus):
```haskell
data HeaderBody = HeaderBody
  { blockNumber :: BlockNo
  , slot :: SlotNo
  , issuerVkey :: VerificationKey ColdKeyRole
  , vrfVkey :: VerificationKey VRFKeyRole
  , leaderVrfOutput :: VRFOutput
  , leaderVrfProof :: VRFProof
  , nonceVrfOutput :: Maybe VRFOutput  -- TPraos only
  , nonceVrfProof :: Maybe VRFProof    -- TPraos only
  , blockBodySize :: Word32
  , blockBodyHash :: BlockBodyHash
  , operationalCert :: OperationalCert
  }
```

### 3.6 Storage & Snapshots

**ChainDB** (Immutable + Volatile):
- **Immutable Suffix**: Blocks older than `3k` slots, written sequentially, never modified
- **Volatile Prefix**: Recent blocks, updated as chain extends/reorg'd
- **Checkpointing**: Periodic snapshots of ledger state for fast recovery

**Snapshot Types**:
- **Ledger Snapshot**: LedgerState serialized at point (slot, hash)
- **Utxo-HD Snapshot**: Full ledger state + index structure for query
- **Block Snapshot**: Range of blocks with metadata

**Key Recovery**:
- On restart, find latest immutable ledger snapshot
- Replay volatile suffix blocks to recover current state
- Limit replay window to avoid long sync times

---

## 4. NETWORK LAYER

### 4.1 ouroboros-network Repository Structure

**Sublibraries**:
1. `network-mux` — General-purpose network multiplexer (Mux)
2. `ouroboros-network-api` — Shared types
3. `ouroboros-network-framework` — Low-level components (snockets, connection manager, inbound governor, handshake)
4. `ouroboros-network-protocols` — All mini-protocol implementations
5. `ouroboros-network` — Top-level integration + outbound governor
6. `cardano-diffusion` — Cardano-specific glue
7. `cardano-ping`, `cardano-client`, `ntp-client` — User tools
8. `monoidal-synchronisation` — Synchronization primitives

### 4.2 Network Architecture

**Two Flavours**:
1. **Node-to-Node** (N2N): Between peer Cardano nodes
2. **Node-to-Client** (N2C): Between node and local/remote clients (wallets, indexers)

**Mini-Protocols** (N2N):
1. **Handshake** — Negotiate versions + network magic
2. **ChainSync** — Synchronize chain tip
3. **BlockFetch** — Fetch blocks in bulk
4. **TxSubmission** — Relay unconfirmed transactions
5. **KeepAlive** — Heartbeat (NOP frames)
6. **PeerSharing** — Exchange peer addresses (P2P mode)

**Mini-Protocols** (N2C):
1. **Handshake** — Negotiate versions
2. **ChainSync** — Synchronize chain tip
3. **StateQuery** — Query ledger state (UTxOs, epoch, etc.)
4. **TxSubmission** — Submit transactions
5. **TxMonitoring** — Observe mempool

### 4.3 Multiplexing (network-mux)

**Mux Type**:
- Supports full-duplex multiplexing over single TCP connection
- Fair scheduling across mini-protocols
- Per-protocol message size limits + timeouts

**Mux Frame Format** (8-byte header + payload):
```
┌─────────────┬──────────┬────────────┐
│ MiniProtId  │ Reserved │ PayloadLen │
│ (2 bytes)   │ (2 byte) │ (4 bytes)  │
└─────────────┴──────────┴────────────┘
```

**Flow Control**:
- Ingress queue per mini-protocol
- Egress queue per mini-protocol
- Backpressure if buffer full
- Timeouts enforced per message

### 4.4 ChainSync Protocol (Detailed)

**State Machine** (client side):
```
Idle
  ↓ (SendMsgFindIntersect)
Intersecting → Intersecting (collision, retry)
  ↓ (RecvMsgIntersectFound)
Syncing
  ↓ (SendMsgRequestNext)
Awaiting
  ↓ (RecvMsgRollForward | RecvMsgRollBackward)
Syncing
  ↓ loop or Done
```

**Key Messages**:
- `MsgFindIntersect [Point]` — Find common ancestor
- `MsgIntersectFound Point` — Ancestor found
- `MsgIntersectNotFound` — No common ancestor (fork too long)
- `MsgRequestNext` — Ask for successor block
- `MsgRollForward Header Tip` — New block + new tip
- `MsgRollBackward Point Tip` — Rollback to point + new tip
- `MsgDone` — Terminate protocol

**Intersection Logic**:
- Client sends recent points (tip, tip-k, tip-2k, ..., genesis)
- Server finds first that exists in its chain
- Returns `MsgIntersectFound` or `MsgIntersectNotFound`

### 4.5 BlockFetch Protocol

**Pattern**: Client requests batches of block headers/bodies, server streams them.

**Key Messages**:
- `MsgRequestRange BlockNo BlockNo` — Request blocks in range
- `MsgStartBatch BlockNo` — Server acknowledges start
- `MsgNoBlocks` — Range not available
- `MsgBlock Block` — Individual block streamed
- `MsgBatchDone` — Batch complete

**Optimizations**:
- Pipelined requests (multiple ranges in flight)
- Efficient streaming (doesn't wait for full batch in memory)
- Per-peer fairness enforced by mux

### 4.6 TxSubmission Protocol

**Pattern**: Client pushes transactions; server acknowledges + provides feedack.

**Known Txs Map**: Server maintains mempool TxIds to avoid resending.

**Key Messages**:
- `MsgInit Num Num` — (txs to send, txs I want from you)
- `MsgRequestTxIds RequestAmount` — Ask for next batch
- `MsgReplyTxIds [(TxId, TxSize)]` — TxIds + sizes
- `MsgRequestTxs [TxId]` — Fetch specific txs
- `MsgReplyTxs [Tx]` — Transaction payload
- `MsgMempool Mempool` — Your mempool state

### 4.7 Peer Management

**PeerSource Enum**:
- `PeerSourceLocalRoot` — Static config + dynamic local override
- `PeerSourcePublicRoot` — Root peers (bootstrap)
- `PeerSourcePeerSharing` — Discovered via peer sharing protocol
- (Additional sources in Rust node: Ledger, BigLedger, etc.)

**PeerStatus**:
- `Cold` — No connection
- `Warm` — Connection established but no active mini-protocols
- `Hot` — Active mini-protocol (ChainSync, TxSubmission)
- `Graylist` — Temporarily deferred after failure
- `Blacklist` — Permanently banned

**Outbound Governor**:
- Targets (e.g., 2 Hot, 13 Warm, unknown Cold)
- Decisions: Promote/demote peers based on targets
- Churn: Periodic re-evaluation to refresh peer set
- Valency enforcement: max N peers per source

**Inbound Governor**:
- Rate limits inbound connections
- Rejects excess inbound from same peer
- Slot-based fairness for new inbound

### 4.8 Handshake Protocol

**Negotiation Workflow**:
1. Client sends list of versions + network magic
2. Server chooses highest common version + magic
3. Both sides receive version data (node version, network capabilities)
4. Mini-protocols can now proceed

**Version Negotiation**:
- Map from version number to callbacks + version data
- Enables protocol evolution without breaking old clients
- Each version can have different message format

---

## 5. MEMPOOL IMPLEMENTATION

### 5.1 Mempool Role

**Purpose**: Transaction admission, ordering, and relay.

**Entry Points**:
1. Local submission (user via TxSubmission N2C)
2. Peer relay (from TxSubmission N2N)

**Exit Points**:
1. Included in block (confirmed)
2. Evicted (TTL expired, fee too low, replaced)
3. Relayed to peers

### 5.2 Mempool State Management

**TicketNo** (Haskell mempool):
- Total unique transactions seen (ever increasing counter)
- Used for ordering + age tracking

**TxSeq** (Finger-tree):
- Efficient data structure for transaction ordering
- Supports slicing by fee/size
- Can efficiently extract first N transactions

**Key Tracking**:
- `TicketNo` assigned on admission
- `TxSize` tracked for bandwidth fair-share  
- `TxFee` used for fee-based eviction
- `TxId` for deduplication + relay state

### 5.3 Mempool Operations

**InsertTx**:
- Check duplicate by TxId
- Validate syntax + basic script complexity
- Check fee > minFeeA * size + minFeeB
- Track TicketNo for ordering
- May evict lower-fee tx if capacity hit

**RemoveTx**:
- By TxId (confirmed or timeout)
- By TicketNo (age-based eviction)

**SnapshotTxs**:
- Return ordered list for block production
- Typically first N txs (by fee priority)

**RelayTx**:
- Mark Tx as "available to peer" (SnocList in some peers)
- Track which peers have seen it (to avoid re-relay)

### 5.4 Admission Criteria

**Static Validation**:
1. Syntax: Parseable CBOR
2. Size: Within protocol limits (~16KB)
3. Network: Tx not too old (TTL validation)
4. Fee: >= minFeeA * bytes + minFeeB

**Dynamic Validation**:
1. UTxO check: All inputs exist
2. Fee sufficiency: Fees cover declared cost
3. Script validation: No invalid scripts (Plutus validation deferred to block)
4. Duplicate: Not already confirmed/pending

**Rejection Codes**:
- `TextEncodingError` — Unparseable
- `TxTooLarge` — Exceeds size limit
- `FeesTooSmall` — Fee insufficient
- `TxOutsideValidityInterval` — TTL exceeded
- `UnknownInput` — Input UTxO not found
- `DuplicateTxId` — Already processed

---

## 6. STORAGE LAYER

### 6.1 Storage Abstraction (ouroboros-consensus)

**Dual-Store Model**:
1. **Immutable Store**: Append-only, indexed by absolute block number
   - Blocks older than `3k` slots
   - Sequential file layout for efficiency
   - Never modified once written
   
2. **Volatile Store**: In-memory + file-backed prefix
   - Recent blocks (< `k` slots old)
   - Can be reorganized on chain reorg
   - Lost on node restart (re-synced)

### 6.2 ChainDB Interface

**Key Methods**:
- `getBlock :: Point → Maybe Block` — Lookup block by point
- `getTip :: IO Tip` — Current chain tip
- `getImmutableLedger :: IO LedgerSnapshot` — Latest immutable ledger state
- `applyBlock :: Block → IO LedgerState` — Validate + apply block
- `rollback :: Point → IO ()` — Reorg to previous point
- `addBlock :: Block → IO ()` — Add new block + update tip
- `iterBlocks :: Point → Point → (Block → IO a) → IO [a]` — Iterate range

### 6.3 Ledger Store Interface

**Key Methods**:
- `storeLedgerState :: LedgerState → IO ()` — Checkpoint ledger
- `loadLedgerState :: Point → IO (Maybe LedgerState)` — Recover ledger
- `purgeAncientLedgerStates :: ()` — GC old checkpoints

**Snapshot Policy**:
- On-disk ledger snapshots at fixed block intervals (e.g., every 2160 blocks = k)
- Allows fast recovery without replaying all blocks
- Indexed by (BlockNo, BlockHash) pairs

### 6.4 DB Analyzers & Utilities

**db-analyser**:
- Scan entire ChainDB
- Validate all blocks (consensus + ledger rules)
- Compute statistics (slots, fees, txs, governance actions)
- Extract ledger state at specific points
- Dump block ranges for analysis

**db-truncater**:
- Convert volatile blocks to immutable
- Useful for cleanup after long uptime

**snapshot-converter**:
- Convert between storage backends (LMDB/LSM/In-Memory)
- Useful for performance tuning

---

## 7. CLI & CONFIGURATION

### 7.1 Configuration File Structure

**YAML Format** (network-specific):
```yaml
Testnetwork: false|true  # Testnet magic
RequiresNetworkMagic: RequiresMagic|RequiresNoMagic

GenesisFile: genesis.json
ShelleyGenesisFile: shelley-genesis.json
AlonzoGenesisFile: alonzo-genesis.json
ConwayGenesisFile: conway-genesis.json

DBPath: db/
SocketPath: /tmp/node.socket

EnableP2P: true|false
EnableInboundGovernor: true|false
PeerSharing: true|false

EnableLogging: true|false
LogOutputFile: logs/
TracingOn: true|false
EnableEKG: true|false
EnablePrometheus: true|false

Port: 3001
HostAddr: 127.0.0.1
```

**Genesis Files** (JSON):

```json
shelley-genesis.json {
  "systemStart": "2020-07-28T21:44:51Z",
  "networkMagic": 764824073,
  "networkId": { "tag": "Mainnet" },
  "activeSlotsCoeff": 0.05,
  "protocolParams": {
    "minFeeA": 44,
    "minFeeB": 155381,
    "maxTxSize": 16384,
    "maxBlockBodySize": 65536,
    "keyDeposit": 2000000,
    "poolDeposit": 500000000,
    "minPoolCost": 340000000,
    "maxBlockHeaderSize": 1100,
    "maxTxExecutionUnits": { "mem": ..., "steps": ... },
    ...
  }
}
```

### 7.2 Command-Line Interface

**cardano-node CLI**:
```bash
cardano-node run \
  --config node.json \
  --database-path db/ \
  --socket-path /tmp/node.socket \
  --port 3001
```

**cardano-cli** (user-facing tool):
```bash
cardano-cli transaction build \
  --tx-in <txin> \
  --tx-out <address>+<lovelace> \
  --change-address <address> \
  --fee <fee> \
  --out-file tx.json

cardano-cli transaction sign \
  --tx-body-file tx.json \
  --signing-key-file key.skey \
  --out-file tx-signed.json

cardano-cli transaction submit \
  --tx-file tx-signed.json \
  --mainnet
```

### 7.3 Query Interfaces

**LocalStateQuery** (N2C protocol):
- Query UTxOs by address
- Query epoch + current slot
- Query protocol parameters
- Query stake distribution
- Query pool metadata
- Query governance state (Conway)

**Example** (via CLI):
```bash
cardano-cli query utxo \
  --address $(cat payment.addr) \
  --mainnet

cardano-cli query tip --mainnet
```

---

## 8. TRACING & MONITORING

### 8.1 Tracing Infrastructure

**Structured Tracing** (via `trace-dispatcher` / `iohk-monitoring`):
- Event-based logging with key-value metadata
- Severity levels: Silence, Critical, Error, Warning, Notice, Info, Debug
- Multiple backends: JSON, text files, EKG (metrics), Prometheus

### 8.2 Key Trace Events

**Category: ChainSync**
- `TraceChainSyncClientSeqPt` (received point)
- `TraceChainSyncHeaderSelection` (new header/rollback)
- `TraceChainSyncIntersection` (found intersection)

**Category: BlockFetch**
- `TraceFetchedBlock` (block received)
- `TraceFetchedHeader` (header received)
- `TraceBlockFetchClientState` (state change)

**Category: TxSubmission**
- `TraceTxSubmissionInbound` (peer submitted tx)
- `TraceTxSubmissionOutbound` (sent tx to peer)
- `TraceTxInMempool` (tx accepted to mempool)

**Category: Consensus**
- `TraceBlockIsInvalid` (block rejected)
- `TraceLeadershipChecksFailed` (not eligible leader)
- `TraceSelectingChainFork` (chain selection decision)

**Category: Ledger**
- `TraceApplyBlockRules` (ledger rule outcome)
- `TraceValidationFailure` (UTxO/fee/script error)
- `TraceUTxOState` (UTXO map changes)

**Category: LocalTxSubmission**
- `TraceLocalTxSubmissionEndpoint` (socket operations)
- `TraceLocalTxSubmissionServer` (TX processed)

**Category: PeerSelection**
- `TracePeerSelection` (peer governor decision)
- `TraceConnectionUpgraded` (cold→warm)
- `TraceConnectionDowngraded` (hot→warm)

### 8.3 Metrics Export

**EKG Endpoint** (localhost:12788 by default):
- HTTP server serving JSON metrics
- Real-time CPU, memory, GC stats
- Block height, slot, peers connected
- Mempool size, transaction submission rate

**Prometheus Export**:
- OpenMetrics format
- Scrape-based collection
- Same metrics as EKG + custom counters

**Key Metrics**:
- `cardano.node.chain.length` (blocks)
- `cardano.node.slot` (current slot)
- `cardano.node.peers.connected` (peer count)
- `cardano.node.txs.submitted` (transaction rate)
- `cardano.node.mempool.size` (pending txs)

---

## 9. CROSS-SUBSYSTEM DEPENDENCIES

### 9.1 Data Flow Architecture

```
User TX          Genesis Files        Peer Network
    │                  │                    │
    v                  v                    v
CardanoCLI ────► Genesis Load ◄────── NetworkStack
    │                  │                    │
    └──────────────────┼────────────────────┘
                       v
                Node Startup
                       │
        ┌──────────────┼──────────────┐
        v              v              v
   Storage      LedgerState      NetworkGov
   (ChainDB)    (Initial)        (PeerSelection)
        │              │              │
        │              └──────┬───────┘
        └───────────────┬─────┘
                        v
                   SyncService
                        │
        ┌───────────────┼───────────────┐
        v               v               v
   ChainSync      BlockFetch      TxSubmission
   (Protocol)     (Protocol)      (Protocol)
        │               │               │
        └───────────────┼───────────────┘
                        v
                  HeaderValidation
                  + Consensus Rules
                        │
                        v
                  BlockValidation
                  + Ledger Rules
                        │
                        v
                  State Transition
                  (LedgerState Update)
                        │
                        v
                  StoreBlocks
                  (ChainDB)
                        │
                        v
                monitoring/
                  tracing
```

### 9.2 Type Dependencies

**cardano-ledger** provides:
- `Block era`, `Header era`, `Tx era`, `TxOut era`
- `LedgerState era` (with all state types)
- `Point`, `Tip`, `BlockHash`, `BlockNo`
- `Coin`, `Value`, `TxId`
- `Address`, `StakeCredential`, `Certificate`
- `PlutusScript`, `Script`, `ExUnits`

**ouroboros-consensus** provides:
- `SecurityParam`, `ChainState`, `ChainDB`
- `HeaderBody` (consensus-specific header types)
- Consensus rule functions
- Storage interfaces

**ouroboros-network** provides:
- `ConnectionManager`, `PeerRegistry`, `PeerState`
- Mini-protocol implementations (ChainSync, BlockFetch, TxSubmission)
- `Mux`, versioning negotiation
- Outbound governor + inbound governor

**cardano-node** implements:
- Node orchestration (startup, shutdown)
- SyncService (orchestrates ChainSync + BlockFetch + TxSubmission)
- LocalStateQuery server
- LocalTxSubmission server
- Integration glue

---

## 10. KEY ALGORITHMS & PATTERNS

### 10.1 Chain Extension Workflow

**Pseudocode** (on-sync):
```
For each point from tip, working backward:
  1. Send MsgFindIntersect([tip, tip-k, tip-2k, ..., genesis])
  2. If MsgIntersectFound(point):
       - Start at point
       - Loop: RequestNext() → RollForward(header) or RollBackward()
  3. Upon RollForward(header):
       - Validate header (consensus rules)
       - Request BlockFetch(blockNo)
       - Upon Block received:
           - Validate block (ledger rules)
           - Apply to LedgerState (outputs new state)
           - Store in ChainDB
           - Update Tip
           - Broadcast to peers (if appropriate)
```

### 10.2 Epoch Boundary Transition

**Pseudocode** (at epoch boundary):
```
On slot crossing epoch boundary:
  1. Snapshot current delegations → DelegState snapshot
  2. Compute per-pool stakes from snapshot
  3. Calculate rewards pot (transaction fees from epoch)
  4. Compute per-pool reward per Lovelace staked
  5. Distribute rewards to stake addresses
  6. Update treasury + reserves
  7. Retire pools marked for retirement
  8. Advance nonce (non_evol_nonce ← evol_nonce)
  9. Update governance state (expire proposals, remove inactive DReps)
  10. Mark for checkpoint + snapshot
```

### 10.3 Block Validation Sequence

**Consensus Layer** (header validation):
```
1. Check blockSlot > prev_blockSlot
2. Check blockPrevHash == prev_headerHash
3. Extract VRF output + proof from header
4. Verify VRF proof (issuer VKey + VRF proof → output)  
5. Check VRF_output < threshold(stake_fraction)
6. Extract operational certificate
7. Verify certificate signature
8. Check KES period validity
9. Verify header signature (issuer key)
```

**Ledger Layer** (body validation):
```
1. For each transaction:
     a. Check inputs exist in UTxO
     b. Compute min fee from tx size
     c. Check declared fee >= min fee
     d. If Plutus scripts present:
        - For each script: evaluate within budget
        - Mark failed scripts → consume collateral
     e. Check native scripts valid (timelock checks)
     f. Compute outputs value hash
2. Check body hash matches header declaration
3. Apply all transactions (remove inputs, add outputs, update delegations, etc.)
4. Check total fees collected
5. Process certificates (delegations, pool ops, governance)
6. Process governance votes
7. If crossing epoch boundary: Apply epoch boundary rules
```

---

## 11. WHAT'S IMPLEMENTED vs. WHAT REMAINS (FOR RUST NODE)

### 11.1 Fully Implemented Upstream

✅ **Ledger Core**:
- All 7 eras (Byron → Conway) with era-specific types
- UTXO state transitions
- Governance proposals + voting + ratification (Conway)
- Plutus evaluation (`PlutusInterpreter`)
- Formal ledger specifications (machine-checked for Conway)

✅ **Consensus**:
- Ouroboros Praos + TPraos leader election
- VRF verification (libsodium-vrf)
- OperationalCert (KES signature)
- Chain selection (longest chain + density)
- ChainDB (immutable + volatile stores)
- Block validation rules

✅ **Network**:
- All 6 mini-protocols (Handshake, ChainSync, BlockFetch, TxSubmission, KeepAlive, PeerSharing)
- Multiplexing (Mux)
- Peer discovery + peer sharing
- Outbound + inbound governors
- Typed protocols (session-type encoding)

✅ **Storage**:
- LMDB backend (key-value store)
- LSM tree backend (write-optimized)
- Snapshot + recovery

✅ **CLI & Configuration**:
- YAML configuration files
- Genesis loading + parameter extraction
- LocalStateQuery protocol
- LocalTxSubmission protocol
- cardano-cli tool

### 11.2 Partially Implemented / Specific to Haskell

⚠️ **Specific to Haskell Implementation**:
- `finger-tree` data structure for transaction ordering
- `SnocList` for peer state tracking
- GHC-specific memory/garbage collection optimizations
- Haskell exception handling + STM (Software Transactional Memory)
- iohk-monitoring / trace-dispatcher (can be replaced by Rust tracing)

⚠️ **Database Backends**:
- Upstream uses LMDB (C library, requires FFI)
- Upstream also offers LSM tree (Haskell)
- **Rust node should evaluate**:
  - Pure Rust  not RocksDB that is C, but a custom implementation in Pure Rust
  - Avoid FFI-backed LMDB to stay typesafe

### 11.3 Missing / Incomplete Upstream Features

❌ **Active Work in Progress**:
- MPE-HD (Multi-Pool Epoch Heads) — distributed ledger indexing (early stage)
- UTXO-HD — on-disk UTXO indexing for large wallets (being generalized)

❌ **Not Yet Implemented** (Rust must build from spec):
- Pure Rust crypto (cardano-base provides Haskell bindings over C/Rust libs)
- Byron backward-compatible binary block format (use cardano-base as reference)
- Some experimental DoS mitigations (latest PRs)

---

## 12. KEY REFERENCES & DOCUMENTATION

### 12.1 Formal Specifications

- **Cardano Ledger Formal Specs**: https://github.com/IntersectMBO/cardano-ledger/releases
  - Byron Ledger Spec PDF
  - Shelley Ledger + Delegation Design
  - Mary (Multi-Asset) Spec
  - Alonzo (eUTXO) + Babbage + Conway

- **Formal Ledger Specifications (Machine-Verified)**: https://intersectmbo.github.io/formal-ledger-specifications/site
  - Agda proofs for Conway (complete)
  - Earlier eras (partial)

- **Small-Step Semantics Framework**: https://github.com/IntersectMBO/cardano-ledger/releases/download/.../small-step-semantics.pdf
  - Notation + style guide for ledger rules

### 12.2 Protocol Specifications

- **Ouroboros Praos Paper**: https://eprint.iacr.org/2017/573.pdf
  - Leader election via VRF lottery
  - Chain selection + rollback limits

- **Network Specification**: https://ouroboros-network.cardano.intersectmbo.org/pdfs/network-spec
  - Mini-protocol definitions
  - Serialization formats (CBOR)
  - Multiplexing + versioning

- **Network Design Document**: https://ouroboros-network.cardano.intersectmbo.org/pdfs/network-design
  - High-level architecture + constraints

### 12.3 API Documentation

- **Haddock (cardano-ledger)**: https://cardano-ledger.cardano.intersectmbo.org/
- **Haddock (ouroboros-consensus)**: https://ouroboros-consensus.cardano.intersectmbo.org/haddocks/
- **Haddock (ouroboros-network)**: https://ouroboros-network.cardano.intersectmbo.org/

### 12.4 Developer Resources

- **Cardano Developer Portal**: https://developers.cardano.org/
- **Cardano Engineering Handbook**: https://input-output-hk.github.io/cardano-engineering-handbook/
- **GitHub Issues**: Active discussion of design decisions + bug fixes

---

## 13. RECOMMENDED IMPLEMENTATION PRIORITIES (FOR RUST NODE)

### Phase 1: Foundation
1. ✅ Pure-Rust crypto (done in upstream cardano-base analysis)
2. ✅ CBOR encoding/decoding (ledger types)
3. ✅ Byron + Shelley block + transaction types
4. ✅ Basic ledger state + UTxO updates

### Phase 2: Consensus & Validation
1. Praos header validation (VRF, OpCert)
2. Ledger rule validation (fees, UTxO conservation)
3. ChainDB + storage interfaces
4. Chain selection algorithm

### Phase 3: Network & Sync
1. Multiplexing + typed protocols
2. ChainSync + BlockFetch implementations
3. Peer management + governor
4. TxSubmission protocol

### Phase 4: Advanced Features
1. Plutus script evaluation (CEK machine)
2. Governance (Conway era) proposals + ratification
3. Epoch boundary processing + rewards
4. LocalStateQuery + LocalTxSubmission APIs

### Phase 5: Production Hardening
1. Comprehensive test suite (property-based, integration)
2. Performance optimization (storage, syncing)
3. Monitoring + tracing infrastructure
4. CLI + configuration management

---

## 14. CONCLUSION

The upstream Haskell implementation provides a comprehensive, formal-specification-backed reference for implementing a Cardano node. Key insights:

1. **Modular Design**: Clear separation between ledger (state machines), consensus (protocol + chain selection), and network (peer + mux management).

2. **Type Safety**: Haskell's type system enforces many invariants at compile time; Rust's ownership model can achieve similar rigor.

3. **Formal Specs**: All ledger rules are formally specified; Rust node should mirror these exactly for parity.

4. **Incremental Milestones**: 7 eras can be implemented iteratively; Byron alone has significant complexity (envelope format, witness structure).

5. **Cryptographic Parity**: VRF, Ed25519, KES, Blake2b must match upstream exactly; consider leveraging cardano-base as a reference.

6. **Testing**: Comprehensive test suites (golden tests, property-based, integration) are essential for ensuring parity.

7. **Monitoring**: Structured tracing from the start enables debugging + production observability.

The Rust implementation should prioritize:
- Exact type alignment with upstream (naming, structure, semantics)
- Comprehensive test coverage against upstream test vectors
- Formal verification where possible
- Pure Rust implementations (no FFI except where unavoidable)
- Clear documentation of any design divergences from upstream

---

**Document Status**: Reference implementation complete for all 5 major repositories. Ready for deep-dive implementation work in any subsystem.
