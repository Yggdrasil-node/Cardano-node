// Tests for the parent module. Extracted from inline `#[cfg(test)] mod
// tests` block in R256 Phase H to keep the parent file readable.
// `use super::*;` still gives full access to the parent's items.

use super::{
    BlockProvider, ChainProvider, InboundSessionAborts, PEER_SHARING_MAX_AMOUNT,
    PeerSharingProvider, SharedChainDb, SharedPeerSharingProvider, TxSubmissionConsumer,
    process_connection_manager_timeouts, run_inbound_accept_loop,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use yggdrasil_consensus::mempool::SharedTxState;
use yggdrasil_ledger::TxId;
use yggdrasil_ledger::{
    Block, BlockHeader, BlockNo, CborDecode, CborEncode, Encoder, Era, HeaderHash, Point,
    ShelleyBlock, ShelleyHeader, ShelleyHeaderBody, ShelleyOpCert, ShelleyVrfCert, SlotNo, Tip,
};
use yggdrasil_network::{
    ConnStateId, ConnectionEntry, ConnectionId, ConnectionManagerState, ConnectionState,
    HandshakeVersion, KeepAliveMessage, MuxError, MuxHandle, NextResponse, PeerListener,
    PeerSharingMessage, SharedPeerAddress, TxIdAndSize, TxSubmissionMessage,
};
use yggdrasil_node_runtime::NodeConfig;
use yggdrasil_node_runtime::bootstrap;
use yggdrasil_storage::{
    ChainDb, ImmutableStore, InMemoryImmutable, InMemoryLedgerStore, InMemoryVolatile,
    VolatileStore,
};

const SHELLEY_ERA_TAG: u64 = 2;

fn sample_vrf_cert(seed: u8) -> ShelleyVrfCert {
    ShelleyVrfCert {
        output: vec![seed; 32],
        proof: [seed.wrapping_add(1); 80],
    }
}

fn sample_opcert(seed: u8) -> ShelleyOpCert {
    ShelleyOpCert {
        hot_vkey: [seed; 32],
        sequence_number: 42,
        kes_period: 100,
        sigma: [seed.wrapping_add(2); 64],
    }
}

#[test]
fn select_within_byte_budget_admits_first_even_when_oversize() {
    // Single oversize candidate is always admitted to guarantee
    // forward progress (matches upstream `collectTxs` semantics).
    let big = TxId([0x11; 32]);
    let mut sizes = HashMap::new();
    sizes.insert(big, 100_000u32);
    let (admitted, deferred) = super::select_within_byte_budget(&[big], &sizes, 64 * 1024);
    assert_eq!(admitted, vec![big]);
    assert_eq!(deferred, 0);
}

#[test]
fn select_within_byte_budget_greedy_prefix_then_defers() {
    // Greedy-prefix: admits while `sz <= remaining`, then breaks.
    // After admitting 800 + 500 (remaining = 700, then 200), the
    // 600-byte candidate exceeds remaining 200 so it and any
    // subsequent items are deferred.
    let a = TxId([1; 32]);
    let b = TxId([2; 32]);
    let c = TxId([3; 32]);
    let d = TxId([4; 32]);
    let mut sizes = HashMap::new();
    sizes.insert(a, 800u32);
    sizes.insert(b, 500u32);
    sizes.insert(c, 600u32);
    sizes.insert(d, 100u32);
    let (admitted, deferred) = super::select_within_byte_budget(&[a, b, c, d], &sizes, 1500);
    assert_eq!(admitted, vec![a, b]);
    assert_eq!(deferred, 2);
}

#[test]
fn select_within_byte_budget_zero_budget_admits_one_then_defers() {
    // Zero remaining budget still admits the first item (forward
    // progress) but defers everything after it.
    let a = TxId([1; 32]);
    let b = TxId([2; 32]);
    let mut sizes = HashMap::new();
    sizes.insert(a, 50u32);
    sizes.insert(b, 50u32);
    let (admitted, deferred) = super::select_within_byte_budget(&[a, b], &sizes, 0);
    assert_eq!(admitted, vec![a]);
    assert_eq!(deferred, 1);
}

#[test]
fn select_within_byte_budget_missing_size_treated_as_zero() {
    // Missing size in the lookup defaults to 0, so an item with no
    // declared size is always admittable up to the loop's own bound.
    let a = TxId([1; 32]);
    let b = TxId([2; 32]);
    let sizes = HashMap::new();
    let (admitted, deferred) = super::select_within_byte_budget(&[a, b], &sizes, 0);
    // Both admitted: a admitted as first (forward progress), b
    // admitted because its size lookup is 0 which fits in budget 0.
    assert_eq!(admitted, vec![a, b]);
    assert_eq!(deferred, 0);
}

#[test]
fn clamp_request_count_returns_full_batch_when_headroom_exceeds_batch() {
    // peer at 10 unacked, ack 2 → outstanding 8, headroom 64-8=56,
    // batch 16 → request 16.
    assert_eq!(super::clamp_request_count(10, 2, 16, 64), 16);
}

#[test]
fn clamp_request_count_clamps_to_remaining_headroom() {
    // peer at 60 unacked, ack 2 → outstanding 58, headroom 64-58=6,
    // batch 16 → request 6.
    assert_eq!(super::clamp_request_count(60, 2, 16, 64), 6);
}

#[test]
fn clamp_request_count_returns_one_at_cap_for_forward_progress() {
    // peer at 64 unacked, ack 0 → outstanding 64, headroom 0,
    // batch 16 → request 1 (max(1) floor).
    assert_eq!(super::clamp_request_count(64, 0, 16, 64), 1);
}

#[test]
fn clamp_request_count_ack_widens_headroom() {
    // peer at 64 unacked, ack 16 → outstanding 48, headroom 16,
    // batch 16 → request 16.  Without ack subtraction we would
    // erroneously request only 1.
    assert_eq!(super::clamp_request_count(64, 16, 16, 64), 16);
}

#[test]
fn clamp_to_count_budget_admits_full_list_when_under_cap() {
    let a = TxId([0x10; 32]);
    let b = TxId([0x11; 32]);
    let c = TxId([0x12; 32]);
    let (admitted, deferred) = super::clamp_to_count_budget(&[a, b, c], 32);
    assert_eq!(admitted, vec![a, b, c]);
    assert_eq!(deferred, 0);
}

#[test]
fn clamp_to_count_budget_truncates_to_remaining_headroom() {
    let a = TxId([0x10; 32]);
    let b = TxId([0x11; 32]);
    let c = TxId([0x12; 32]);
    let (admitted, deferred) = super::clamp_to_count_budget(&[a, b, c], 2);
    assert_eq!(admitted, vec![a, b]);
    assert_eq!(deferred, 1);
}

#[test]
fn clamp_to_count_budget_admits_one_at_cap_for_forward_progress() {
    let a = TxId([0x10; 32]);
    let b = TxId([0x11; 32]);
    let (admitted, deferred) = super::clamp_to_count_budget(&[a, b], 0);
    // First-admit guarantee: peer at cap still gets one fetch so
    // the loop does not stall.
    assert_eq!(admitted, vec![a]);
    assert_eq!(deferred, 1);
}

#[test]
fn clamp_to_count_budget_empty_input_returns_empty() {
    let (admitted, deferred) = super::clamp_to_count_budget(&[], 32);
    assert!(admitted.is_empty());
    assert_eq!(deferred, 0);
}

#[test]
fn global_cap_composes_min_with_per_peer_cap() {
    // Reproduce the in-loop budget computation: take the minimum of
    // per-peer remaining and global remaining, then call
    // `select_within_byte_budget`.  Verifies the global aggregate
    // cap (`maxTxsSizeInflight`) correctly limits a peer that has
    // ample per-peer headroom but the shared global pool is nearly
    // full.  Mirrors the runtime wiring in `run_txsubmission_server`.
    let shared = SharedTxState::default();
    let _busy_peer = SocketAddr::from(([127, 0, 0, 1], 4000));
    let quiet_peer = SocketAddr::from(([127, 0, 0, 1], 4001));

    // Saturate the global pool from `busy_peer` up to (TOTAL - 1500),
    // staying within the per-peer cap by spreading across many small
    // entries.  We model this directly by marking many small advertised
    // txs as in-flight.
    const PER_PEER: u64 = 64 * 1024;
    const TOTAL: u64 = 64 * 1024 * 32;

    // Drive busy_peer's bytes near per-peer cap (exactly PER_PEER - 1000)
    // and global bytes via additional sized entries from other simulated
    // peers — but for the unit test we use SharedTxState's accounting
    // directly: load up to `target_global_used` via repeated peers.
    let target_global_used = TOTAL - 1500; // global remaining = 1500
    let chunk: u32 = 1000;
    let mut filled: u64 = 0;
    let mut counter: u32 = 0;
    while filled + (chunk as u64) <= target_global_used {
        let p = SocketAddr::from(([127, 0, 0, 1], 5000 + (counter as u16 % 60000)));
        // Build a unique 32-byte TxId from `counter` so cross-peer
        // dedup does not silently drop later inserts.
        let mut id_bytes = [0u8; 32];
        id_bytes[..4].copy_from_slice(&counter.to_be_bytes());
        let id = TxId(id_bytes);
        let _ = shared.filter_advertised(&p, &[id]);
        shared.mark_in_flight_sized(&p, &[(id, chunk)]);
        filled += chunk as u64;
        counter += 1;
    }

    let per_peer_remaining = PER_PEER.saturating_sub(shared.peer_inflight_bytes(&quiet_peer));
    let global_remaining = TOTAL.saturating_sub(shared.inflight_bytes_total());
    let budget_remaining = per_peer_remaining.min(global_remaining);

    // quiet_peer has full per-peer headroom (64 KiB) but global is the
    // binding constraint (within one chunk of `target_global_used`
    // residual since the loop stops when the NEXT chunk would
    // overshoot `target_global_used`).  Verify min() picks global.
    assert!(per_peer_remaining > global_remaining);
    assert_eq!(budget_remaining, global_remaining);
    assert!(budget_remaining <= 1500 + chunk as u64);

    // Greedy admission with this composed budget defers anything
    // past ~1500 bytes worth of advertised txs, even though the
    // per-peer view alone would have admitted ~64 KiB.
    let a = TxId([0xa1; 32]);
    let b = TxId([0xa2; 32]);
    let c = TxId([0xa3; 32]);
    let mut sizes = HashMap::new();
    sizes.insert(a, 800u32);
    sizes.insert(b, 600u32);
    sizes.insert(c, 800u32);
    let (admitted, deferred) =
        super::select_within_byte_budget(&[a, b, c], &sizes, budget_remaining);
    // a (800) + b (600) = 1400 ≤ ~1500; c (800) exceeds → deferred.
    assert_eq!(admitted, vec![a, b]);
    assert_eq!(deferred, 1);
}

#[tokio::test]
async fn inbound_session_aborts_aborts_registered_mux_and_is_idempotent() {
    let aborts = InboundSessionAborts::default();
    let peer = SocketAddr::from(([127, 0, 0, 1], 3001));

    let mux = MuxHandle {
        reader: tokio::spawn(async { std::future::pending::<Result<(), MuxError>>().await }),
        writer: tokio::spawn(async { std::future::pending::<Result<(), MuxError>>().await }),
    };

    aborts.insert(peer, &mux);
    assert!(aborts.abort(&peer));
    assert!(!aborts.abort(&peer));

    assert!(mux.reader.await.is_err(), "reader task should be aborted");
    assert!(mux.writer.await.is_err(), "writer task should be aborted");
}

#[test]
fn process_connection_manager_timeouts_removes_expired_terminating_entry() {
    let peer = SocketAddr::from(([127, 0, 0, 1], 4001));
    let conn_id = ConnectionId {
        local: SocketAddr::from(([127, 0, 0, 1], 3001)),
        remote: peer,
    };

    let mut cm = ConnectionManagerState::new();
    cm.connections.insert(
        peer,
        ConnectionEntry {
            conn_state_id: ConnStateId(1),
            state: ConnectionState::TerminatingState {
                conn_id,
                error: None,
            },
            responder_timeout_deadline: None,
            time_wait_deadline: Some(Instant::now() - Duration::from_secs(1)),
        },
    );

    let cm = Arc::new(RwLock::new(cm));
    let aborts = InboundSessionAborts::default();
    process_connection_manager_timeouts(&cm, Some(&aborts));

    let cm = cm.read().expect("cm lock poisoned");
    assert!(
        !cm.connections.contains_key(&peer),
        "expired terminating entry should be removed by timeout tick"
    );
}

fn make_shelley_block(
    slot: u64,
    block_number: u64,
    prev_hash: Option<[u8; 32]>,
) -> (Block, ShelleyHeader) {
    let header = ShelleyHeader {
        body: ShelleyHeaderBody {
            block_number,
            slot,
            prev_hash,
            issuer_vkey: [0x11; 32],
            vrf_vkey: [0x22; 32],
            nonce_vrf: sample_vrf_cert(0x30),
            leader_vrf: sample_vrf_cert(0x40),
            block_body_size: 0,
            block_body_hash: [0x55; 32],
            operational_cert: sample_opcert(0x60),
            protocol_version: (2, 0),
        },
        signature: vec![0xDD; 448],
    };

    let shelley_block = ShelleyBlock {
        header: header.clone(),
        transaction_bodies: Vec::new(),
        transaction_witness_sets: Vec::new(),
        transaction_metadata_set: HashMap::new(),
    };

    let body_bytes = shelley_block.to_cbor_bytes();
    let mut enc = Encoder::new();
    enc.array(2);
    enc.unsigned(SHELLEY_ERA_TAG);
    enc.raw(&body_bytes);
    let raw_cbor = enc.into_bytes();

    let header_hash = header.header_hash();
    (
        Block {
            era: Era::Shelley,
            header: BlockHeader {
                hash: header_hash,
                prev_hash: HeaderHash(prev_hash.unwrap_or([0; 32])),
                slot_no: SlotNo(slot),
                block_no: BlockNo(block_number),
                issuer_vkey: header.body.issuer_vkey,
                protocol_version: Some(header.body.protocol_version),
            },
            transactions: Vec::new(),
            raw_cbor: Some(std::sync::Arc::from(raw_cbor)),
            header_cbor_size: None,
        },
        header,
    )
}

#[test]
fn block_provider_uses_exclusive_lower_bound_from_origin() {
    let (block, _) = make_shelley_block(10, 1, Some([0xAA; 32]));
    let expected_raw: Vec<u8> = block.raw_cbor.clone().expect("raw block").to_vec();
    let upper = Point::BlockPoint(block.header.slot_no, block.header.hash).to_cbor_bytes();

    let mut immutable = InMemoryImmutable::default();
    immutable
        .append_block(block)
        .expect("append immutable block");
    let db = ChainDb::new(
        immutable,
        InMemoryVolatile::default(),
        InMemoryLedgerStore::default(),
    );
    let provider = SharedChainDb::new(db);

    let blocks = provider.get_block_range(&Point::Origin.to_cbor_bytes(), &upper);
    assert_eq!(blocks, vec![expected_raw]);
}

#[test]
fn block_provider_skips_lower_bound_block() {
    let (first_block, first_header) = make_shelley_block(10, 1, Some([0xAA; 32]));
    let first_point = Point::BlockPoint(first_block.header.slot_no, first_block.header.hash);
    let (second_block, _) = make_shelley_block(20, 2, Some(first_header.header_hash().0));
    let expected_raw: Vec<u8> = second_block.raw_cbor.clone().expect("raw block").to_vec();
    let upper =
        Point::BlockPoint(second_block.header.slot_no, second_block.header.hash).to_cbor_bytes();

    let mut immutable = InMemoryImmutable::default();
    immutable
        .append_block(first_block)
        .expect("append immutable block");
    let mut volatile = InMemoryVolatile::default();
    volatile
        .add_block(second_block)
        .expect("append volatile block");
    let db = ChainDb::new(immutable, volatile, InMemoryLedgerStore::default());
    let provider = SharedChainDb::new(db);

    let blocks = provider.get_block_range(&first_point.to_cbor_bytes(), &upper);
    assert_eq!(blocks, vec![expected_raw]);
}

#[test]
fn chain_provider_returns_header_bytes_and_advances_by_point() {
    let (first_block, first_header) = make_shelley_block(10, 1, Some([0xAA; 32]));
    let first_point = Point::BlockPoint(first_block.header.slot_no, first_block.header.hash);
    let (second_block, second_header) =
        make_shelley_block(20, 2, Some(first_header.header_hash().0));
    let second_point = Point::BlockPoint(second_block.header.slot_no, second_block.header.hash);

    let mut immutable = InMemoryImmutable::default();
    immutable
        .append_block(first_block)
        .expect("append immutable block");
    let mut volatile = InMemoryVolatile::default();
    volatile
        .add_block(second_block)
        .expect("append volatile block");
    let db = ChainDb::new(immutable, volatile, InMemoryLedgerStore::default());
    let provider = SharedChainDb::new(db);

    let (cursor_point, first_raw_header, first_tip) = provider
        .next_header(&None)
        .expect("first chainsync response");
    assert_eq!(
        Point::from_cbor_bytes(&cursor_point).expect("first point"),
        first_point
    );
    assert_eq!(
        ShelleyHeader::from_cbor_bytes(&first_raw_header).expect("first header"),
        first_header
    );
    // R220 — tip is encoded as the upstream `Tip` envelope
    // (`[point, blockNo]` 2-element list), not bare `Point`.
    // Pre-R220 the assertion below pinned the wrong shape and
    // upstream-conforming clients could not decode the chain
    // tip out of `MsgRollForward`/`MsgIntersectFound`.
    let expected_tip = {
        let mut enc = Encoder::new();
        Tip::Tip(second_point, yggdrasil_ledger::BlockNo(2)).encode_cbor(&mut enc);
        enc.into_bytes()
    };
    assert_eq!(first_tip, expected_tip);

    let (next_point, second_raw_header, second_tip) = provider
        .next_header(&Some(cursor_point))
        .expect("second chainsync response");
    assert_eq!(
        Point::from_cbor_bytes(&next_point).expect("second point"),
        second_point
    );
    assert_eq!(
        ShelleyHeader::from_cbor_bytes(&second_raw_header).expect("second header"),
        second_header
    );
    assert_eq!(second_tip, expected_tip);

    assert!(
        provider
            .next_header(&Some(second_point.to_cbor_bytes()))
            .is_none()
    );
    assert_eq!(
        provider.find_intersect(&[second_point.to_cbor_bytes()]),
        Some((second_point.to_cbor_bytes(), expected_tip.clone()))
    );
    assert_eq!(provider.chain_tip(), expected_tip);

    // R221 — `chain_tip_point()` MUST return bare `Point` shape
    // (`[slot, hash]`), distinct from `chain_tip()`'s upstream
    // `Tip` envelope shape (`[point, blockNo]`).  These two
    // shapes are the canonical contract — `MsgRollBackward
    // { point, tip }` carries them in distinct argument
    // positions.  See `ChainProvider` rustdoc for the per-method
    // wire-protocol use-site mapping.
    assert_eq!(provider.chain_tip_point(), second_point.to_cbor_bytes());
    assert_ne!(
        provider.chain_tip_point(),
        provider.chain_tip(),
        "chain_tip_point (bare Point) and chain_tip (Tip envelope) MUST be distinct shapes",
    );
}

#[derive(Default)]
struct RecordingTxSubmissionConsumer {
    received: Mutex<Vec<Vec<u8>>>,
}

impl TxSubmissionConsumer for RecordingTxSubmissionConsumer {
    fn consume_txs(&self, txs: Vec<Vec<u8>>) -> usize {
        let accepted = txs.len();
        self.received.lock().expect("poisoned").extend(txs);
        accepted
    }
}

#[derive(Clone)]
struct StaticPeerSharingProvider {
    peers: Vec<SharedPeerAddress>,
}

impl PeerSharingProvider for StaticPeerSharingProvider {
    fn shareable_peers(&self, amount: u16) -> Vec<SharedPeerAddress> {
        let mut peers = self.peers.clone();
        peers.truncate(amount as usize);
        peers
    }
}

#[tokio::test]
async fn inbound_accept_loop_records_responder_egress_metrics() {
    let listener = PeerListener::bind("127.0.0.1:0", 42, vec![HandshakeVersion(15)])
        .await
        .expect("bind listener");
    let listen_addr = listener.local_addr().expect("listen addr");
    let consumer = Arc::new(RecordingTxSubmissionConsumer::default());
    let metrics = Arc::new(yggdrasil_node_tracer::NodeMetrics::new());
    let shared_peer = SocketAddr::from(([10, 0, 0, 1], 3001));
    let peer_sharing_provider = Arc::new(StaticPeerSharingProvider {
        peers: vec![SharedPeerAddress { addr: shared_peer }],
    });

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let accept_task = tokio::spawn({
        let consumer = Arc::clone(&consumer);
        let metrics = Arc::clone(&metrics);
        let peer_sharing_provider = Arc::clone(&peer_sharing_provider);
        async move {
            run_inbound_accept_loop(
                &listener,
                None,
                None,
                Some(consumer),
                Some(peer_sharing_provider),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                Some(&metrics),
                async move {
                    let _ = shutdown_rx.await;
                },
            )
            .await
        }
    });

    // Give the accept loop a chance to start polling before connecting.
    tokio::time::sleep(Duration::from_millis(100)).await;

    let mut session = bootstrap(&NodeConfig {
        peer_addr: listen_addr,
        network_magic: 42,
        protocol_versions: vec![HandshakeVersion(15)],
        peer_sharing: 1,
    })
    .await
    .expect("bootstrap client");

    let keepalive_cookie = 0xBEEFu16;
    session
        .keep_alive
        .keep_alive(keepalive_cookie)
        .await
        .expect("keepalive response");

    let shared = session
        .peer_sharing
        .as_mut()
        .expect("peer sharing client negotiated")
        .share_request(1)
        .await
        .expect("peer sharing response");
    assert_eq!(shared, vec![SharedPeerAddress { addr: shared_peer }]);

    session
        .tx_submission
        .init()
        .await
        .expect("init txsubmission");

    let first_request = session
        .tx_submission
        .recv_request()
        .await
        .expect("recv tx ids request");
    let (ack, req) = match first_request {
        yggdrasil_network::TxServerRequest::RequestTxIds { blocking, ack, req } => {
            assert!(blocking);
            (ack, req)
        }
        other => panic!("expected tx ids request, got {other:?}"),
    };
    assert_eq!(ack, 0);
    assert_eq!(req, 16);

    let txid = yggdrasil_ledger::TxId([7; 32]);
    session
        .tx_submission
        .reply_tx_ids(vec![TxIdAndSize { txid, size: 3 }])
        .await
        .expect("reply tx ids");

    let second_request = session
        .tx_submission
        .recv_request()
        .await
        .expect("recv tx bodies request");
    match second_request {
        yggdrasil_network::TxServerRequest::RequestTxs { txids } => {
            assert_eq!(txids, vec![txid]);
        }
        other => panic!("expected tx request, got {other:?}"),
    }

    session
        .tx_submission
        .reply_txs(vec![vec![1, 2, 3]])
        .await
        .expect("reply tx bodies");

    let third_request = session
        .tx_submission
        .recv_request()
        .await
        .expect("recv follow-up tx ids request");
    match third_request {
        yggdrasil_network::TxServerRequest::RequestTxIds { blocking, ack, req } => {
            assert!(blocking);
            assert_eq!(ack, 1);
            assert_eq!(req, 16);
        }
        other => panic!("expected follow-up tx ids request, got {other:?}"),
    }

    session.tx_submission.send_done().await.expect("send done");

    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        consumer.received.lock().expect("poisoned").clone(),
        vec![vec![1, 2, 3]]
    );
    let expected_keepalive = KeepAliveMessage::MsgKeepAliveResponse {
        cookie: keepalive_cookie,
    }
    .to_cbor()
    .len() as u64;
    let expected_peersharing = PeerSharingMessage::MsgSharePeers {
        peers: vec![SharedPeerAddress { addr: shared_peer }],
    }
    .to_cbor()
    .len() as u64;
    let expected_txsubmission = (TxSubmissionMessage::MsgRequestTxIds {
        blocking: true,
        ack: 0,
        req: 16,
    }
    .to_cbor()
    .len()
        + TxSubmissionMessage::MsgRequestTxs { txids: vec![txid] }
            .to_cbor()
            .len()
        + TxSubmissionMessage::MsgRequestTxIds {
            blocking: true,
            ack: 1,
            req: 16,
        }
        .to_cbor()
        .len()) as u64;
    let snapshot = metrics.snapshot();
    assert_eq!(
        snapshot.keepalive_server_bytes_served_total,
        expected_keepalive
    );
    assert_eq!(
        snapshot.peersharing_server_bytes_served_total,
        expected_peersharing
    );
    assert_eq!(
        snapshot.txsubmission_server_bytes_served_total,
        expected_txsubmission
    );
    let per_peer_total: u64 = metrics
        .peer_lifetime_bytes_out_by_peer()
        .into_iter()
        .map(|(_, bytes)| bytes)
        .sum();
    assert_eq!(
        per_peer_total,
        expected_keepalive + expected_peersharing + expected_txsubmission
    );

    let _ = shutdown_tx.send(());
    accept_task
        .await
        .expect("accept task join")
        .expect("accept loop");
}

