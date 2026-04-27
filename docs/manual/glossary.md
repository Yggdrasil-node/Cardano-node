---
title: Glossary
layout: default
parent: User Manual
nav_order: 12
---

# Glossary

Cardano-specific terminology used throughout this manual and the code.

## Cryptography

**Blake2b-224** — 28-byte hash function. Used for pool key hashes, script hashes, addresses (key portion).

**Blake2b-256** — 32-byte hash function. Used for transaction IDs, block IDs, header hashes, script context digests.

**BLS12-381** — Pairing-friendly elliptic curve. Used by Plutus V3 builtins (`Bls12_381_G1_*`, `Bls12_381_G2_*`, `Bls12_381_MillerLoop`, `FinalVerify`) per CIP-0381.

**Ed25519** — Edwards-curve signature scheme. Used for transaction witnesses, OpCert signatures, KES leaf signatures.

**KES (Key Evolving Signature)** — A signature scheme that periodically destroys old signing keys so a compromise of the current key cannot forge past blocks. Yggdrasil uses Sum-KES with depth 6 (62 evolutions). On mainnet a KES key is valid for ~90 days.

**OpCert (Operational Certificate)** — A signed certificate from the cold key authorising the KES key to forge blocks for a window of slots. Sequence number must monotonically increase per pool.

**VRF (Verifiable Random Function)** — Provides each block with a verifiable pseudo-random value used for both leader election and the per-block contribution to the epoch nonce.

## Consensus and chain

**Active slot coefficient (`f`)** — Probability that any given slot has at least one block. Mainnet: `0.05`. So roughly 1 in 20 slots produces a block.

**Cardano Era** — A protocol version with its own ledger format. In order: Byron, Shelley, Allegra, Mary, Alonzo, Babbage, Conway. Era boundaries are hard forks; a chain is the sequence of all eras' blocks.

**ChainDB** — Yggdrasil's chain database: immutable region (stable blocks, append-only) + volatile region (recent K blocks, may roll back) + ledger snapshots (periodic state checkpoints).

**ChainSync** — One of the five mini-protocols. Used to follow a peer's chain: client requests next header or next rollback. State machine: `MsgFindIntersect`, `MsgRequestNext`, `MsgRollForward`, `MsgRollBackward`, `MsgAwaitReply`.

**Epoch** — A unit of time during which stake distribution and protocol parameters are fixed. Mainnet: 432,000 slots = 5 days.

**Epoch boundary** — The slot where one epoch ends and the next begins. Triggers stake snapshot rotation, reward calculation, and a number of book-keeping operations.

**Fork** — Two competing chains with a common ancestor. Resolved by the chain-selection rule (longer chain wins, with VRF-based tiebreakers).

**Genesis** — The initial chain state. For each era there is a genesis JSON: Byron, Shelley, Alonzo, Conway. Yggdrasil hashes each at startup and verifies against pinned values.

**Hard fork** — An era transition. Coordinated by an on-chain proposal in the previous era's governance system.

**Header** — Block metadata: slot, prev hash, body hash, VRF, KES signature, OpCert. ChainSync ships headers; BlockFetch ships bodies.

**Immutable region** — Blocks past the security parameter `k` from the current tip. By assumption (Praos liveness), these will not roll back.

**Praos** — The Ouroboros protocol variant Cardano runs since the Shelley era. Successor to TPraos. Uses VRF leader election plus epoch-nonce evolution.

**Roll forward** — Apply the next block in chain order.

**Roll backward** — Undo recent blocks because the peer's chain has switched to a different fork.

**Security parameter (`k`)** — Mainnet: 2160. Beyond `3k/f` slots, the chain is considered final.

**Slot** — One second of network time. Slot 0 corresponds to the network's epoch start time (preset-specific). Mainnet slot 0 = 2017-09-23 21:44:51 UTC.

**Slot leader** — The node entitled to produce a block in a given slot, determined by VRF on the slot index plus the epoch nonce, weighted by stake.

**Stable block** — A block in the immutable region. In Yggdrasil's storage model, stable blocks live in the immutable store.

**TPraos** — The "Transitional Praos" protocol used in the Shelley era. Replaced by Praos in Babbage.

**Volatile region** — The most recent K=2160 blocks, which may roll back if the chain switches forks.

## Network

**Bootstrap peer** — IOG-curated entry point used to seed peer discovery. Defined in `topology.json`.

**Big-ledger peer** — A peer from the largest stake pools, used as a high-quality discovery source during bootstrap or recovery.

**Cold peer** — A known peer with no current connection.

**ConnectionManager (CM)** — The component that tracks the lifecycle of every connection (inbound + outbound), handles graceful shutdown, and applies rate limits.

**DataFlow** — Per-connection mode: `Unidirectional` (one-way) or `Duplex` (both directions over the same TCP socket — saves a connection slot).

**Diffusion mode** — Per-local-root setting: `InitiatorAndResponderDiffusionMode` (full duplex) or `InitiatorOnlyDiffusionMode` (outbound only — used by block producers behind NAT).

**Established peer** — Has an active TCP connection and completed handshake.

