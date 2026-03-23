---
name: network-crate-agent
description: Guidance for Cardano mini-protocol and peer networking work
---

Focus on typed protocol state machines, connection lifecycle, and exact wire-behavior boundaries.

## Scope
- Handshake, multiplexing, mini-protocol state machines, and peer lifecycle.
- Peer candidate resolution, topology domain types, and bootstrap-target ordering helpers that feed runtime peer policy.
- Node-to-node and node-to-client protocol surfaces.

##  Rules *Non-Negotiable*
- Handshake and multiplexing interfaces MUST remain independent from peer policy logic.
- Topology domain types, peer candidate ordering helpers, and the governor decision engine (promotion/demotion/churn policy) belong here. The governor is a pure decision function — effectful connection management stays in `node`.
- Mini-protocols MUST be introduced incrementally, starting with ChainSync and BlockFetch.
- Testable state transitions MUST be preferred over implicit runtime behavior.
- Public protocol types, handshake surfaces, and state-machine functions MUST have Rustdocs where message flow or invariants are not self-evident.
- Stay true to the official type naming and terminology for node concepts, network protocols, and ledger types when possible.
- Naming MUST mirror the official node and Ouroboros network specs so protocol traces and docs line up cleanly.
- Wire behavior and protocol sequencing MUST be explained by reference to the official node and upstream Ouroboros network sources.
- Always read the folder specific `**/AGENTS.md` files. They MUST stay current and MUST remain operational rather than long-form documentation. If the folder context is outdated, missing, or incorrect, update the relevant AGENTS.md file.

## Official Upstream References *Always research referances and add or update links as needed*
- Networking repository root: <https://github.com/IntersectMBO/ouroboros-network/>
- Multiplexer package: <https://github.com/IntersectMBO/ouroboros-network/tree/main/network-mux/>
- Framework and handshake layer: <https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network-framework/>
- Protocol implementations: <https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network-protocols/>
- Shelley networking spec PDF: <https://ouroboros-network.cardano.intersectmbo.org/pdfs/network-spec/>