#[test]
fn shared_peer_sharing_provider_returns_warm_and_hot_peers() {
    use std::net::SocketAddr;
    use yggdrasil_network::{PeerRegistry, PeerSource, PeerStatus};

    let mut registry = PeerRegistry::default();
    let warm: SocketAddr = "1.2.3.4:3001".parse().unwrap();
    let hot: SocketAddr = "5.6.7.8:3001".parse().unwrap();
    let cold: SocketAddr = "9.10.11.12:3001".parse().unwrap();

    registry.insert_source(warm, PeerSource::PeerSourceBootstrap);
    registry.insert_source(hot, PeerSource::PeerSourceBootstrap);
    registry.insert_source(cold, PeerSource::PeerSourceBootstrap);

    registry.set_status(warm, PeerStatus::PeerWarm);
    registry.set_status(hot, PeerStatus::PeerHot);
    // cold stays PeerCold by default

    let provider = SharedPeerSharingProvider::new(Arc::new(RwLock::new(registry)));
    let peers = provider.shareable_peers(10);

    let addrs: Vec<SocketAddr> = peers.iter().map(|p| p.addr).collect();
    assert!(addrs.contains(&warm), "warm peer should be shareable");
    assert!(addrs.contains(&hot), "hot peer should be shareable");
    assert!(!addrs.contains(&cold), "cold peer should not be shareable");
}

