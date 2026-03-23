use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};
use yggdrasil_network::{
    AfterSlot,
    BlockFetchMessage,
    ChainSyncMessage,
    HandshakeVersion,
    TxSubmissionMessage,
    MiniProtocolNum,
    UseLedgerPeers,
    peer_accept,
};
use yggdrasil_ledger::{
    AlonzoCompatibleSubmittedTx, AlonzoTxBody, AlonzoTxOut, Block, BlockHeader, BlockNo,
    ByronBlock, CborEncode, Era, HeaderHash, LedgerError, LedgerState, MultiEraSubmittedTx,
    Point, Encoder, PoolParams, Relay, RewardAccount, ShelleyTx, ShelleyTxBody, ShelleyTxIn,
    ShelleyTxOut, ShelleyWitnessSet, SlotNo, StakeCredential, TxId, UnitInterval, Value,
};
use yggdrasil_mempool::{Mempool, MempoolEntry, SharedMempool};
use yggdrasil_node::{
    LedgerCheckpointPolicy, MempoolAddTxResult, NodeConfig, TxSubmissionServiceOutcome,
    ReconnectingVerifiedSyncRequest, ResumeReconnectingVerifiedSyncRequest,
    add_tx_to_mempool, add_tx_to_shared_mempool, add_txs_to_mempool,
    add_txs_to_shared_mempool, bootstrap, bootstrap_with_fallbacks, run_txsubmission_service,
    ReconnectingSyncServiceOutcome, ResumedSyncServiceOutcome, VerificationConfig,
    VerifiedSyncServiceConfig,
    resume_reconnecting_verified_sync_service_chaindb,
    run_reconnecting_verified_sync_service_chaindb,
    run_reconnecting_verified_sync_service,
    run_txsubmission_service_shared, serve_txsubmission_request_from_mempool,
};
use yggdrasil_storage::{
    ChainDb, ImmutableStore, InMemoryImmutable, InMemoryLedgerStore, InMemoryVolatile,
    VolatileStore,
};

/// Spawn a responder that accepts a connection, then return the listen address.
async fn spawn_responder(magic: u32) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local addr");

    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut conn = peer_accept(stream, magic, &[HandshakeVersion(15)])
            .await
            .expect("accept handshake");

        // Act as server for ChainSync: receive MsgRequestNext, reply MsgRollForward.
        if let Some(mut cs) = conn.protocols.remove(&yggdrasil_network::MiniProtocolNum::CHAIN_SYNC) {
            let raw = cs.recv().await.expect("cs recv");
            let msg = ChainSyncMessage::from_cbor(&raw).expect("cs decode");
            assert_eq!(msg, ChainSyncMessage::MsgRequestNext);
            let reply = ChainSyncMessage::MsgRollForward {
                header: b"hdr".to_vec(),
                tip: b"tip".to_vec(),
            };
            cs.send(reply.to_cbor()).await.expect("cs send");
        }

        // Wait for teardown.
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        conn.mux.abort();
    });

    addr
}

fn build_multi_era_envelope(tag: u64, block_body: &[u8]) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(2).unsigned(tag);
    let mut out = enc.into_bytes();
    out.extend_from_slice(block_body);
    out
}

fn build_byron_ebb_body(epoch: u64, difficulty: u64, prev_hash: &[u8; 32]) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(3);
    enc.array(5);
    enc.unsigned(764824073);
    enc.bytes(prev_hash);
    enc.bytes(&[0u8; 32]);
    enc.array(2).unsigned(epoch);
    enc.array(1).unsigned(difficulty);
    enc.unsigned(0);
    enc.bytes(&[]);
    enc.bytes(&[]);
    enc.into_bytes()
}

async fn spawn_verified_batch_responder(
    magic: u32,
    tip: Point,
    block_bytes: Vec<u8>,
    linger: std::time::Duration,
) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local addr");

    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut conn = peer_accept(stream, magic, &[HandshakeVersion(15)])
            .await
            .expect("accept handshake");

        let mut cs = conn
            .protocols
            .remove(&MiniProtocolNum::CHAIN_SYNC)
            .expect("chainsync handle");
        let mut bf = conn
            .protocols
            .remove(&MiniProtocolNum::BLOCK_FETCH)
            .expect("blockfetch handle");

        let cs_req = cs.recv().await.expect("cs recv");
        let cs_msg = ChainSyncMessage::from_cbor(&cs_req).expect("decode cs request");
        assert_eq!(cs_msg, ChainSyncMessage::MsgRequestNext);

        cs.send(
            ChainSyncMessage::MsgRollForward {
                header: b"byron-hdr".to_vec(),
                tip: tip.to_cbor_bytes(),
            }
            .to_cbor(),
        )
        .await
        .expect("send rollforward");

        let bf_req = bf.recv().await.expect("bf recv");
        let _bf_msg = BlockFetchMessage::from_cbor(&bf_req).expect("decode bf request");

        bf.send(BlockFetchMessage::MsgStartBatch.to_cbor())
            .await
            .expect("start batch");
        bf.send(
            BlockFetchMessage::MsgBlock {
                block: block_bytes,
            }
            .to_cbor(),
        )
        .await
        .expect("send block");
        bf.send(BlockFetchMessage::MsgBatchDone.to_cbor())
            .await
            .expect("batch done");

        tokio::time::sleep(linger).await;
        conn.mux.abort();
    });

    addr
}