**Governor** — The component that decides which peers to promote or demote among cold/warm/hot states. Its decisions are translated into `CmAction`s and applied by the connection manager.

**Hot peer** — Active for sync (ChainSync + BlockFetch). Subset of warm.

**Inbound governor** — Per-connection state machine for inbound peers, mirroring upstream `Ouroboros.Network.InboundGovernor`.

**Ledger peer** — A peer learned from on-chain stake-pool relay registrations. Activated via `useLedgerAfterSlot`.

**Local root** — A peer the operator manually configured. Trusted, prioritised in sensitive mode.

**Mini-protocol** — One of five sub-protocols multiplexed over a connection: ChainSync (2), BlockFetch (3), TxSubmission (4), KeepAlive (8), PeerSharing (10).

**Mux** — The multiplexer that fairly schedules egress traffic across mini-protocols on a single TCP socket.

**NtN (Node-to-Node)** — Inter-node protocol, runs over TCP. Bootstrap, ChainSync, BlockFetch, TxSubmission, KeepAlive, PeerSharing.

**NtC (Node-to-Client)** — Local-only protocol over a Unix socket. LocalStateQuery, LocalTxSubmission, LocalTxMonitor. Used by wallets and `cardano-cli`.

**PeerSharing** — Mini-protocol 10. Lets neighbours exchange addresses of other peers.

**Public root** — A DNS-published root peer set, advertised by various community operators.

**SDU** — Service Data Unit. A fragment of a mini-protocol message. Mux fragments outgoing messages into SDUs of up to ~12 KiB.

**Warm peer** — Has an established connection but is not actively syncing.

## Mempool and transactions

**Mempool** — Fee-ordered queue of pending transactions. Yggdrasil applies upstream's per-peer byte budget (64 KiB), per-peer outstanding-TxIds cap (64), per-peer in-flight count cap (32), and a global aggregate byte budget (2 MiB).

**TxId (Transaction ID)** — Blake2b-256 of the transaction body CBOR.

**TxSubmission2** — Mini-protocol 4. The pull-based flow where the local node advertises new TxIds and the peer pulls bodies for any unknown TxIds.

**LocalTxSubmission** — NtC protocol for submitting a transaction to the node's mempool. Used by wallets.

**LocalTxMonitor** — NtC protocol for monitoring the mempool: list pending TxIds, get individual transactions, etc.

## Ledger

**DRep (Delegated Representative)** — Conway-era governance entity. Stake holders delegate their voting power to DReps.

**EnactState** — The committed governance state at the start of an epoch. Pulled from the previous epoch's ratified proposals.

**Governance Action (GovAction)** — A Conway-era proposal: `ParameterChange`, `HardForkInitiation`, `TreasuryWithdrawals`, `NoConfidence`, `UpdateCommittee`, `NewConstitution`, `InfoAction`.

**LedgerState** — Yggdrasil's in-memory representation of the chain's ledger state. Includes UTxO, pool state, account state, governance state, etc.

**Plutus** — Cardano's smart contract platform. Three versions: V1, V2 (Babbage), V3 (Conway).

**Pool key hash** — Blake2b-224 of a pool's cold key vkey. The on-chain identifier for a stake pool.

**Stake credential** — Either a key hash or a script hash that owns reward delegation rights.

**Stake distribution** — Per-pool aggregated active stake at the start of an epoch. Determines block-production probability.

**UTxO (Unspent Transaction Output)** — The atomic unit of state. A transaction consumes UTxOs and produces new ones.

**Withdrawal** — A reward-account-to-payment-account transfer signed by the stake credential's witness.

## Yggdrasil-specific

**`max_concurrent_block_fetch_peers`** — Operator knob. Default `1` (legacy single-peer pipeline). Set to `2` (mainnet operator-friendly) after running §6.5 rehearsal to enable multi-peer parallel fetch.

**ChainDb** — The coordinated immutable + volatile + ledger-state storage facade in `crates/storage`.

**FetchWorkerPool** — Per-peer BlockFetch worker registry. Mirrors upstream `Ouroboros.Network.BlockFetch.ClientRegistry`.

**HotPeerScheduling** — Per-mini-protocol weight table for mux egress on hot peers. Defaults: BlockFetch=10, ChainSync=3, TxSubmission=2, KeepAlive=1, PeerSharing=1.

**OutboundPeerManager** — Yggdrasil's runtime-side container for active outbound peer sessions. Owns the migrate-to-worker logic.

**PeerSession** — Per-peer state held by the runtime: TCP connection, mux handles, mini-protocol clients, KES heartbeat scheduler.

**Tentative header** — A header announced via `MsgRollForward` before its body has been validated. Yggdrasil supports this for diffusion pipelining (DPvDV).

## See also

- [Cardano Operations Book](https://book.world.dev.cardano.org/) — comprehensive reference for production operators.
- [Formal Ledger Specifications](https://intersectmbo.github.io/formal-ledger-specifications/site) — the authoritative ledger semantics.
- [IOHK Documentation](https://docs.cardano.org/) — concept-level overviews.