#[test]
fn shared_peer_sharing_provider_respects_amount_limit() {
    use std::net::SocketAddr;
    use yggdrasil_network::{PeerRegistry, PeerSource, PeerStatus};

    let mut registry = PeerRegistry::default();
    for i in 1..=5u8 {
        let addr: SocketAddr = format!("10.0.0.{i}:3001").parse().unwrap();
        registry.insert_source(addr, PeerSource::PeerSourceBootstrap);
        registry.set_status(addr, PeerStatus::PeerWarm);
    }

    let provider = SharedPeerSharingProvider::new(Arc::new(RwLock::new(registry)));
    let peers = provider.shareable_peers(2);
    assert_eq!(peers.len(), 2, "should return at most the requested amount");
}

/// Byzantine peer sends `MsgShareRequest { amount: u16::MAX }` to force
/// a full-registry walk every request.  The server must clamp to
/// `PEER_SHARING_MAX_AMOUNT` (255, matching upstream `Word8`).
#[test]
fn shared_peer_sharing_provider_clamps_to_upstream_word8_max() {
    use std::net::SocketAddr;
    use yggdrasil_network::{PeerRegistry, PeerSource, PeerStatus};

    let mut registry = PeerRegistry::default();
    // Populate enough peers that the request would otherwise return
    // many; populating 300 peers exceeds the 255 cap, so the cap is
    // observable via `peers.len() == 255` not `== 300`.
    for i in 0..300u32 {
        let addr: SocketAddr = format!("10.0.{}.{}:3001", (i / 256) % 256, i % 256)
            .parse()
            .unwrap();
        registry.insert_source(addr, PeerSource::PeerSourceBootstrap);
        registry.set_status(addr, PeerStatus::PeerWarm);
    }

    let provider = SharedPeerSharingProvider::new(Arc::new(RwLock::new(registry)));
    let peers = provider.shareable_peers(u16::MAX);
    assert_eq!(
        peers.len(),
        PEER_SHARING_MAX_AMOUNT as usize,
        "u16::MAX request must be clamped to the upstream Word8 ceiling"
    );

    // Smaller-than-cap requests still work.
    let peers_small = provider.shareable_peers(10);
    assert_eq!(peers_small.len(), 10);
}