async fn spawn_verified_batch_responder_from_point(
    magic: u32,
    expected_lower: Point,
    tip: Point,
    block_bytes: Vec<u8>,
    linger: std::time::Duration,
) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local addr");

    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut conn = peer_accept(stream, magic, &[HandshakeVersion(15)])
            .await
            .expect("accept handshake");

        let mut cs = conn
            .protocols
            .remove(&MiniProtocolNum::CHAIN_SYNC)
            .expect("chainsync handle");
        let mut bf = conn
            .protocols
            .remove(&MiniProtocolNum::BLOCK_FETCH)
            .expect("blockfetch handle");

        let cs_req = cs.recv().await.expect("cs recv");
        let cs_msg = ChainSyncMessage::from_cbor(&cs_req).expect("decode cs request");
        assert_eq!(cs_msg, ChainSyncMessage::MsgRequestNext);

        cs.send(
            ChainSyncMessage::MsgRollForward {
                header: b"byron-hdr".to_vec(),
                tip: tip.to_cbor_bytes(),
            }
            .to_cbor(),
        )
        .await
        .expect("send rollforward");

        let bf_req = bf.recv().await.expect("bf recv");
        let bf_msg = BlockFetchMessage::from_cbor(&bf_req).expect("decode bf request");
        match bf_msg {
            BlockFetchMessage::MsgRequestRange(range) => {
                assert_eq!(range.lower, expected_lower.to_cbor_bytes());
                assert_eq!(range.upper, tip.to_cbor_bytes());
            }
            other => panic!("unexpected blockfetch request: {other:?}"),
        }

        bf.send(BlockFetchMessage::MsgStartBatch.to_cbor())
            .await
            .expect("start batch");
        bf.send(
            BlockFetchMessage::MsgBlock {
                block: block_bytes,
            }
            .to_cbor(),
        )
        .await
        .expect("send block");
        bf.send(BlockFetchMessage::MsgBatchDone.to_cbor())
            .await
            .expect("batch done");

        tokio::time::sleep(linger).await;
        conn.mux.abort();
    });

    addr
}

async fn spawn_disconnect_after_handshake_responder(magic: u32) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local addr");

    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let conn = peer_accept(stream, magic, &[HandshakeVersion(15)])
            .await
            .expect("accept handshake");

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        conn.mux.abort();
    });

    addr
}

fn empty_witness_set() -> ShelleyWitnessSet {
    ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![],
        plutus_data: vec![],
        redeemers: vec![],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![],
    }
}

fn sample_shelley_submitted_tx(seed: u8) -> MultiEraSubmittedTx {
    MultiEraSubmittedTx::Shelley(ShelleyTx {
        body: ShelleyTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [seed; 32],
                index: 0,
            }],
            outputs: vec![ShelleyTxOut {
                address: vec![0x61; 28],
                amount: 2_000_000,
            }],
            fee: 150_000,
            ttl: 123_456,
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        },
        witness_set: empty_witness_set(),
        auxiliary_data: Some(vec![0x81, seed]),
    })
}

fn sample_pool_params_for_addr(addr: SocketAddr, operator: u8) -> PoolParams {
    let relay = match addr.ip() {
        std::net::IpAddr::V4(ipv4) => Relay::SingleHostAddr(Some(addr.port()), Some(ipv4.octets()), None),
        std::net::IpAddr::V6(ipv6) => Relay::SingleHostAddr(Some(addr.port()), None, Some(ipv6.octets())),
    };

    PoolParams {
        operator: [operator; 28],
        vrf_keyhash: [operator; 32],
        pledge: 1,
        cost: 1,
        margin: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        reward_account: RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash([operator; 28]),
        },
        pool_owners: vec![[operator; 28]],
        relays: vec![relay],
        pool_metadata: None,
    }
}

fn write_peer_snapshot_file(contents: &str) -> std::path::PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("yggdrasil-runtime-peer-snapshot-{unique}"));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("peer-snapshot.json");
    std::fs::write(&path, contents).expect("write snapshot");
    path
}

fn sample_alonzo_submitted_tx(seed: u8) -> MultiEraSubmittedTx {
    MultiEraSubmittedTx::Alonzo(AlonzoCompatibleSubmittedTx::new(
        AlonzoTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [seed; 32],
                index: 1,
            }],
            outputs: vec![AlonzoTxOut {
                address: vec![0x61; 28],
                amount: Value::Coin(2_000_000),
                datum_hash: None,
            }],
            fee: 200_000,
            ttl: Some(9_999),
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
            validity_interval_start: None,
            mint: None,
            script_data_hash: None,
            collateral: None,
            required_signers: None,
            network_id: None,
        },
        empty_witness_set(),
        true,
        Some(vec![0x81, seed.wrapping_add(1)]),
    ))
}

fn shelley_submitted_tx_spending(
    input_seed: u8,
    output_seed: u8,
    output_amount: u64,
    fee: u64,
) -> MultiEraSubmittedTx {
    MultiEraSubmittedTx::Shelley(ShelleyTx {
        body: ShelleyTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: [input_seed; 32],
                index: 0,
            }],
            outputs: vec![ShelleyTxOut {
                address: vec![output_seed; 28],
                amount: output_amount,
            }],
            fee,
            ttl: 123_456,
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        },
        witness_set: empty_witness_set(),
        auxiliary_data: None,
    })
}

fn shelley_submitted_tx_dependent(
    parent_tx_id: TxId,
    output_seed: u8,
    output_amount: u64,
    fee: u64,
) -> MultiEraSubmittedTx {
    MultiEraSubmittedTx::Shelley(ShelleyTx {
        body: ShelleyTxBody {
            inputs: vec![ShelleyTxIn {
                transaction_id: parent_tx_id.0,
                index: 0,
            }],
            outputs: vec![ShelleyTxOut {
                address: vec![output_seed; 28],
                amount: output_amount,
            }],
            fee,
            ttl: 123_456,
            certificates: None,
            withdrawals: None,
            update: None,
            auxiliary_data_hash: None,
        },
        witness_set: empty_witness_set(),
        auxiliary_data: None,
    })
}

