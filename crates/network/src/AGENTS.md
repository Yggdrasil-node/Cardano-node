---
name: network-src-subagent
description: Guidance for concrete network transport, mux, and client-driver implementation work
---

Focus on implementation details for bearer I/O, mux/demux behavior, protocol driver ergonomics, and wire-level safety properties.

## Scope
- `bearer.rs`, `multiplexer.rs`, `mux.rs`, `peer.rs`, `peer_selection.rs`, and typed client drivers.
- CBOR message boundary handling, segmentation/reassembly, and protocol-handle composition.

##  Rules *Non-Negotiable*
- Keep wire framing deterministic and byte-accurate.
- Do not leak protocol business logic from `protocols/` state machines into transport primitives.
- Keep reusable topology types, peer candidate resolution, bootstrap-target sequencing, reconnect attempt ordering, and preferred-peer retry state here rather than in `node`, while avoiding full peer-governor state in low-level transport helpers.
- Preserve strict separation between raw transport (`ProtocolHandle`) and higher-level message orchestration (`MessageChannel`, client drivers).
- Any receive-path buffering or boundary detection changes MUST ship with regression tests for partial/incremental payload delivery.
- Public transport and driver APIs MUST include Rustdocs when behavior is non-obvious.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant AGENTS.md file.

## Official Upstream References *Always research referances and add or update links as needed*
- Multiplexer implementation: <https://github.com/IntersectMBO/ouroboros-network/tree/main/network-mux>
- Network framework: <https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network-framework>
- Mini-protocol implementations: <https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network-protocols>

## Current Phase
- TCP bearer and SDU framing are implemented and tested.
- Mux/demux routing is implemented with per-protocol handles.
- Large-message SDU segmentation/reassembly is implemented via `MAX_SEGMENT_SIZE` + `MessageChannel`.
- Typed ChainSync, BlockFetch, KeepAlive, and TxSubmission client drivers are in place. TxSubmission now uses typed ledger `TxId` values for request/advertise flows, provides typed reply helpers for both `Vec<Tx>` and `Vec<MultiEraSubmittedTx>`, and maintains an outstanding/requestable TxId FIFO so invalid acknowledgements and transaction requests are rejected before replying while preserving raw wire bodies.
- Topology domain model work is now in this crate: local roots carry `hotValency`, `warmValency`, `diffusionMode`, and trustability, while public roots remain a separate type.
- Root-set provider work has started here as well: topology parsing now feeds a resolved provider snapshot, mutable `RootPeerProviderState`, and a refresh-oriented provider API that enforce local-root versus bootstrap/public-root precedence and disjointness. Provider refreshes can also reconcile the `PeerRegistry` on the same crate-owned path, and a DNS-backed root-peer provider now re-resolves configured local-root, bootstrap, and public-root access points without involving `node`. An optional `DnsRefreshPolicy` gates re-resolution behind a time-based schedule with exponential backoff on stale results (defaults matching upstream `clipTTLBelow` 60 s / `clipTTLAbove` 900 s).
- A minimal peer registry now lives here too: `PeerRegistry` tracks `PeerSource` and `PeerStatus` per peer and can reconcile root-provider snapshots plus ledger, big-ledger, and peer-share source sets while preserving unrelated sources and peer status.
- Ledger peer provider work is now complete: `LedgerPeerProvider` trait, `LedgerPeerSnapshot` normalization (deduplicates and enforces disjoint ledger/big-ledger sets), `LedgerPeerProviderRefresh` (combined/per-kind), `apply_ledger_peer_refresh()` helper, `refresh_ledger_peer_registry()` orchestration, and `ScriptedLedgerPeerProvider` for testing. Provider refreshes reconcile the `PeerRegistry` on crate-owned paths without node involvement.
- Next: typed protocol payload decoding (replace remaining opaque `Vec<u8>` payloads where practical), then implement governor-style promotion/churn behavior or consensus bridge for ledger peers.