type RawChainSyncTip = (Vec<u8>, Vec<u8>, Vec<u8>);

#[derive(Clone)]
struct MockTentativeChainProvider {
    confirmed_tip: Point,
    tentative: Arc<RwLock<Option<RawChainSyncTip>>>,
}

impl ChainProvider for MockTentativeChainProvider {
    fn chain_tip(&self) -> Vec<u8> {
        // R220 — emit upstream `Tip` envelope.  Mock uses
        // `Tip::TipGenesis` for Origin or
        // `Tip::Tip(point, BlockNo(0))` (mock has no real
        // block_no; 0 is a sentinel acceptable by upstream
        // for the test path).
        mock_tip_envelope(self.confirmed_tip)
    }

    fn chain_tip_point(&self) -> Vec<u8> {
        // R221 — bare `Point` for rollback target / cursor seed.
        self.confirmed_tip.to_cbor_bytes()
    }

    fn next_header(&self, _cursor: &Option<Vec<u8>>) -> Option<(Vec<u8>, Vec<u8>, Vec<u8>)> {
        None
    }

    fn find_intersect(&self, points: &[Vec<u8>]) -> Option<(Vec<u8>, Vec<u8>)> {
        let tip_envelope = mock_tip_envelope(self.confirmed_tip);
        points
            .iter()
            .find(|candidate| {
                Point::from_cbor_bytes(candidate)
                    .map(|point| point == self.confirmed_tip)
                    .unwrap_or(false)
            })
            .map(|point| (point.clone(), tip_envelope.clone()))
    }