fn seed_shelley_input(state: &mut LedgerState, seed: u8, amount: u64) {
    state.utxo_mut().insert(
        ShelleyTxIn {
            transaction_id: [seed; 32],
            index: 0,
        },
        ShelleyTxOut {
            address: vec![0x42; 28],
            amount,
        },
    );
}

async fn spawn_txsubmission_responder(magic: u32, expected_txids: Vec<[u8; 32]>, expected_txs: Vec<Vec<u8>>) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local addr");

    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut conn = peer_accept(stream, magic, &[HandshakeVersion(15)])
            .await
            .expect("accept handshake");

        let mut tx = conn
            .protocols
            .remove(&yggdrasil_network::MiniProtocolNum::TX_SUBMISSION)
            .expect("txsubmission handle");

        let raw = tx.recv().await.expect("recv init");
        let msg = TxSubmissionMessage::from_cbor(&raw).expect("decode init");
        assert_eq!(msg, TxSubmissionMessage::MsgInit);

        tx.send(
            TxSubmissionMessage::MsgRequestTxIds {
                blocking: true,
                ack: 0,
                req: expected_txids.len() as u16,
            }
            .to_cbor(),
        )
        .await
        .expect("send request txids");

        let raw = tx.recv().await.expect("recv reply txids");
        let msg = TxSubmissionMessage::from_cbor(&raw).expect("decode reply txids");
        let txids = match msg {
            TxSubmissionMessage::MsgReplyTxIds { txids } => txids,
            other => panic!("expected MsgReplyTxIds, got {other:?}"),
        };
        assert_eq!(txids.len(), expected_txids.len());
        for (actual, expected) in txids.iter().zip(expected_txids.iter()) {
            assert_eq!(actual.txid.0, *expected);
        }

        tx.send(
            TxSubmissionMessage::MsgRequestTxs {
                txids: txids.iter().map(|item| item.txid).collect(),
            }
            .to_cbor(),
        )
        .await
        .expect("send request txs");

        let raw = tx.recv().await.expect("recv reply txs");
        let msg = TxSubmissionMessage::from_cbor(&raw).expect("decode reply txs");
        let txs = match msg {
            TxSubmissionMessage::MsgReplyTxs { txs } => txs,
            other => panic!("expected MsgReplyTxs, got {other:?}"),
        };
        assert_eq!(txs, expected_txs);

        tx.send(
            TxSubmissionMessage::MsgRequestTxIds {
                blocking: true,
                ack: expected_txids.len() as u16,
                req: 1,
            }
            .to_cbor(),
        )
        .await
        .expect("send final request txids");

        let raw = tx.recv().await.expect("recv done");
        let msg = TxSubmissionMessage::from_cbor(&raw).expect("decode done");
        assert_eq!(msg, TxSubmissionMessage::MsgDone);

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        conn.mux.abort();
    });

    addr
}

async fn spawn_txsubmission_idle_responder(magic: u32) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local addr");

    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut conn = peer_accept(stream, magic, &[HandshakeVersion(15)])
            .await
            .expect("accept handshake");

        let mut tx = conn
            .protocols
            .remove(&yggdrasil_network::MiniProtocolNum::TX_SUBMISSION)
            .expect("txsubmission handle");

        let raw = tx.recv().await.expect("recv init");
        let msg = TxSubmissionMessage::from_cbor(&raw).expect("decode init");
        assert_eq!(msg, TxSubmissionMessage::MsgInit);

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        conn.mux.abort();
    });

    addr
}

async fn spawn_txsubmission_delayed_responder(
    magic: u32,
    expected_txids: Vec<[u8; 32]>,
    expected_txs: Vec<Vec<u8>>,
    delay: std::time::Duration,
) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local addr");

    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut conn = peer_accept(stream, magic, &[HandshakeVersion(15)])
            .await
            .expect("accept handshake");

        let mut tx = conn
            .protocols
            .remove(&yggdrasil_network::MiniProtocolNum::TX_SUBMISSION)
            .expect("txsubmission handle");

        let raw = tx.recv().await.expect("recv init");
        let msg = TxSubmissionMessage::from_cbor(&raw).expect("decode init");
        assert_eq!(msg, TxSubmissionMessage::MsgInit);

        tokio::time::sleep(delay).await;

        tx.send(
            TxSubmissionMessage::MsgRequestTxIds {
                blocking: true,
                ack: 0,
                req: expected_txids.len() as u16,
            }
            .to_cbor(),
        )
        .await
        .expect("send request txids");

        let raw = tx.recv().await.expect("recv reply txids");
        let msg = TxSubmissionMessage::from_cbor(&raw).expect("decode reply txids");
        let txids = match msg {
            TxSubmissionMessage::MsgReplyTxIds { txids } => txids,
            other => panic!("expected MsgReplyTxIds, got {other:?}"),
        };
        assert_eq!(txids.len(), expected_txids.len());
        for (actual, expected) in txids.iter().zip(expected_txids.iter()) {
            assert_eq!(actual.txid.0, *expected);
        }

        tx.send(
            TxSubmissionMessage::MsgRequestTxs {
                txids: txids.iter().map(|item| item.txid).collect(),
            }
            .to_cbor(),
        )
        .await
        .expect("send request txs");

        let raw = tx.recv().await.expect("recv reply txs");
        let msg = TxSubmissionMessage::from_cbor(&raw).expect("decode reply txs");
        let txs = match msg {
            TxSubmissionMessage::MsgReplyTxs { txs } => txs,
            other => panic!("expected MsgReplyTxs, got {other:?}"),
        };
        assert_eq!(txs, expected_txs);

        tx.send(
            TxSubmissionMessage::MsgRequestTxIds {
                blocking: true,
                ack: expected_txids.len() as u16,
                req: 1,
            }
            .to_cbor(),
        )
        .await
        .expect("send final request txids");

        let raw = tx.recv().await.expect("recv done");
        let msg = TxSubmissionMessage::from_cbor(&raw).expect("decode done");
        assert_eq!(msg, TxSubmissionMessage::MsgDone);

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        conn.mux.abort();
    });

    addr
}

