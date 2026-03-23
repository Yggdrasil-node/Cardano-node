---
name: network-src-subagent
description: Guidance for concrete network transport, mux, and client-driver implementation work
---

Focus on implementation details for bearer I/O, mux/demux behavior, protocol driver ergonomics, and wire-level safety properties.

## Scope
- `bearer.rs`, `multiplexer.rs`, `mux.rs`, `peer.rs`, `peer_selection.rs`, typed client/server drivers, and peer governor.
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
- Server-side (responder) drivers are now implemented for all four data mini-protocols: `KeepAliveServer`, `BlockFetchServer`, `ChainSyncServer`, and `TxSubmissionServer`. Each server driver wraps `MessageChannel`, maintains the protocol state machine, and provides typed send/receive methods. The server drivers follow the same pattern as the client drivers but handle server-agency states.
- Topology domain model work is now in this crate: local roots carry `hotValency`, `warmValency`, `diffusionMode`, and trustability, while public roots remain a separate type.
- Root-set provider work has started here as well: topology parsing now feeds a resolved provider snapshot, mutable `RootPeerProviderState`, and a refresh-oriented provider API that enforce local-root versus bootstrap/public-root precedence and disjointness. Provider refreshes can also reconcile the `PeerRegistry` on the same crate-owned path, and a DNS-backed root-peer provider now re-resolves configured local-root, bootstrap, and public-root access points without involving `node`. An optional `DnsRefreshPolicy` gates re-resolution behind a time-based schedule with exponential backoff on stale results (defaults matching upstream `clipTTLBelow` 60 s / `clipTTLAbove` 900 s).
- A minimal peer registry now lives here too: `PeerRegistry` tracks `PeerSource` and `PeerStatus` per peer and can reconcile root-provider snapshots plus ledger, big-ledger, and peer-share source sets while preserving unrelated sources and peer status.
- Ledger peer provider work is now complete: `LedgerPeerProvider` trait, `LedgerPeerSnapshot` normalization (deduplicates and enforces disjoint ledger/big-ledger sets), `LedgerPeerProviderRefresh` (combined/per-kind), `apply_ledger_peer_refresh()` helper, `refresh_ledger_peer_registry()` orchestration, and policy helpers `judge_ledger_peer_usage()` plus `reconcile_ledger_peer_registry_with_policy()` for `useLedgerAfterSlot`, latest-slot, ledger-state judgement, and peer-snapshot freshness gating. Provider refreshes reconcile the `PeerRegistry` on crate-owned paths without node involvement.
- Peer governor module (`governor.rs`) now provides a pure decision engine: `GovernorTargets` (target_known/established/active), `LocalRootTargets` (per-group warm/hot valency from `LocalRootConfig`), and `GovernorAction` (PromoteToWarm/PromoteToHot/DemoteToWarm/DemoteToCold). Evaluation functions `evaluate_cold_to_warm_promotions`, `evaluate_warm_to_hot_promotions`, `evaluate_hot_to_warm_demotions`, `evaluate_warm_to_cold_demotions`, `enforce_local_root_valency`, and `governor_tick` compute actions from current `PeerRegistry` state. `GovernorState` carries mutable failure-tracking (`record_success`/`record_failure`, `is_backing_off`, `filter_backed_off`) and churn timing (`ChurnConfig`, `evaluate_churn`, `tick`). 11 governor tests.
- PeerSharing protocol (mini-protocol 10) is now implemented: `PeerSharingState` state machine + `PeerSharingMessage` (MsgShareRequest/MsgSharePeers/MsgDone) + `SharedPeerAddress` (IPv4/IPv6 CBOR codec) in `protocols/peer_sharing.rs`. Client driver `PeerSharingClient` (`peersharing_client.rs`) and server driver `PeerSharingServer` (`peersharing_server.rs`) with serve_loop callback. 8 protocol-level tests.
- Runtime integration status: the node runtime now opens mini-protocol 10 on standard node-to-node sessions, advertises `peer_sharing = 1` during bootstrap proposals, and can reconcile peer-sharing discoveries into `PeerRegistry` as `PeerSourcePeerShare`. `PeerRegistry` now also tracks per-peer `hot_tip_slot` metadata and exposes `preferred_hot_peer()` so reconnect paths can prefer the most advanced known hot peer. Remaining work is full hot-protocol scheduling beyond reconnect bootstrap preference and any additional responder-side runtime usage.