## Current Phase
- Multiplexer framing (SDU header encode/decode, MiniProtocolNum, MiniProtocolDir) is implemented and tested.
- Async bearer transport (Bearer trait, TcpBearer, Sdu framing) is implemented and tested over TCP loopback.
- Multiplexer/demuxer (`mux.rs`) is implemented: `start()` splits a TcpStream into reader/writer halves, spawns background demux (reads SDUs, dispatches by protocol number to per-protocol ingress channels) and mux (collects tagged payloads from protocol handles, frames as SDUs) tasks. Per-protocol `ProtocolHandle` provides `send()`/`recv()` for exchanging payloads through the mux. Tested: single-protocol round-trip, multi-protocol routing, bidirectional exchange, connection close detection, empty payloads, multiple messages, clean shutdown on handle drop.
- SDU segmentation is implemented for large protocol payloads: mux writer splits payloads into `MAX_SEGMENT_SIZE` chunks, and `MessageChannel` reassembles segmented SDUs into complete CBOR messages using self-delimiting CBOR item-length detection.
- Peer connection lifecycle (`peer.rs`) is implemented: `connect()` (initiator) and `accept()` (responder) establish TCP, start the mux, run the Handshake mini-protocol (ProposeVersions / AcceptVersion / Refuse), and return a `PeerConnection` with negotiated version + data-protocol handles. Version negotiation selects the highest common version matching network magic. Tested: happy-path connect/accept, data exchange after handshake, handshake refusal on magic mismatch, highest-version selection.
- Handshake protocol types, state machine, and CBOR wire codec (ProposeVersions/AcceptVersion/Refuse/QueryReply) are complete.
- ChainSync, BlockFetch, KeepAlive, and TxSubmission2 have full message enums, validated state machines, transition tests, and CBOR wire codecs.
- Wire tags match upstream CDDL. ChainSync and BlockFetch now expose typed point/range helpers (`request_next_typed`, `find_intersect_points`, `request_range_points`) backed by ledger `Point` CBOR. ChainSync also exposes generic decoded-header support via `request_next_decoded_header::<H>()`. BlockFetch now exposes both per-item decode helpers (`recv_block_decoded::<B>()`, `recv_block_with()`, `recv_block_raw_with()`) and batch collection helpers (`request_range_collect*`) for raw, decoded, and raw+decoded flows. TxSubmission now uses ledger `TxId` for advertised/requested identifiers, exposes typed reply helpers for both storage `Tx` wrappers and ledger `MultiEraSubmittedTx` values, and tracks the outstanding/requestable TxId FIFO so invalid `ack`, blocking-mode, and request sequences are rejected locally while transaction bodies on the wire remain raw bytes.
- Server-side (responder) drivers are now implemented for all four data mini-protocols: `KeepAliveServer` (echo loop), `BlockFetchServer` (range-request + batch streaming), `ChainSyncServer` (request-next + find-intersect responding), and `TxSubmissionServer` (server-driven tx-id/tx request-reply). Each server driver wraps `MessageChannel`, maintains the protocol state machine, and provides typed send/receive methods mirroring the client driver pattern.
- Upstream planning anchor: `cardano-node` `TopologyP2P` and `ouroboros-network` `Diffusion.Topology` model local root groups with `hotValency`, `warmValency`, `diffusionMode`, and trustability, while `PublicRootPeers` distinguishes configured public roots, bootstrap peers, ledger peers, and big ledger peers. Yggdrasil now carries those local and public root topology domain types in `peer_selection.rs`.
- Yggdrasil also now carries network-owned topology root configuration and an initial resolved root-provider snapshot, including upstream-style `UseBootstrapPeers` and `UseLedgerPeers` semantics plus precedence and disjointness handling between local, bootstrap, and public roots.
- A minimal peer registry now also lives in this crate. It tracks per-peer `PeerSource` tags and `PeerStatus` state, and it can reconcile the canonical root-provider snapshot into root-peer registry entries without involving `node`.
- Root-set provider layer is now expanded: DNS-backed root-peer provider re-resolves configured local-root, bootstrap, and public-root access points with optional `DnsRefreshPolicy` (TTL clamping 60s/900s, exponential backoff). Provider refreshes reconcile the `PeerRegistry` on crate-owned paths.
- Peer registry is now extended with ledger, big-ledger, and peer-share source reconciliation helpers that preserve unrelated sources and peer status.
- Ledger peer provider layer is now complete: `LedgerPeerProvider` trait, `LedgerPeerSnapshot` normalization (deduplicates and enforces disjoint ledger/big-ledger sets), `LedgerPeerProviderRefresh` (combined/per-kind), `apply_ledger_peer_refresh()` helper, `refresh_ledger_peer_registry()` orchestration, `judge_ledger_peer_usage()` plus `reconcile_ledger_peer_registry_with_policy()` for `useLedgerAfterSlot` / latest-slot / ledger-state / peer-snapshot freshness gating, and `ScriptedLedgerPeerProvider` for testing. Provider refreshes reconcile the `PeerRegistry` on crate-owned paths without node involvement.
- Peer governor module (`governor.rs`) implements a pure decision engine: `GovernorTargets` (target_known/established/active), `LocalRootTargets` (per-group warm/hot valency), `GovernorAction` (PromoteToWarm/PromoteToHot/DemoteToWarm/DemoteToCold), evaluation functions for promotions/demotions, local-root valency enforcement, and combined `governor_tick`. `GovernorState` carries mutable failure-tracking (record_success/record_failure, is_backing_off, filter_backed_off) and churn timing (ChurnConfig, evaluate_churn, tick). 11 governor tests.
- PeerSharing protocol (mini-protocol 10) is now implemented: `PeerSharingState` state machine, `PeerSharingMessage` (MsgShareRequest/MsgSharePeers/MsgDone), `SharedPeerAddress` (IPv4/IPv6 CBOR codec). Client driver `PeerSharingClient` and server driver `PeerSharingServer` (with serve_loop callback) are in place. 8 protocol-level tests.
- TCP listener (`listener.rs`) wraps `TcpListener` for inbound peer connections: `PeerListener::bind()`, `from_listener()`, `accept_peer()`.
- Next implementation order:
	1. Complete consensus-network bridge parity by replacing node-owned ledger-peer refresh orchestration with live consensus-fed judgement and snapshot updates into network-owned providers.
	2. Extend hot-peer behavior from reconnect preference to real multi-peer hot-protocol scheduling and chain-selection-aware sync assignment.
	3. Expand typed protocol payload decoding (replace remaining opaque `Vec<u8>` payloads where practical).