#[tokio::test]
async fn runtime_bootstrap_creates_all_drivers() {
    let magic = 42;
    let addr = spawn_responder(magic).await;

    let config = NodeConfig {
        peer_addr: addr,
        network_magic: magic,
        protocol_versions: vec![HandshakeVersion(15)],
    };

    let mut session = bootstrap(&config).await.expect("bootstrap");

    assert_eq!(session.connected_peer_addr, addr);
    assert_eq!(session.version, HandshakeVersion(15));
    assert_eq!(session.version_data.network_magic, magic);

    // Use the ChainSync client to request_next.
    let resp = session.chain_sync.request_next().await.expect("cs request_next");
    assert!(matches!(
        resp,
        yggdrasil_network::NextResponse::RollForward { .. }
    ));

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    session.mux.abort();
}

#[tokio::test]
async fn runtime_bootstrap_uses_fallback_peer_when_primary_fails() {
    let magic = 43;
    let good_addr = spawn_responder(magic).await;
    let bad_addr: SocketAddr = "127.0.0.1:1".parse().expect("bad addr");

    let config = NodeConfig {
        peer_addr: bad_addr,
        network_magic: magic,
        protocol_versions: vec![HandshakeVersion(15)],
    };

    let mut session = bootstrap_with_fallbacks(&config, &[good_addr])
        .await
        .expect("bootstrap via fallback");

    assert_eq!(session.connected_peer_addr, good_addr);
    assert_eq!(session.version, HandshakeVersion(15));

    let resp = session.chain_sync.request_next().await.expect("cs request_next");
    assert!(matches!(
        resp,
        yggdrasil_network::NextResponse::RollForward { .. }
    ));

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    session.mux.abort();
}

#[tokio::test]
async fn runtime_reconnecting_verified_sync_service_rotates_peers() {
    let magic = 44;
    let block_one_body = build_byron_ebb_body(1, 1, &[0x11; 32]);
    let block_two_body = build_byron_ebb_body(2, 2, &[0x22; 32]);
    let block_one = build_multi_era_envelope(0, &block_one_body);
    let block_two = build_multi_era_envelope(0, &block_two_body);
    let tip_one = Point::BlockPoint(
        SlotNo(21_600),
        ByronBlock::decode_ebb(&block_one_body).expect("decode block one").header_hash(),
    );
    let tip_two = Point::BlockPoint(
        SlotNo(43_200),
        ByronBlock::decode_ebb(&block_two_body).expect("decode block two").header_hash(),
    );

    let first_addr = spawn_verified_batch_responder(
        magic,
        tip_one,
        block_one,
        std::time::Duration::from_millis(10),
    )
    .await;
    let second_addr = spawn_verified_batch_responder(
        magic,
        tip_two,
        block_two,
        std::time::Duration::from_secs(2),
    )
    .await;

    let node_config = NodeConfig {
        peer_addr: first_addr,
        network_magic: magic,
        protocol_versions: vec![HandshakeVersion(15)],
    };
    let service_config = VerifiedSyncServiceConfig {
        batch_size: 1,
        verification: VerificationConfig {
            slots_per_kes_period: 129_600,
            max_kes_evolutions: 62,
            verify_body_hash: true,
        },
        nonce_config: None,
        security_param: None,
        checkpoint_policy: LedgerCheckpointPolicy::default(),
        plutus_cost_model: None,
    };
    let mut store = InMemoryVolatile::default();

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
        let _ = shutdown_tx.send(());
    });

    let outcome: ReconnectingSyncServiceOutcome = run_reconnecting_verified_sync_service(
        &mut store,
        ReconnectingVerifiedSyncRequest::new(
            &node_config,
            &[second_addr],
            Point::Origin,
            LedgerState::new(Era::Byron),
            &service_config,
        ),
        async { let _ = shutdown_rx.await; },
    )
    .await
    .expect("reconnecting verified sync service");

    assert_eq!(outcome.total_blocks, 2);
    assert_eq!(outcome.reconnect_count, 1);
    assert_eq!(outcome.final_point, tip_two);
    assert_eq!(outcome.last_connected_peer_addr, Some(second_addr));
    assert_eq!(store.tip(), tip_two);
}

