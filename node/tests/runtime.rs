use std::net::SocketAddr;
use yggdrasil_network::{
    ChainSyncMessage,
    HandshakeVersion,
    TxSubmissionMessage,
    peer_accept,
};
use yggdrasil_ledger::{
    AlonzoCompatibleSubmittedTx, AlonzoTxBody, AlonzoTxOut, MultiEraSubmittedTx, ShelleyTx,
    ShelleyTxBody, ShelleyTxIn, ShelleyTxOut, ShelleyWitnessSet, SlotNo, Value,
};
use yggdrasil_mempool::{Mempool, MempoolEntry};
use yggdrasil_node::{
    NodeConfig, TxSubmissionServiceOutcome, bootstrap, run_txsubmission_service,
    serve_txsubmission_request_from_mempool,
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