    fn tentative_tip(&self) -> Option<(Vec<u8>, Vec<u8>, Vec<u8>)> {
        self.tentative.read().ok()?.clone()
    }
}

fn mock_tip_envelope(point: Point) -> Vec<u8> {
    let mut enc = Encoder::new();
    match point {
        Point::Origin => Tip::TipGenesis.encode_cbor(&mut enc),
        p => Tip::Tip(p, BlockNo(0)).encode_cbor(&mut enc),
    }
    enc.into_bytes()
}

#[tokio::test]
async fn chainsync_server_rolls_back_after_tentative_trap() {
    let listener = PeerListener::bind("127.0.0.1:0", 42, vec![HandshakeVersion(15)])
        .await
        .expect("bind listener");
    let listen_addr = listener.local_addr().expect("listen addr");

    let confirmed_point = Point::BlockPoint(SlotNo(1), HeaderHash([0xCD; 32]));
    let tentative_point = Point::BlockPoint(SlotNo(42), HeaderHash([0xAB; 32]));
    let tentative_state = Arc::new(RwLock::new(Some((
        tentative_point.to_cbor_bytes(),
        vec![0x80],
        tentative_point.to_cbor_bytes(),
    ))));

    let provider = Arc::new(MockTentativeChainProvider {
        confirmed_tip: confirmed_point,
        tentative: Arc::clone(&tentative_state),
    });
    let chain_provider: Arc<dyn ChainProvider> = provider;

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let accept_task = tokio::spawn(async move {
        run_inbound_accept_loop(
            &listener,
            None,
            Some(chain_provider),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            async move {
                let _ = shutdown_rx.await;
            },
        )
        .await
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    let mut session = bootstrap(&NodeConfig {
        peer_addr: listen_addr,
        network_magic: 42,
        protocol_versions: vec![HandshakeVersion(15)],
        peer_sharing: 1,
    })
    .await
    .expect("bootstrap client");

    let first = session
        .chain_sync
        .request_next()
        .await
        .expect("first request_next");
    match first {
        NextResponse::RollForward { tip, .. } | NextResponse::AwaitRollForward { tip, .. } => {
            assert_eq!(tip, tentative_point.to_cbor_bytes());
        }
        other => panic!("expected tentative roll-forward, got {other:?}"),
    }

    {
        let mut state = tentative_state.write().expect("tentative state lock");
        *state = None;
    }

    let second = session
        .chain_sync
        .request_next()
        .await
        .expect("second request_next");
    match second {
        NextResponse::RollBackward { point, tip }
        | NextResponse::AwaitRollBackward { point, tip } => {
            // R220 — `point` is bare Point (echoed in
            // `MsgRollBackward`'s first slot); `tip` is the
            // upstream `Tip` envelope (R220 contract).  Pre-R220
            // both pinned `confirmed_point.to_cbor_bytes()`
            // (bare Point) for tip — that asserted the wrong
            // wire shape.
            assert_eq!(point, confirmed_point.to_cbor_bytes());
            assert_eq!(tip, mock_tip_envelope(confirmed_point));
        }
        other => panic!("expected rollback after tentative trap, got {other:?}"),
    }

    session.mux.abort();
    let _ = shutdown_tx.send(());
    accept_task
        .await
        .expect("accept task join")
        .expect("accept loop");
}