#[tokio::test]
async fn runtime_reconnecting_verified_sync_service_chaindb_rotates_peers() {
    let magic = 77;

    let block_one = build_multi_era_envelope(0, &build_byron_ebb_body(0, 1, &[0; 32]));
    let tip_one = Point::BlockPoint(
        SlotNo(0),
        ByronBlock::decode_ebb(&block_one[2..]).expect("decode ebb 1").header_hash(),
    );
    let first_addr = spawn_verified_batch_responder(
        magic,
        tip_one,
        block_one,
        std::time::Duration::from_millis(300),
    )
    .await;

    let block_two = build_multi_era_envelope(0, &build_byron_ebb_body(0, 2, &[0; 32]));
    let tip_two = Point::BlockPoint(
        SlotNo(0),
        ByronBlock::decode_ebb(&block_two[2..]).expect("decode ebb 2").header_hash(),
    );
    let second_addr = spawn_verified_batch_responder(
        magic,
        tip_two,
        block_two,
        std::time::Duration::from_secs(2),
    )
    .await;

    let node_config = NodeConfig {
        peer_addr: first_addr,
        network_magic: magic,
        protocol_versions: vec![HandshakeVersion(15)],
    };
    let service_config = VerifiedSyncServiceConfig {
        batch_size: 1,
        verification: VerificationConfig {
            slots_per_kes_period: 129_600,
            max_kes_evolutions: 62,
            verify_body_hash: true,
        },
        nonce_config: None,
        security_param: Some(yggdrasil_consensus::SecurityParam(1)),
        checkpoint_policy: LedgerCheckpointPolicy::default(),
        plutus_cost_model: None,
    };
    let mut chain_db = ChainDb::new(
        InMemoryImmutable::default(),
        InMemoryVolatile::default(),
        InMemoryLedgerStore::default(),
    );

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
        let _ = shutdown_tx.send(());
    });

    let outcome: ReconnectingSyncServiceOutcome = run_reconnecting_verified_sync_service_chaindb(
        &mut chain_db,
        ReconnectingVerifiedSyncRequest::new(
            &node_config,
            &[second_addr],
            Point::Origin,
            LedgerState::new(Era::Byron),
            &service_config,
        ),
        async { let _ = shutdown_rx.await; },
    )
    .await
    .expect("reconnecting verified sync service via chaindb");

    assert_eq!(outcome.total_blocks, 2);
    assert_eq!(outcome.reconnect_count, 1);
    assert_eq!(outcome.final_point, tip_two);
    assert_eq!(outcome.last_connected_peer_addr, Some(second_addr));
    assert_eq!(outcome.stable_block_count, 1);
    assert_eq!(chain_db.immutable().len(), 1);
    assert_eq!(chain_db.volatile().tip(), tip_two);

    let (checkpoint_slot, checkpoint) = chain_db
        .latest_ledger_checkpoint()
        .expect("decode checkpoint")
        .expect("checkpoint persisted after chaindb sync");
    assert_eq!(checkpoint_slot, SlotNo(0));
    assert_eq!(checkpoint.restore().tip, tip_two);
}

#[tokio::test]
async fn runtime_resume_reconnecting_verified_sync_service_chaindb_uses_recovered_point() {
    let magic = 78;

    let mut checkpoint_state = LedgerState::new(Era::Byron);
    let recovered_point = Point::BlockPoint(SlotNo(0), HeaderHash([0x11; 32]));
    checkpoint_state.tip = recovered_point;

    let block_two = build_multi_era_envelope(0, &build_byron_ebb_body(0, 2, &[0; 32]));
    let tip_two = Point::BlockPoint(
        SlotNo(0),
        ByronBlock::decode_ebb(&block_two[2..]).expect("decode ebb 2").header_hash(),
    );
    let addr = spawn_verified_batch_responder_from_point(
        magic,
        recovered_point,
        tip_two,
        block_two,
        std::time::Duration::from_secs(2),
    )
    .await;

    let node_config = NodeConfig {
        peer_addr: addr,
        network_magic: magic,
        protocol_versions: vec![HandshakeVersion(15)],
    };
    let service_config = VerifiedSyncServiceConfig {
        batch_size: 1,
        verification: VerificationConfig {
            slots_per_kes_period: 129_600,
            max_kes_evolutions: 62,
            verify_body_hash: true,
        },
        nonce_config: None,
        security_param: Some(yggdrasil_consensus::SecurityParam(1)),
        checkpoint_policy: LedgerCheckpointPolicy::default(),
        plutus_cost_model: None,
    };
    let mut chain_db = ChainDb::new(
        InMemoryImmutable::default(),
        InMemoryVolatile::default(),
        InMemoryLedgerStore::default(),
    );
    chain_db
        .add_volatile_block(Block {
            era: Era::Byron,
            header: BlockHeader {
                hash: recovered_point.hash().expect("recovered hash"),
                prev_hash: HeaderHash([0; 32]),
                slot_no: SlotNo(0),
                block_no: BlockNo(0),
                issuer_vkey: [0; 32],
            },
            transactions: Vec::new(),
            raw_cbor: None,
        })
        .expect("seed coordinated tip");
    chain_db
        .save_ledger_checkpoint(SlotNo(0), &checkpoint_state.checkpoint())
        .expect("save checkpoint");

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let _ = shutdown_tx.send(());
    });

    let outcome: ResumedSyncServiceOutcome = resume_reconnecting_verified_sync_service_chaindb(
        &mut chain_db,
        ResumeReconnectingVerifiedSyncRequest::new(
            &node_config,
            &[],
            LedgerState::new(Era::Byron),
            &service_config,
        ),
        async { let _ = shutdown_rx.await; },
    )
    .await
    .expect("resume reconnecting verified sync service via chaindb");

    assert_eq!(outcome.recovery.point, recovered_point);
    assert_eq!(outcome.sync.final_point, tip_two);
    assert_eq!(outcome.sync.total_blocks, 1);

    let (checkpoint_slot, checkpoint) = chain_db
        .latest_ledger_checkpoint()
        .expect("decode checkpoint")
        .expect("checkpoint persisted after resumed sync");
    assert_eq!(checkpoint_slot, SlotNo(0));
    assert_eq!(checkpoint.restore().tip, tip_two);
}

