---
title: Overview
layout: default
parent: User Manual
nav_order: 1
---

# Overview

## What Yggdrasil is

Yggdrasil is a Cardano node implementation written in pure Rust. It speaks the same protocols as the upstream Haskell node, downloads the same blocks, applies the same ledger rules, and produces the same on-chain artifacts — block hashes, transaction IDs, and ledger state are byte-for-byte compatible with the Haskell node.

A Cardano node performs four jobs:

1. **Stays in sync with the network.** It connects to peers, downloads new blocks as they are produced, validates them, and stores them on disk.
2. **Maintains the ledger state.** Every block changes the UTxO set, stake distribution, governance state, and other ledger components. The node replays every block since genesis to compute the current state.
3. **Serves block and chain data to other peers.** Once it has the chain, it acts as a relay so other nodes can download from it.
4. **Optionally produces blocks.** A stake pool operator runs the node with KES, VRF, and operational-certificate credentials so the node can forge blocks during the slots it is elected for.

A relay node does jobs 1–3. A block producer does all four. Yggdrasil supports both modes from the same binary, distinguished only by which credentials are configured.

## What Yggdrasil is not

- **Not a wallet.** Yggdrasil is a full node. It does not manage keys for spending, derive addresses, or build transactions. Use a wallet (Daedalus, Yoroi, Eternl, command-line `cardano-cli`) to construct transactions and submit them to the node via the Local Tx Submission protocol.
- **Not a database server.** It stores its own ledger and chain state in a file-backed format optimised for the consensus protocol. It is not a general-purpose query backend. For analytics, use [`db-sync`](https://github.com/IntersectMBO/cardano-db-sync) or [`Carp`](https://github.com/dcSpark/carp), both of which can connect to a Yggdrasil node.

## What Yggdrasil offers

- **No native dependencies.** Pure Rust. No FFI for cryptography, no system libraries beyond `libc`. Easy to build, easy to audit, easy to cross-compile.
- **Edition 2024, toolchain 1.95.0.** Fixed and reproducible.
- **Five mini-protocols implemented end-to-end.** ChainSync, BlockFetch, KeepAlive, TxSubmission2, PeerSharing — both client and server sides.
- **All eight eras supported.** Byron, Shelley, Allegra, Mary, Alonzo, Babbage, Conway. Era boundaries are handled by the same multi-era apply pipeline used in upstream.
- **Plutus V1, V2, and V3.** Native CEK machine, calibrated cost model from genesis, V3 Conway script context.
- **Drift detection against upstream.** A `node/scripts/check_upstream_drift.sh` tool compares pinned Haskell-node SHAs against live HEAD and surfaces lag.

## How a Cardano node connects

A Cardano network is a peer-to-peer overlay where each node maintains a set of:

- **Local roots** — peers the operator manually configured (typically your own relays for a pool topology).
- **Public roots** — DNS-resolved peers from a published list (`config.json` `PublicRoots`).
- **Bootstrap peers** — IOG-operated entry points used to seed initial connectivity.
- **Ledger peers** — registered stake pool relays discovered from the on-chain ledger state.
- **Big-ledger peers** — a curated subset of the largest pools' relays, used for emergency reconnection.
- **Peer-share peers** — peers learned through the PeerSharing mini-protocol from existing neighbours.

The peer governor maintains targets for each category and balances them through promotion (`cold → warm → hot`) and demotion (`hot → warm → cold`) decisions every governor tick.

A connection between two nodes carries multiple mini-protocols multiplexed over a single TCP socket. The mux gives each protocol a fair share of the egress bandwidth via weighted round-robin scheduling.

## Where to go next

- If you want to get a node running quickly, go to [Quick Start]({{ "/manual/quick-start/" | relative_url }}).
- If you want to understand the architecture in depth before installing anything, read [Architecture]({{ "/ARCHITECTURE/" | relative_url }}).
- Otherwise, continue to [Installation]({{ "/manual/installation/" | relative_url }}).