#[tokio::test]
async fn runtime_resume_reconnecting_verified_sync_service_chaindb_refreshes_ledger_peers_on_reconnect() {
    let magic = 79;

    let recovered_point = Point::BlockPoint(SlotNo(0), HeaderHash([0x11; 32]));
    let first_addr = spawn_disconnect_after_handshake_responder(magic).await;

    let block_two = build_multi_era_envelope(0, &build_byron_ebb_body(0, 2, &[0; 32]));
    let tip_two = Point::BlockPoint(
        SlotNo(0),
        ByronBlock::decode_ebb(&block_two[2..]).expect("decode ebb 2").header_hash(),
    );
    let second_addr = spawn_verified_batch_responder_from_point(
        magic,
        recovered_point,
        tip_two,
        block_two,
        std::time::Duration::from_secs(2),
    )
    .await;

    let node_config = NodeConfig {
        peer_addr: first_addr,
        network_magic: magic,
        protocol_versions: vec![HandshakeVersion(15)],
    };
    let service_config = VerifiedSyncServiceConfig {
        batch_size: 1,
        verification: VerificationConfig {
            slots_per_kes_period: 129_600,
            max_kes_evolutions: 62,
            verify_body_hash: true,
        },
        nonce_config: None,
        security_param: Some(yggdrasil_consensus::SecurityParam(1)),
        checkpoint_policy: LedgerCheckpointPolicy::default(),
        plutus_cost_model: None,
    };

    let mut checkpoint_state = LedgerState::new(Era::Byron);
    checkpoint_state.tip = recovered_point;
    checkpoint_state
        .pool_state_mut()
        .register(sample_pool_params_for_addr(second_addr, 0x44));

    let mut chain_db = ChainDb::new(
        InMemoryImmutable::default(),
        InMemoryVolatile::default(),
        InMemoryLedgerStore::default(),
    );
    chain_db
        .add_volatile_block(Block {
            era: Era::Byron,
            header: BlockHeader {
                hash: recovered_point.hash().expect("recovered hash"),
                prev_hash: HeaderHash([0; 32]),
                slot_no: SlotNo(0),
                block_no: BlockNo(0),
                issuer_vkey: [0; 32],
            },
            transactions: Vec::new(),
            raw_cbor: None,
        })
        .expect("seed coordinated tip");
    chain_db
        .save_ledger_checkpoint(SlotNo(0), &checkpoint_state.checkpoint())
        .expect("save checkpoint");

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
        let _ = shutdown_tx.send(());
    });

    let request = ResumeReconnectingVerifiedSyncRequest::new(
        &node_config,
        &[],
        LedgerState::new(Era::Byron),
        &service_config,
    )
    .with_use_ledger_peers(Some(UseLedgerPeers::UseLedgerPeers(AfterSlot::Always)));
    let outcome: ResumedSyncServiceOutcome = resume_reconnecting_verified_sync_service_chaindb(
        &mut chain_db,
        request,
        async { let _ = shutdown_rx.await; },
    )
    .await
    .expect("resume reconnecting verified sync service via ledger peers");

    assert_eq!(outcome.recovery.point, recovered_point);
    assert_eq!(outcome.sync.final_point, tip_two);
    assert_eq!(outcome.sync.reconnect_count, 1);
    assert_eq!(outcome.sync.last_connected_peer_addr, Some(second_addr));
}

#[tokio::test]
async fn runtime_resume_reconnecting_verified_sync_service_chaindb_refreshes_snapshot_peers_on_reconnect() {
    let magic = 80;

    let recovered_point = Point::BlockPoint(SlotNo(0), HeaderHash([0x22; 32]));
    let first_addr = spawn_disconnect_after_handshake_responder(magic).await;

    let block_two = build_multi_era_envelope(0, &build_byron_ebb_body(0, 2, &[0; 32]));
    let tip_two = Point::BlockPoint(
        SlotNo(0),
        ByronBlock::decode_ebb(&block_two[2..]).expect("decode ebb 2").header_hash(),
    );
    let second_addr = spawn_verified_batch_responder_from_point(
        magic,
        recovered_point,
        tip_two,
        block_two,
        std::time::Duration::from_secs(2),
    )
    .await;

    let snapshot_path = write_peer_snapshot_file(&format!(
        r#"{{
            "version": 2,
            "slotNo": 0,
            "bigLedgerPools": [
                {{
                    "accumulatedStake": 0.75,
                    "relativeStake": 0.50,
                    "relays": [
                        {{ "address": "{}", "port": {} }}
                    ]
                }}
            ]
        }}"#,
        second_addr.ip(),
        second_addr.port(),
    ));

    let node_config = NodeConfig {
        peer_addr: first_addr,
        network_magic: magic,
        protocol_versions: vec![HandshakeVersion(15)],
    };
    let service_config = VerifiedSyncServiceConfig {
        batch_size: 1,
        verification: VerificationConfig {
            slots_per_kes_period: 129_600,
            max_kes_evolutions: 62,
            verify_body_hash: true,
        },
        nonce_config: None,
        security_param: Some(yggdrasil_consensus::SecurityParam(1)),
        checkpoint_policy: LedgerCheckpointPolicy::default(),
        plutus_cost_model: None,
    };

    let mut checkpoint_state = LedgerState::new(Era::Byron);
    checkpoint_state.tip = recovered_point;

    let mut chain_db = ChainDb::new(
        InMemoryImmutable::default(),
        InMemoryVolatile::default(),
        InMemoryLedgerStore::default(),
    );
    chain_db
        .add_volatile_block(Block {
            era: Era::Byron,
            header: BlockHeader {
                hash: recovered_point.hash().expect("recovered hash"),
                prev_hash: HeaderHash([0; 32]),
                slot_no: SlotNo(0),
                block_no: BlockNo(0),
                issuer_vkey: [0; 32],
            },
            transactions: Vec::new(),
            raw_cbor: None,
        })
        .expect("seed coordinated tip");
    chain_db
        .save_ledger_checkpoint(SlotNo(0), &checkpoint_state.checkpoint())
        .expect("save checkpoint");

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
        let _ = shutdown_tx.send(());
    });

    let request = ResumeReconnectingVerifiedSyncRequest::new(
        &node_config,
        &[],
        LedgerState::new(Era::Byron),
        &service_config,
    )
    .with_use_ledger_peers(Some(UseLedgerPeers::UseLedgerPeers(AfterSlot::Always)))
    .with_peer_snapshot_path(Some(snapshot_path.clone()));
    let outcome: ResumedSyncServiceOutcome = resume_reconnecting_verified_sync_service_chaindb(
        &mut chain_db,
        request,
        async { let _ = shutdown_rx.await; },
    )
    .await
    .expect("resume reconnecting verified sync service via snapshot peers");

    assert_eq!(outcome.recovery.point, recovered_point);
    assert_eq!(outcome.sync.final_point, tip_two);
    assert_eq!(outcome.sync.reconnect_count, 1);
    assert_eq!(outcome.sync.last_connected_peer_addr, Some(second_addr));

    std::fs::remove_file(&snapshot_path).ok();
    if let Some(parent) = snapshot_path.parent() {
        std::fs::remove_dir_all(parent).ok();
    }
}

#[tokio::test]
async fn runtime_serves_txsubmission_requests_from_mempool() {
    let magic = 52;
    let shelley = sample_shelley_submitted_tx(0x21);
    let alonzo = sample_alonzo_submitted_tx(0x42);
    let expected_txids = vec![alonzo.tx_id().0, shelley.tx_id().0];
    let expected_txs = vec![alonzo.raw_cbor(), shelley.raw_cbor()];
    let addr = spawn_txsubmission_responder(magic, expected_txids, expected_txs).await;

    let config = NodeConfig {
        peer_addr: addr,
        network_magic: magic,
        protocol_versions: vec![HandshakeVersion(15)],
    };

    let mut session = bootstrap(&config).await.expect("bootstrap");
    session.tx_submission.init().await.expect("txsubmission init");

    let mut mempool = Mempool::with_capacity(1_000_000);
    mempool
        .insert(MempoolEntry::from_multi_era_submitted_tx(
            shelley,
            100,
            SlotNo(123_456),
        ))
        .expect("insert shelley entry");
    mempool
        .insert(MempoolEntry::from_multi_era_submitted_tx(
            alonzo,
            200,
            SlotNo(234_567),
        ))
        .expect("insert alonzo entry");

    assert!(serve_txsubmission_request_from_mempool(&mut session.tx_submission, &mempool)
        .await
        .expect("reply txids"));
    assert!(serve_txsubmission_request_from_mempool(&mut session.tx_submission, &mempool)
        .await
        .expect("reply txs"));

    let empty = Mempool::with_capacity(1_000_000);
    assert!(!serve_txsubmission_request_from_mempool(&mut session.tx_submission, &empty)
        .await
        .expect("send done"));

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    session.mux.abort();
}

#[tokio::test]
async fn runtime_txsubmission_service_runs_to_protocol_completion() {
    let magic = 62;
    let shelley = sample_shelley_submitted_tx(0x31);
    let alonzo = sample_alonzo_submitted_tx(0x52);
    let expected_txids = vec![shelley.tx_id().0, alonzo.tx_id().0];
    let expected_txs = vec![shelley.raw_cbor(), alonzo.raw_cbor()];
    let addr = spawn_txsubmission_responder(magic, expected_txids, expected_txs).await;

    let config = NodeConfig {
        peer_addr: addr,
        network_magic: magic,
        protocol_versions: vec![HandshakeVersion(15)],
    };

    let mut session = bootstrap(&config).await.expect("bootstrap");
    let mut mempool = Mempool::with_capacity(1_000_000);
    mempool
        .insert(MempoolEntry::from_multi_era_submitted_tx(
            shelley,
            100,
            SlotNo(123_456),
        ))
        .expect("insert shelley entry");
    mempool
        .insert(MempoolEntry::from_multi_era_submitted_tx(
            alonzo,
            200,
            SlotNo(234_567),
        ))
        .expect("insert alonzo entry");

    let outcome: TxSubmissionServiceOutcome = run_txsubmission_service(
        &mut session.tx_submission,
        &mempool,
        std::future::pending::<()>(),
    )
    .await
    .expect("run txsubmission service");

    assert_eq!(
        outcome,
        TxSubmissionServiceOutcome {
            handled_requests: 3,
            terminated_by_protocol: true,
        }
    );

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    session.mux.abort();
}

#[tokio::test]
async fn runtime_txsubmission_service_stops_on_shutdown() {
    let magic = 63;
    let addr = spawn_txsubmission_idle_responder(magic).await;

    let config = NodeConfig {
        peer_addr: addr,
        network_magic: magic,
        protocol_versions: vec![HandshakeVersion(15)],
    };

    let mut session = bootstrap(&config).await.expect("bootstrap");
    let mempool = Mempool::with_capacity(1_000_000);

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let _ = shutdown_tx.send(());
    });

    let outcome: TxSubmissionServiceOutcome = run_txsubmission_service(
        &mut session.tx_submission,
        &mempool,
        async {
            let _ = shutdown_rx.await;
        },
    )
    .await
    .expect("run txsubmission service with shutdown");

    assert_eq!(
        outcome,
        TxSubmissionServiceOutcome {
            handled_requests: 0,
            terminated_by_protocol: false,
        }
    );

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    session.mux.abort();
}

#[test]
fn runtime_add_tx_to_mempool_accepts_valid_tx_and_updates_ledger() {
    let mut ledger = LedgerState::new(Era::Shelley);
    seed_shelley_input(&mut ledger, 0x71, 2_150_000);

    let tx = sample_shelley_submitted_tx(0x71);
    let tx_id = tx.tx_id();
    let mut mempool = Mempool::with_capacity(1_000_000);

    let result = add_tx_to_mempool(&mut ledger, &mut mempool, tx, SlotNo(500))
        .expect("add tx to mempool");

    assert_eq!(result, MempoolAddTxResult::MempoolTxAdded(tx_id));
    assert!(mempool.contains(&tx_id));
    assert!(
        ledger
            .utxo()
            .get(&ShelleyTxIn {
                transaction_id: [0x71; 32],
                index: 0,
            })
            .is_none()
    );
    assert!(
        ledger
            .utxo()
            .get(&ShelleyTxIn {
                transaction_id: tx_id.0,
                index: 0,
            })
            .is_some()
    );
}

#[test]
fn runtime_add_tx_to_shared_mempool_rejects_invalid_tx_without_mutation() {
    let mut ledger = LedgerState::new(Era::Shelley);
    let mempool = SharedMempool::with_capacity(1_000_000);
    let tx = sample_shelley_submitted_tx(0x72);
    let tx_id = tx.tx_id();

    let result = add_tx_to_shared_mempool(&mut ledger, &mempool, tx, SlotNo(500))
        .expect("reject invalid tx cleanly");

    assert_eq!(
        result,
        MempoolAddTxResult::MempoolTxRejected(tx_id, LedgerError::InputNotInUtxo)
    );
    assert_eq!(ledger, LedgerState::new(Era::Shelley));
    assert_eq!(mempool.len(), 0);
}

#[test]
fn runtime_add_txs_to_mempool_accepts_dependent_transactions_in_order() {
    let mut ledger = LedgerState::new(Era::Shelley);
    seed_shelley_input(&mut ledger, 0x81, 2_150_000);

    let parent = shelley_submitted_tx_spending(0x81, 0x91, 2_000_000, 150_000);
    let parent_id = parent.tx_id();
    let child = shelley_submitted_tx_dependent(parent_id, 0x92, 1_900_000, 100_000);
    let child_id = child.tx_id();
    let mut mempool = Mempool::with_capacity(1_000_000);

    let results = add_txs_to_mempool(&mut ledger, &mut mempool, vec![parent, child], SlotNo(500))
        .expect("add dependent tx batch");

    assert_eq!(
        results,
        vec![
            MempoolAddTxResult::MempoolTxAdded(parent_id),
            MempoolAddTxResult::MempoolTxAdded(child_id),
        ]
    );
    assert!(mempool.contains(&parent_id));
    assert!(mempool.contains(&child_id));
    assert!(
        ledger
            .utxo()
            .get(&ShelleyTxIn {
                transaction_id: parent_id.0,
                index: 0,
            })
            .is_none()
    );
    assert!(
        ledger
            .utxo()
            .get(&ShelleyTxIn {
                transaction_id: child_id.0,
                index: 0,
            })
            .is_some()
    );
}

#[test]
fn runtime_add_txs_to_shared_mempool_matches_repeated_single_adds() {
    let mut batch_ledger = LedgerState::new(Era::Shelley);
    let mut single_ledger = LedgerState::new(Era::Shelley);
    seed_shelley_input(&mut batch_ledger, 0x82, 2_150_000);
    seed_shelley_input(&mut single_ledger, 0x82, 2_150_000);

    let parent = shelley_submitted_tx_spending(0x82, 0x93, 2_000_000, 150_000);
    let parent_id = parent.tx_id();
    let child = shelley_submitted_tx_dependent(parent_id, 0x94, 1_900_000, 100_000);
    let parent_single = parent.clone();
    let child_single = child.clone();

    let batch_mempool = SharedMempool::with_capacity(1_000_000);
    let single_mempool = SharedMempool::with_capacity(1_000_000);

    let batch_results = add_txs_to_shared_mempool(
        &mut batch_ledger,
        &batch_mempool,
        vec![parent, child],
        SlotNo(500),
    )
    .expect("batch add to shared mempool");

    let single_results = vec![
        add_tx_to_shared_mempool(&mut single_ledger, &single_mempool, parent_single, SlotNo(500))
            .expect("single parent add"),
        add_tx_to_shared_mempool(&mut single_ledger, &single_mempool, child_single, SlotNo(500))
            .expect("single child add"),
    ];

    assert_eq!(batch_results, single_results);
    assert_eq!(batch_ledger, single_ledger);
    assert_eq!(batch_mempool.snapshot(), single_mempool.snapshot());
}

#[tokio::test]
async fn runtime_txsubmission_service_shared_observes_concurrent_insert() {
    let magic = 64;
    let shelley = sample_shelley_submitted_tx(0x61);
    let expected_txids = vec![shelley.tx_id().0];
    let expected_txs = vec![shelley.raw_cbor()];
    let addr = spawn_txsubmission_delayed_responder(
        magic,
        expected_txids,
        expected_txs,
        std::time::Duration::from_millis(75),
    )
    .await;

    let config = NodeConfig {
        peer_addr: addr,
        network_magic: magic,
        protocol_versions: vec![HandshakeVersion(15)],
    };

    let mut session = bootstrap(&config).await.expect("bootstrap");
    let mempool = SharedMempool::with_capacity(1_000_000);
    let writer = mempool.clone();

    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        writer
            .insert(MempoolEntry::from_multi_era_submitted_tx(
                shelley,
                100,
                SlotNo(123_456),
            ))
            .expect("insert shelley entry");
    });

    let outcome: TxSubmissionServiceOutcome = run_txsubmission_service_shared(
        &mut session.tx_submission,
        &mempool,
        std::future::pending::<()>(),
    )
    .await
    .expect("run shared txsubmission service");

    assert_eq!(
        outcome,
        TxSubmissionServiceOutcome {
            handled_requests: 3,
            terminated_by_protocol: true,
        }
    );
    assert_eq!(mempool.len(), 1);

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    session.mux.abort();
}
