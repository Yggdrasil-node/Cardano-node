#![allow(clippy::unwrap_used)]
use std::net::SocketAddr;

use yggdrasil_consensus::{ChainState, SecurityParam};
use yggdrasil_ledger::{
    AlonzoBlock, BabbageBlock, BabbageTxBody, BabbageTxOut, Block, BlockHeader, BlockNo,
    ByronBlock, CborEncode, ConwayBlock, ConwayTxBody, Encoder, Era, HeaderHash, LedgerState,
    Nonce, Point, PraosHeader, PraosHeaderBody, ShelleyBlock, ShelleyHeader, ShelleyHeaderBody,
    ShelleyOpCert, ShelleyTxBody, ShelleyTxIn, ShelleyVrfCert, ShelleyWitnessSet, SlotNo,
    StakeCredential, Tip, Tx, TxId, compute_block_body_hash,
};
use yggdrasil_mempool::{Mempool, MempoolEntry};
use yggdrasil_network::{
    BlockFetchMessage, ChainSyncMessage, HandshakeVersion, KeepAliveMessage, MiniProtocolNum,
    peer_accept,
};
use yggdrasil_node::{
    DecodedSyncStep, LedgerCheckpointPolicy, MultiEraBlock, MultiEraSyncStep, NodeConfig,
    SHELLEY_KES_DEPTH, SyncServiceConfig, SyncStep, TypedIntersectResult, TypedSyncStep,
    VerificationConfig, VerifiedSyncServiceConfig, apply_multi_era_step_to_volatile,
    apply_nonce_evolution, apply_typed_progress_to_volatile, bootstrap, collect_rolled_back_tx_ids,
    decode_multi_era_block, decode_multi_era_blocks, evict_confirmed_from_mempool, extract_tx_ids,
    keepalive_heartbeat, multi_era_block_to_block, multi_era_block_to_chain_entry,
    promote_stable_blocks, recover_ledger_state_chaindb, run_sync_service,
    run_verified_sync_service_chaindb, shelley_header_body_to_consensus,
    shelley_header_to_consensus, shelley_opcert_to_consensus, sync_batch_apply, sync_step,
    sync_step_decoded, sync_step_multi_era, sync_step_typed, sync_steps, sync_steps_typed,
    sync_until_typed, track_chain_state, track_chain_state_entries, typed_find_intersect,
    validate_block_body_size, validate_block_protocol_version, verify_block_body_hash,
    verify_multi_era_block, verify_shelley_header,
};
use yggdrasil_storage::{
    ChainDb, ImmutableStore, InMemoryImmutable, InMemoryLedgerStore, InMemoryVolatile,
    VolatileStore,
};

fn test_shelley_initial_funds_address(seed: u8) -> Vec<u8> {
    let mut address = vec![0x60];
    address.extend_from_slice(&[seed; 28]);
    address
}

fn test_store_block(hash_byte: u8, slot: u64) -> Block {
    Block {
        era: Era::Shelley,
        header: BlockHeader {
            hash: HeaderHash([hash_byte; 32]),
            prev_hash: HeaderHash([0; 32]),
            slot_no: SlotNo(slot),
            block_no: BlockNo(slot),
            issuer_vkey: [0; 32],
            protocol_version: None,
        },
        transactions: Vec::new(),
        raw_cbor: None,
        header_cbor_size: None,
    }
}

async fn spawn_rollforward_responder(magic: u32) -> SocketAddr {
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
                header: vec![0x82, 0x00, 0x01],
                tip: vec![0x81, 0x01],
            }
            .to_cbor(),
        )
        .await
        .expect("send cs rollforward");

        let bf_req = bf.recv().await.expect("bf recv");
        let bf_msg = BlockFetchMessage::from_cbor(&bf_req).expect("decode bf request");
        match bf_msg {
            BlockFetchMessage::MsgRequestRange(range) => {
                assert_eq!(range.lower, Point::Origin.to_cbor_bytes());
                assert_eq!(range.upper, vec![0x81, 0x01]);
            }
            other => panic!("unexpected blockfetch request: {other:?}"),
        }

        bf.send(BlockFetchMessage::MsgStartBatch.to_cbor())
            .await
            .expect("start batch");
        bf.send(
            BlockFetchMessage::MsgBlock {
                block: b"block-1".to_vec(),
            }
            .to_cbor(),
        )
        .await
        .expect("send block1");
        bf.send(
            BlockFetchMessage::MsgBlock {
                block: b"block-2".to_vec(),
            }
            .to_cbor(),
        )
        .await
        .expect("send block2");
        bf.send(BlockFetchMessage::MsgBatchDone.to_cbor())
            .await
            .expect("batch done");

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        conn.mux.abort();
    });

    addr
}

async fn spawn_rollback_responder(magic: u32) -> SocketAddr {
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

        let cs_req = cs.recv().await.expect("cs recv");
        let cs_msg = ChainSyncMessage::from_cbor(&cs_req).expect("decode cs request");
        assert_eq!(cs_msg, ChainSyncMessage::MsgRequestNext);

        cs.send(
            ChainSyncMessage::MsgRollBackward {
                point: vec![0x82, 0x05, 0x07],
                tip: vec![0x82, 0x05, 0x06],
            }
            .to_cbor(),
        )
        .await
        .expect("send cs rollback");

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        conn.mux.abort();
    });

    addr
}

async fn spawn_two_step_responder(magic: u32) -> SocketAddr {
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

        // Step 1: roll forward with one block.
        let cs_req_1 = cs.recv().await.expect("cs recv 1");
        let cs_msg_1 = ChainSyncMessage::from_cbor(&cs_req_1).expect("decode cs request 1");
        assert_eq!(cs_msg_1, ChainSyncMessage::MsgRequestNext);
        cs.send(
            ChainSyncMessage::MsgRollForward {
                header: vec![0x82, 0x00, 0x01],
                tip: vec![0x81, 0x01],
            }
            .to_cbor(),
        )
        .await
        .expect("send cs rollforward 1");

        let bf_req_1 = bf.recv().await.expect("bf recv 1");
        let bf_msg_1 = BlockFetchMessage::from_cbor(&bf_req_1).expect("decode bf request 1");
        match bf_msg_1 {
            BlockFetchMessage::MsgRequestRange(range) => {
                assert_eq!(range.lower, Point::Origin.to_cbor_bytes());
                assert_eq!(range.upper, vec![0x81, 0x01]);
            }
            other => panic!("unexpected blockfetch request step 1: {other:?}"),
        }

        bf.send(BlockFetchMessage::MsgStartBatch.to_cbor())
            .await
            .expect("start batch 1");
        bf.send(
            BlockFetchMessage::MsgBlock {
                block: b"block-1".to_vec(),
            }
            .to_cbor(),
        )
        .await
        .expect("send block 1");
        bf.send(BlockFetchMessage::MsgBatchDone.to_cbor())
            .await
            .expect("batch done 1");

        // Step 2: rollback.
        let cs_req_2 = cs.recv().await.expect("cs recv 2");
        let cs_msg_2 = ChainSyncMessage::from_cbor(&cs_req_2).expect("decode cs request 2");
        assert_eq!(cs_msg_2, ChainSyncMessage::MsgRequestNext);
        cs.send(
            ChainSyncMessage::MsgRollBackward {
                point: vec![0x81, 0x00],
                tip: vec![0x81, 0x02],
            }
            .to_cbor(),
        )
        .await
        .expect("send cs rollback 2");

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        conn.mux.abort();
    });

    addr
}

async fn spawn_verified_batch_responder(
    magic: u32,
    tip: Point,
    block_bytes: Vec<u8>,
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

        let tip_cbor = match tip {
            Point::Origin => Tip::TipGenesis.to_cbor_bytes(),
            Point::BlockPoint(s, h) => {
                Tip::Tip(Point::BlockPoint(s, h), BlockNo(0)).to_cbor_bytes()
            }
        };
        cs.send(
            ChainSyncMessage::MsgRollForward {
                header: vec![0x82, 0x00, 0x01],
                tip: tip_cbor,
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
        bf.send(BlockFetchMessage::MsgBlock { block: block_bytes }.to_cbor())
            .await
            .expect("send block");
        bf.send(BlockFetchMessage::MsgBatchDone.to_cbor())
            .await
            .expect("batch done");

        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
        conn.mux.abort();
    });

    addr
}

#[tokio::test]
async fn sync_step_rollforward_fetches_blocks() {
    let magic = 42;
    let addr = spawn_rollforward_responder(magic).await;

    let config = NodeConfig {
        peer_addr: addr,
        network_magic: magic,
        protocol_versions: vec![HandshakeVersion(15)],
        peer_sharing: 1,
    };

    let mut session = bootstrap(&config).await.expect("bootstrap");

    let step = sync_step(
        &mut session.chain_sync,
        session.block_fetch.as_mut().expect("block_fetch migrated"),
        Point::Origin.to_cbor_bytes(),
    )
    .await
    .expect("sync step");

    assert_eq!(
        step,
        SyncStep::RollForward {
            header: vec![0x82, 0x00, 0x01],
            tip: vec![0x81, 0x01],
            blocks: vec![b"block-1".to_vec(), b"block-2".to_vec()],
        }
    );

    session.mux.abort();
}

#[tokio::test]
async fn run_verified_sync_service_chaindb_persists_checkpoint() {
    let magic = 81;
    let block_body = build_byron_ebb_body(0, 1, &[0; 32]);
    let block_bytes = build_multi_era_envelope(0, &block_body);
    let tip = Point::BlockPoint(
        SlotNo(0),
        ByronBlock::decode_ebb(&block_bytes[2..])
            .expect("decode ebb")
            .header_hash(),
    );
    let addr = spawn_verified_batch_responder(magic, tip, block_bytes).await;

    let config = NodeConfig {
        peer_addr: addr,
        network_magic: magic,
        protocol_versions: vec![HandshakeVersion(15)],
        peer_sharing: 1,
    };
    let service_config = VerifiedSyncServiceConfig {
        batch_size: 1,
        verification: VerificationConfig {
            slots_per_kes_period: 129_600,
            max_kes_evolutions: 62,
            verify_body_hash: true,
            max_major_protocol_version: None,
            future_check: None,
            ocert_counters: None,
            pp_major_protocol_version: None,
            network_magic: Some(magic),
        },
        nonce_config: None,
        security_param: Some(SecurityParam(1)),
        checkpoint_policy: LedgerCheckpointPolicy::default(),
        plutus_cost_model: None,
        verify_vrf: false,
        active_slot_coeff: None,
        slot_length_secs: None,
        system_start_unix_secs: None,
        epoch_schedule: None,
        block_fetch_pool: None,
        max_concurrent_block_fetch_peers: 1,
        density_registry: None,
        shared_fetch_worker_pool: None,
        shared_chainsync_worker_pool: None,
    };
    let mut session = bootstrap(&config).await.expect("bootstrap");
    let mut chain_db = ChainDb::new(
        InMemoryImmutable::default(),
        InMemoryVolatile::default(),
        InMemoryLedgerStore::default(),
    );

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let _ = shutdown_tx.send(());
    });

    let outcome = run_verified_sync_service_chaindb(
        &mut session.chain_sync,
        session.block_fetch.as_mut().expect("block_fetch migrated"),
        &mut chain_db,
        LedgerState::new(Era::Byron),
        Point::Origin,
        &service_config,
        None,
        async {
            let _ = shutdown_rx.await;
        },
    )
    .await
    .expect("verified sync service via chaindb");

    assert_eq!(outcome.total_blocks, 1);
    assert_eq!(outcome.final_point, tip);

    let (checkpoint_slot, checkpoint) = chain_db
        .latest_ledger_checkpoint()
        .expect("decode checkpoint")
        .expect("checkpoint persisted after verified sync");
    assert_eq!(checkpoint_slot, SlotNo(0));
    assert_eq!(checkpoint.restore().tip, tip);

    session.mux.abort();
}

#[tokio::test]
async fn sync_step_rollback_skips_blockfetch() {
    let magic = 4242;
    let addr = spawn_rollback_responder(magic).await;

    let config = NodeConfig {
        peer_addr: addr,
        network_magic: magic,
        protocol_versions: vec![HandshakeVersion(15)],
        peer_sharing: 1,
    };

    let mut session = bootstrap(&config).await.expect("bootstrap");

    let step = sync_step(
        &mut session.chain_sync,
        session.block_fetch.as_mut().expect("block_fetch migrated"),
        Point::Origin.to_cbor_bytes(),
    )
    .await
    .expect("sync step");

    assert_eq!(
        step,
        SyncStep::RollBackward {
            point: vec![0x82, 0x05, 0x07],
            tip: vec![0x82, 0x05, 0x06],
        }
    );

    session.mux.abort();
}

#[tokio::test]
async fn sync_steps_tracks_progress_and_point() {
    let magic = 77;
    let addr = spawn_two_step_responder(magic).await;

    let config = NodeConfig {
        peer_addr: addr,
        network_magic: magic,
        protocol_versions: vec![HandshakeVersion(15)],
        peer_sharing: 1,
    };

    let mut session = bootstrap(&config).await.expect("bootstrap");

    let progress = sync_steps(
        &mut session.chain_sync,
        session.block_fetch.as_mut().expect("block_fetch migrated"),
        Point::Origin.to_cbor_bytes(),
        2,
    )
    .await
    .expect("sync steps");

    assert_eq!(progress.fetched_blocks, 1);
    assert_eq!(progress.current_point, vec![0x81, 0x00]);
    assert_eq!(progress.steps.len(), 2);
    assert_eq!(
        progress.steps[0],
        SyncStep::RollForward {
            header: vec![0x82, 0x00, 0x01],
            tip: vec![0x81, 0x01],
            blocks: vec![b"block-1".to_vec()],
        }
    );
    assert_eq!(
        progress.steps[1],
        SyncStep::RollBackward {
            point: vec![0x81, 0x00],
            tip: vec![0x81, 0x02],
        }
    );

    session.mux.abort();
}

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

fn sample_header_body() -> ShelleyHeaderBody {
    ShelleyHeaderBody {
        block_number: 1,
        slot: 500,
        prev_hash: Some([0xAA; 32]),
        issuer_vkey: [0x11; 32],
        vrf_vkey: [0x22; 32],
        nonce_vrf: sample_vrf_cert(0x30),
        leader_vrf: sample_vrf_cert(0x40),
        block_body_size: 1024,
        block_body_hash: [0x55; 32],
        operational_cert: sample_opcert(0x60),
        protocol_version: (2, 0),
    }
}

/// Build a sample block and return its CBOR encoding.
fn sample_block_bytes() -> Vec<u8> {
    let block = ShelleyBlock {
        header: ShelleyHeader {
            body: sample_header_body(),
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        transaction_metadata_set: std::collections::HashMap::new(),
    };
    block.to_cbor_bytes()
}

/// Compute the expected Blake2b-256 header hash for the sample block.
fn sample_header_hash() -> HeaderHash {
    let header = ShelleyHeader {
        body: sample_header_body(),
        signature: vec![0xDD; 448],
    };
    header.header_hash()
}

fn sample_praos_header_body() -> PraosHeaderBody {
    PraosHeaderBody {
        block_number: 1,
        slot: 500,
        prev_hash: Some([0xAA; 32]),
        issuer_vkey: [0x11; 32],
        vrf_vkey: [0x22; 32],
        vrf_result: sample_vrf_cert(0x30),
        block_body_size: 1024,
        block_body_hash: [0x55; 32],
        operational_cert: sample_opcert(0x60),
        protocol_version: (2, 0),
    }
}

fn sample_praos_header_hash() -> HeaderHash {
    let header = PraosHeader {
        body: sample_praos_header_body(),
        signature: vec![0xDD; 448],
    };
    header.header_hash()
}

async fn spawn_decoded_rollforward_responder(magic: u32, block_bytes: Vec<u8>) -> SocketAddr {
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
                header: vec![0x82, 0x00, 0x04],
                tip: vec![0x82, 0x06, 0x06],
            }
            .to_cbor(),
        )
        .await
        .expect("send cs rollforward");

        let bf_req = bf.recv().await.expect("bf recv");
        let bf_msg = BlockFetchMessage::from_cbor(&bf_req).expect("decode bf request");
        match bf_msg {
            BlockFetchMessage::MsgRequestRange(range) => {
                // Synthetic tip bytes (`[0x82,0x06,0x06]`) are not a valid
                // Point CBOR, so `normalize_blockfetch_range_bytes` falls back
                // to a raw pass-through and `lower` stays as Origin.
                assert_eq!(range.lower, Point::Origin.to_cbor_bytes());
                assert_eq!(range.upper, vec![0x82, 0x06, 0x06]);
            }
            other => panic!("unexpected blockfetch request: {other:?}"),
        }

        bf.send(BlockFetchMessage::MsgStartBatch.to_cbor())
            .await
            .expect("start batch");
        bf.send(BlockFetchMessage::MsgBlock { block: block_bytes }.to_cbor())
            .await
            .expect("send block bytes");
        bf.send(BlockFetchMessage::MsgBatchDone.to_cbor())
            .await
            .expect("batch done");

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        conn.mux.abort();
    });

    addr
}

async fn spawn_typed_rollforward_responder(
    magic: u32,
    header_bytes: Vec<u8>,
    tip_bytes: Vec<u8>,
    block_bytes: Vec<u8>,
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
                header: header_bytes,
                tip: tip_bytes,
            }
            .to_cbor(),
        )
        .await
        .expect("send cs rollforward");

        let bf_req = bf.recv().await.expect("bf recv");
        let bf_msg = BlockFetchMessage::from_cbor(&bf_req).expect("decode bf request");
        match bf_msg {
            BlockFetchMessage::MsgRequestRange(range) => {
                // Fresh-from-Origin sync collapses to single-block fetch.
                assert_eq!(range.lower, range.upper);
            }
            other => panic!("unexpected blockfetch request: {other:?}"),
        }

        bf.send(BlockFetchMessage::MsgStartBatch.to_cbor())
            .await
            .expect("start batch");
        bf.send(BlockFetchMessage::MsgBlock { block: block_bytes }.to_cbor())
            .await
            .expect("send block bytes");
        bf.send(BlockFetchMessage::MsgBatchDone.to_cbor())
            .await
            .expect("batch done");

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        conn.mux.abort();
    });

    addr
}

async fn spawn_typed_rollback_responder(magic: u32, point: Vec<u8>, tip: Vec<u8>) -> SocketAddr {
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

        let cs_req = cs.recv().await.expect("cs recv");
        let cs_msg = ChainSyncMessage::from_cbor(&cs_req).expect("decode cs request");
        assert_eq!(cs_msg, ChainSyncMessage::MsgRequestNext);

        cs.send(ChainSyncMessage::MsgRollBackward { point, tip }.to_cbor())
            .await
            .expect("send rollback");

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        conn.mux.abort();
    });

    addr
}

async fn spawn_two_step_typed_responder(
    magic: u32,
    first_header: Vec<u8>,
    first_tip: Vec<u8>,
    first_block: Vec<u8>,
    rollback_point: Vec<u8>,
    rollback_tip: Vec<u8>,
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

        // Step 1: roll forward.
        let cs_req_1 = cs.recv().await.expect("cs recv 1");
        let cs_msg_1 = ChainSyncMessage::from_cbor(&cs_req_1).expect("decode cs request 1");
        assert_eq!(cs_msg_1, ChainSyncMessage::MsgRequestNext);

        cs.send(
            ChainSyncMessage::MsgRollForward {
                header: first_header,
                tip: first_tip,
            }
            .to_cbor(),
        )
        .await
        .expect("send rollforward");

        let bf_req_1 = bf.recv().await.expect("bf recv 1");
        let bf_msg_1 = BlockFetchMessage::from_cbor(&bf_req_1).expect("decode bf request 1");
        match bf_msg_1 {
            BlockFetchMessage::MsgRequestRange(range) => {
                // Fresh-from-Origin sync collapses to single-block fetch.
                assert_eq!(range.lower, range.upper);
            }
            other => panic!("unexpected blockfetch request step 1: {other:?}"),
        }

        bf.send(BlockFetchMessage::MsgStartBatch.to_cbor())
            .await
            .expect("start batch 1");
        bf.send(BlockFetchMessage::MsgBlock { block: first_block }.to_cbor())
            .await
            .expect("send first block");
        bf.send(BlockFetchMessage::MsgBatchDone.to_cbor())
            .await
            .expect("batch done 1");

        // Step 2: rollback.
        let cs_req_2 = cs.recv().await.expect("cs recv 2");
        let cs_msg_2 = ChainSyncMessage::from_cbor(&cs_req_2).expect("decode cs request 2");
        assert_eq!(cs_msg_2, ChainSyncMessage::MsgRequestNext);

        cs.send(
            ChainSyncMessage::MsgRollBackward {
                point: rollback_point,
                tip: rollback_tip,
            }
            .to_cbor(),
        )
        .await
        .expect("send rollback");

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        conn.mux.abort();
    });

    addr
}

#[tokio::test]
async fn sync_step_decoded_rollforward_decodes_shelley_blocks() {
    let magic = 99;
    let addr = spawn_decoded_rollforward_responder(magic, sample_block_bytes()).await;

    let config = NodeConfig {
        peer_addr: addr,
        network_magic: magic,
        protocol_versions: vec![HandshakeVersion(15)],
        peer_sharing: 1,
    };

    let mut session = bootstrap(&config).await.expect("bootstrap");

    let step = sync_step_decoded(
        &mut session.chain_sync,
        session.block_fetch.as_mut().expect("block_fetch migrated"),
        Point::Origin.to_cbor_bytes(),
    )
    .await
    .expect("decoded sync step");

    match step {
        DecodedSyncStep::RollForward {
            header,
            tip,
            blocks,
        } => {
            assert_eq!(header, vec![0x82, 0x00, 0x04]);
            assert_eq!(tip, vec![0x82, 0x06, 0x06]);
            assert_eq!(blocks.len(), 1);
            assert_eq!(blocks[0].header.body.block_number, 1);
            assert_eq!(blocks[0].header.body.slot, 500);
        }
        other => panic!("unexpected decoded step: {other:?}"),
    }

    session.mux.abort();
}

#[tokio::test]
async fn sync_step_decoded_reports_decode_error_on_invalid_block_bytes() {
    let magic = 100;
    let addr = spawn_decoded_rollforward_responder(magic, b"not-a-cbor-block".to_vec()).await;

    let config = NodeConfig {
        peer_addr: addr,
        network_magic: magic,
        protocol_versions: vec![HandshakeVersion(15)],
        peer_sharing: 1,
    };

    let mut session = bootstrap(&config).await.expect("bootstrap");

    let err = sync_step_decoded(
        &mut session.chain_sync,
        session.block_fetch.as_mut().expect("block_fetch migrated"),
        Point::Origin.to_cbor_bytes(),
    )
    .await
    .expect_err("expected decode error");

    assert!(
        matches!(err, yggdrasil_node::SyncError::LedgerDecode(_)),
        "expected LedgerDecode error, got {err:?}"
    );

    session.mux.abort();
}

#[tokio::test]
async fn sync_step_typed_rollforward_decodes_header_tip_and_blocks() {
    let magic = 101;

    let header = ShelleyHeader {
        body: sample_header_body(),
        signature: vec![0xAB; 448],
    };
    let tip = Point::BlockPoint(SlotNo(500), HeaderHash([0xCC; 32]));

    let addr = spawn_typed_rollforward_responder(
        magic,
        header.to_cbor_bytes(),
        Tip::Tip(tip, BlockNo(0)).to_cbor_bytes(),
        sample_block_bytes(),
    )
    .await;

    let config = NodeConfig {
        peer_addr: addr,
        network_magic: magic,
        protocol_versions: vec![HandshakeVersion(15)],
        peer_sharing: 1,
    };

    let mut session = bootstrap(&config).await.expect("bootstrap");

    let step = sync_step_typed(
        &mut session.chain_sync,
        session.block_fetch.as_mut().expect("block_fetch migrated"),
        Point::Origin,
    )
    .await
    .expect("typed sync step");

    match step {
        TypedSyncStep::RollForward {
            header: decoded_header,
            tip: decoded_tip,
            blocks,
            raw_blocks: _,
        } => {
            assert_eq!(*decoded_header, header);
            assert_eq!(decoded_tip, tip);
            assert_eq!(blocks.len(), 1);
            assert_eq!(blocks[0].header.body.block_number, 1);
        }
        other => panic!("unexpected typed step: {other:?}"),
    }

    session.mux.abort();
}

#[tokio::test]
async fn sync_step_typed_rollback_decodes_points() {
    let magic = 102;
    let point = Point::BlockPoint(SlotNo(111), HeaderHash([0x11; 32]));
    let tip = Point::BlockPoint(SlotNo(222), HeaderHash([0x22; 32]));

    let addr = spawn_typed_rollback_responder(
        magic,
        point.to_cbor_bytes(),
        Tip::Tip(tip, BlockNo(0)).to_cbor_bytes(),
    )
    .await;

    let config = NodeConfig {
        peer_addr: addr,
        network_magic: magic,
        protocol_versions: vec![HandshakeVersion(15)],
        peer_sharing: 1,
    };

    let mut session = bootstrap(&config).await.expect("bootstrap");

    let step = sync_step_typed(
        &mut session.chain_sync,
        session.block_fetch.as_mut().expect("block_fetch migrated"),
        Point::Origin,
    )
    .await
    .expect("typed rollback step");

    assert_eq!(step, TypedSyncStep::RollBackward { point, tip });

    session.mux.abort();
}

#[tokio::test]
async fn sync_steps_typed_tracks_progress_and_rollbacks() {
    let magic = 103;
    let first_header = ShelleyHeader {
        body: sample_header_body(),
        signature: vec![0xEF; 448],
    };
    let first_tip = Point::BlockPoint(SlotNo(10), HeaderHash([0x10; 32]));
    let rollback_point = Point::BlockPoint(SlotNo(9), HeaderHash([0x09; 32]));
    let rollback_tip = Point::BlockPoint(SlotNo(11), HeaderHash([0x11; 32]));

    let addr = spawn_two_step_typed_responder(
        magic,
        first_header.to_cbor_bytes(),
        Tip::Tip(first_tip, BlockNo(0)).to_cbor_bytes(),
        sample_block_bytes(),
        rollback_point.to_cbor_bytes(),
        Tip::Tip(rollback_tip, BlockNo(0)).to_cbor_bytes(),
    )
    .await;

    let config = NodeConfig {
        peer_addr: addr,
        network_magic: magic,
        protocol_versions: vec![HandshakeVersion(15)],
        peer_sharing: 1,
    };

    let mut session = bootstrap(&config).await.expect("bootstrap");

    let progress = sync_steps_typed(
        &mut session.chain_sync,
        session.block_fetch.as_mut().expect("block_fetch migrated"),
        Point::Origin,
        2,
    )
    .await
    .expect("typed progress");

    assert_eq!(progress.fetched_blocks, 1);
    assert_eq!(progress.rollback_count, 1);
    assert_eq!(progress.current_point, rollback_point);
    assert_eq!(progress.steps.len(), 2);
    assert!(matches!(
        progress.steps[0],
        TypedSyncStep::RollForward { .. }
    ));
    assert!(matches!(
        progress.steps[1],
        TypedSyncStep::RollBackward { .. }
    ));

    session.mux.abort();
}

#[tokio::test]
async fn sync_until_typed_stops_at_target_point() {
    let magic = 104;
    let first_header = ShelleyHeader {
        body: sample_header_body(),
        signature: vec![0xE1; 448],
    };
    let first_tip = Point::BlockPoint(SlotNo(20), HeaderHash([0x20; 32]));
    let rollback_point = Point::BlockPoint(SlotNo(19), HeaderHash([0x19; 32]));
    let rollback_tip = Point::BlockPoint(SlotNo(21), HeaderHash([0x21; 32]));

    let addr = spawn_two_step_typed_responder(
        magic,
        first_header.to_cbor_bytes(),
        Tip::Tip(first_tip, BlockNo(0)).to_cbor_bytes(),
        sample_block_bytes(),
        rollback_point.to_cbor_bytes(),
        Tip::Tip(rollback_tip, BlockNo(0)).to_cbor_bytes(),
    )
    .await;

    let config = NodeConfig {
        peer_addr: addr,
        network_magic: magic,
        protocol_versions: vec![HandshakeVersion(15)],
        peer_sharing: 1,
    };

    let mut session = bootstrap(&config).await.expect("bootstrap");

    let progress = sync_until_typed(
        &mut session.chain_sync,
        session.block_fetch.as_mut().expect("block_fetch migrated"),
        Point::Origin,
        10,
        Some(first_tip),
    )
    .await
    .expect("sync until target");

    assert_eq!(progress.current_point, first_tip);
    assert_eq!(progress.steps.len(), 1);
    assert_eq!(progress.fetched_blocks, 1);
    assert_eq!(progress.rollback_count, 0);

    session.mux.abort();
}

#[tokio::test]
async fn apply_typed_progress_to_volatile_applies_forwards_and_rollbacks() {
    let magic = 105;
    let first_header = ShelleyHeader {
        body: sample_header_body(),
        signature: vec![0xE2; 448],
    };
    let first_tip = Point::BlockPoint(SlotNo(30), HeaderHash([0x30; 32]));
    let rollback_point = Point::BlockPoint(SlotNo(29), HeaderHash([0x29; 32]));
    let rollback_tip = Point::BlockPoint(SlotNo(31), HeaderHash([0x31; 32]));

    let addr = spawn_two_step_typed_responder(
        magic,
        first_header.to_cbor_bytes(),
        Tip::Tip(first_tip, BlockNo(0)).to_cbor_bytes(),
        sample_block_bytes(),
        rollback_point.to_cbor_bytes(),
        Tip::Tip(rollback_tip, BlockNo(0)).to_cbor_bytes(),
    )
    .await;

    let config = NodeConfig {
        peer_addr: addr,
        network_magic: magic,
        protocol_versions: vec![HandshakeVersion(15)],
        peer_sharing: 1,
    };

    let mut session = bootstrap(&config).await.expect("bootstrap");
    let progress = sync_steps_typed(
        &mut session.chain_sync,
        session.block_fetch.as_mut().expect("block_fetch migrated"),
        Point::Origin,
        2,
    )
    .await
    .expect("typed progress");

    let mut store = InMemoryVolatile::default();
    apply_typed_progress_to_volatile(&mut store, &progress).expect("apply to volatile");

    // The rollback point intentionally does not match fetched block hash in this
    // test fixture, so volatile tip remains at the last appended converted
    // Shelley block point.
    assert_eq!(
        store.tip(),
        Point::BlockPoint(SlotNo(500), sample_header_hash())
    );

    session.mux.abort();
}

// ---------------------------------------------------------------------------
// Phase 32: Intersection + batch apply + keepalive heartbeat
// ---------------------------------------------------------------------------

async fn spawn_intersect_found_responder(
    magic: u32,
    intersect_point: Vec<u8>,
    tip: Vec<u8>,
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

        let req = cs.recv().await.expect("cs recv");
        let msg = ChainSyncMessage::from_cbor(&req).expect("decode cs msg");
        match msg {
            ChainSyncMessage::MsgFindIntersect { .. } => {}
            other => panic!("expected MsgFindIntersect, got {other:?}"),
        }

        cs.send(
            ChainSyncMessage::MsgIntersectFound {
                point: intersect_point,
                tip,
            }
            .to_cbor(),
        )
        .await
        .expect("send intersect found");

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        conn.mux.abort();
    });

    addr
}

async fn spawn_intersect_not_found_responder(magic: u32, tip: Vec<u8>) -> SocketAddr {
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

        let req = cs.recv().await.expect("cs recv");
        let msg = ChainSyncMessage::from_cbor(&req).expect("decode cs msg");
        match msg {
            ChainSyncMessage::MsgFindIntersect { .. } => {}
            other => panic!("expected MsgFindIntersect, got {other:?}"),
        }

        cs.send(ChainSyncMessage::MsgIntersectNotFound { tip }.to_cbor())
            .await
            .expect("send intersect not found");

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        conn.mux.abort();
    });

    addr
}

async fn spawn_keepalive_responder(magic: u32, rounds: usize) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local addr");

    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut conn = peer_accept(stream, magic, &[HandshakeVersion(15)])
            .await
            .expect("accept handshake");

        let mut ka = conn
            .protocols
            .remove(&MiniProtocolNum::KEEP_ALIVE)
            .expect("keepalive handle");

        for _ in 0..rounds {
            let req = ka.recv().await.expect("ka recv");
            let msg = KeepAliveMessage::from_cbor(&req).expect("decode ka msg");
            match msg {
                KeepAliveMessage::MsgKeepAlive { cookie } => {
                    ka.send(KeepAliveMessage::MsgKeepAliveResponse { cookie }.to_cbor())
                        .await
                        .expect("send ka response");
                }
                other => panic!("expected MsgKeepAlive, got {other:?}"),
            }
        }

        // After serving `rounds` heartbeats, drop the connection.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        conn.mux.abort();
    });

    addr
}

#[tokio::test]
async fn typed_find_intersect_found() {
    let magic = 200;
    let intersect = Point::BlockPoint(SlotNo(42), HeaderHash([0x42; 32]));
    let tip = Point::BlockPoint(SlotNo(100), HeaderHash([0xAA; 32]));

    let addr = spawn_intersect_found_responder(
        magic,
        intersect.to_cbor_bytes(),
        Tip::Tip(tip, BlockNo(0)).to_cbor_bytes(),
    )
    .await;

    let config = NodeConfig {
        peer_addr: addr,
        network_magic: magic,
        protocol_versions: vec![HandshakeVersion(15)],
        peer_sharing: 1,
    };

    let mut session = bootstrap(&config).await.expect("bootstrap");

    let result = typed_find_intersect(&mut session.chain_sync, &[intersect, Point::Origin])
        .await
        .expect("find intersect");

    assert_eq!(
        result,
        TypedIntersectResult::Found {
            point: intersect,
            tip,
        }
    );

    session.mux.abort();
}

#[tokio::test]
async fn typed_find_intersect_not_found() {
    let magic = 201;
    let tip = Point::BlockPoint(SlotNo(999), HeaderHash([0xFF; 32]));

    let addr =
        spawn_intersect_not_found_responder(magic, Tip::Tip(tip, BlockNo(0)).to_cbor_bytes()).await;

    let config = NodeConfig {
        peer_addr: addr,
        network_magic: magic,
        protocol_versions: vec![HandshakeVersion(15)],
        peer_sharing: 1,
    };

    let mut session = bootstrap(&config).await.expect("bootstrap");

    let result = typed_find_intersect(
        &mut session.chain_sync,
        &[Point::BlockPoint(SlotNo(1), HeaderHash([0x01; 32]))],
    )
    .await
    .expect("find intersect");

    assert_eq!(result, TypedIntersectResult::NotFound { tip });

    session.mux.abort();
}

#[tokio::test]
async fn sync_batch_apply_updates_volatile_store() {
    let magic = 202;
    let first_header = ShelleyHeader {
        body: sample_header_body(),
        signature: vec![0xF1; 448],
    };
    let first_tip = Point::BlockPoint(SlotNo(40), HeaderHash([0x40; 32]));
    let rollback_point = Point::BlockPoint(SlotNo(39), HeaderHash([0x39; 32]));
    let rollback_tip = Point::BlockPoint(SlotNo(41), HeaderHash([0x41; 32]));

    let addr = spawn_two_step_typed_responder(
        magic,
        first_header.to_cbor_bytes(),
        Tip::Tip(first_tip, BlockNo(0)).to_cbor_bytes(),
        sample_block_bytes(),
        rollback_point.to_cbor_bytes(),
        Tip::Tip(rollback_tip, BlockNo(0)).to_cbor_bytes(),
    )
    .await;

    let config = NodeConfig {
        peer_addr: addr,
        network_magic: magic,
        protocol_versions: vec![HandshakeVersion(15)],
        peer_sharing: 1,
    };

    let mut session = bootstrap(&config).await.expect("bootstrap");
    let mut store = InMemoryVolatile::default();

    let progress = sync_batch_apply(
        &mut session.chain_sync,
        session.block_fetch.as_mut().expect("block_fetch migrated"),
        &mut store,
        Point::Origin,
        2,
        None,
    )
    .await
    .expect("sync batch apply");

    assert_eq!(progress.steps.len(), 2);
    assert_eq!(progress.fetched_blocks, 1);
    assert_eq!(progress.rollback_count, 1);
    // Store tip should reflect the block that was appended.
    assert_eq!(
        store.tip(),
        Point::BlockPoint(SlotNo(500), sample_header_hash())
    );

    session.mux.abort();
}

#[tokio::test]
async fn keepalive_heartbeat_terminates_on_connection_close() {
    let magic = 203;
    let rounds = 3;
    let addr = spawn_keepalive_responder(magic, rounds).await;

    let config = NodeConfig {
        peer_addr: addr,
        network_magic: magic,
        protocol_versions: vec![HandshakeVersion(15)],
        peer_sharing: 1,
    };

    let mut session = bootstrap(&config).await.expect("bootstrap");

    // Run heartbeat with short interval; the responder serves 3 rounds then
    // drops the connection, which should cause keepalive_heartbeat to return.
    let err = keepalive_heartbeat(
        &mut session.keep_alive,
        std::time::Duration::from_millis(10),
    )
    .await;

    // The error should be a KeepAlive variant since the connection was closed.
    assert!(
        matches!(err, yggdrasil_node::SyncError::KeepAlive(_)),
        "expected KeepAlive error, got {err:?}"
    );

    session.mux.abort();
}

// ---------------------------------------------------------------------------
// Phase 33: Managed sync service tests
// ---------------------------------------------------------------------------

async fn spawn_service_responder(
    magic: u32,
    header_bytes: Vec<u8>,
    tip_bytes: Vec<u8>,
    block_bytes: Vec<u8>,
    steps: usize,
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

        for _ in 0..steps {
            let cs_req = cs.recv().await.expect("cs recv");
            let cs_msg = ChainSyncMessage::from_cbor(&cs_req).expect("decode cs request");
            assert_eq!(cs_msg, ChainSyncMessage::MsgRequestNext);

            cs.send(
                ChainSyncMessage::MsgRollForward {
                    header: header_bytes.clone(),
                    tip: tip_bytes.clone(),
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
                    block: block_bytes.clone(),
                }
                .to_cbor(),
            )
            .await
            .expect("send block");
            bf.send(BlockFetchMessage::MsgBatchDone.to_cbor())
                .await
                .expect("batch done");
        }

        // After serving the requested steps, keep the connection alive
        // long enough for the service loop's shutdown to fire while the
        // next batch is blocked waiting for more protocol messages.
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        conn.mux.abort();
    });

    addr
}

#[tokio::test]
async fn run_sync_service_shutdown_after_batches() {
    let magic = 300;
    let header = ShelleyHeader {
        body: sample_header_body(),
        signature: vec![0xAB; 448],
    };
    let tip = Point::BlockPoint(SlotNo(500), sample_header_hash());

    // Serve 1 step (1 batch of size 1).
    let addr = spawn_service_responder(
        magic,
        header.to_cbor_bytes(),
        Tip::Tip(tip, BlockNo(0)).to_cbor_bytes(),
        sample_block_bytes(),
        1,
    )
    .await;

    let config = NodeConfig {
        peer_addr: addr,
        network_magic: magic,
        protocol_versions: vec![HandshakeVersion(15)],
        peer_sharing: 1,
    };

    let mut session = bootstrap(&config).await.expect("bootstrap");
    let mut store = InMemoryVolatile::default();

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let svc_config = SyncServiceConfig {
        batch_size: 1,
        keepalive_interval: None,
    };

    // Signal shutdown after a brief pause to allow at least partial work.
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let _ = shutdown_tx.send(());
    });

    let outcome = run_sync_service(
        &mut session.chain_sync,
        session.block_fetch.as_mut().expect("block_fetch migrated"),
        &mut store,
        Point::Origin,
        &svc_config,
        async {
            shutdown_rx.await.ok();
        },
    )
    .await
    .expect("sync service");

    // The service should have completed at least one batch before shutdown.
    assert!(
        outcome.batches_completed >= 1,
        "expected at least 1 batch, got {}",
        outcome.batches_completed
    );
    assert!(outcome.total_blocks >= 1);

    session.mux.abort();
}

// ---------------------------------------------------------------------------
// Phase 34: Consensus header verification bridge tests
// ---------------------------------------------------------------------------

#[test]
fn shelley_opcert_to_consensus_preserves_fields() {
    let opcert = ShelleyOpCert {
        hot_vkey: [0x11; 32],
        sequence_number: 99,
        kes_period: 200,
        sigma: [0x22; 64],
    };

    let consensus = shelley_opcert_to_consensus(&opcert);
    assert_eq!(consensus.sequence_number, 99);
    assert_eq!(consensus.kes_period, 200);
}

#[test]
fn shelley_header_body_to_consensus_preserves_fields() {
    let body = sample_header_body();
    let consensus = shelley_header_body_to_consensus(&body);
    assert_eq!(consensus.block_number.0, 1);
    assert_eq!(consensus.slot.0, 500);
    assert_eq!(consensus.block_body_size, 1024);
    assert_eq!(consensus.block_body_hash, [0x55; 32]);
    assert_eq!(consensus.protocol_version, (2, 0));
    assert_eq!(consensus.operational_cert.sequence_number, 42);
}

#[test]
fn shelley_header_to_consensus_builds_header() {
    let header = ShelleyHeader {
        body: sample_header_body(),
        signature: vec![0xCC; 448],
    };
    let result = shelley_header_to_consensus(&header);
    assert!(result.is_ok(), "expected Ok, got {result:?}");
    let consensus_hdr = result.expect("conversion should succeed");
    assert_eq!(consensus_hdr.body.slot.0, 500);
}

#[test]
fn shelley_header_to_consensus_rejects_bad_kes_length() {
    let header = ShelleyHeader {
        body: sample_header_body(),
        signature: vec![0xCC; 100], // wrong length
    };
    let result = shelley_header_to_consensus(&header);
    assert!(result.is_err());
}

#[test]
fn verify_shelley_header_returns_error_for_dummy_data() {
    // Dummy header won't pass real crypto verification.
    let header = ShelleyHeader {
        body: sample_header_body(),
        signature: vec![0xDD; 448],
    };
    let result = verify_shelley_header(&header, 129600, 62);
    assert!(result.is_err());
}

#[test]
fn shelley_kes_depth_matches_expected() {
    assert_eq!(SHELLEY_KES_DEPTH, 6);
}

// ---------------------------------------------------------------------------
// Phase 35: Multi-era block decode tests
// ---------------------------------------------------------------------------

/// Build a multi-era envelope: `[era_tag, block_body_cbor]`.
fn build_multi_era_envelope(tag: u64, block_body: &[u8]) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(2).unsigned(tag);
    let mut out = enc.into_bytes();
    out.extend_from_slice(block_body);
    out
}

/// Build a minimal valid Byron EBB body CBOR:
/// `[header, body, extra]` where header is
/// `[protocol_magic, prev_hash, body_proof, [epoch, [difficulty]], extra_data]`.
fn build_byron_ebb_body(epoch: u64, difficulty: u64, prev_hash: &[u8; 32]) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(3); // outer: [header, body, extra]
    enc.array(5); // header
    enc.unsigned(764824073); // protocol_magic (mainnet)
    enc.bytes(prev_hash);
    enc.bytes(&[0u8; 32]); // body_proof (dummy)
    enc.array(2).unsigned(epoch); // consensus_data: [epoch, [difficulty]]
    enc.array(1).unsigned(difficulty);
    enc.unsigned(0); // extra_data (dummy)
    enc.bytes(&[]); // body (dummy)
    enc.bytes(&[]); // extra (dummy)
    enc.into_bytes()
}

/// Build a minimal valid Byron main block body CBOR:
/// `[header, body, extra]` where header is
/// `[protocol_magic, prev_hash, body_proof, [slot_id, pubkey, [difficulty], sig], extra_data]`.
fn build_byron_main_body(
    epoch: u64,
    slot_in_epoch: u64,
    difficulty: u64,
    prev_hash: &[u8; 32],
) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(3); // outer: [header, body, extra]
    enc.array(5); // header
    enc.unsigned(764824073); // protocol_magic
    enc.bytes(prev_hash);
    enc.bytes(&[0u8; 32]); // body_proof (dummy)
    enc.array(4); // consensus_data: [slot_id, pubkey, [difficulty], sig]
    enc.array(2).unsigned(epoch).unsigned(slot_in_epoch); // slot_id
    enc.bytes(&[0u8; 64]); // pubkey (dummy)
    enc.array(1).unsigned(difficulty); // [difficulty]
    enc.bytes(&[0u8; 64]); // signature (dummy)
    enc.unsigned(0); // extra_data — 5th element of header array
    // body: [tx_payload, ssc_payload, dlg_payload, upd_payload]
    // decode_main reads body as array(4), then reads tx_payload as array(N).
    enc.array(4); // body array
    enc.array(0); // tx_payload: empty list of TxAux
    enc.unsigned(0); // ssc_payload (placeholder, skipped by decoder)
    enc.unsigned(0); // dlg_payload (placeholder, skipped by decoder)
    enc.unsigned(0); // upd_payload (placeholder, skipped by decoder)
    enc.unsigned(0); // extra — 3rd element of outer array
    enc.into_bytes()
}

#[test]
fn decode_multi_era_block_byron_ebb() {
    let body = build_byron_ebb_body(3, 100, &[0xAA; 32]);
    let envelope = build_multi_era_envelope(0, &body);
    let result = decode_multi_era_block(&envelope).expect("decode");
    match result {
        MultiEraBlock::Byron { block, era_tag } => {
            assert_eq!(era_tag, 0);
            assert!(block.is_ebb());
            assert_eq!(block.epoch(), 3);
            assert_eq!(block.chain_difficulty(), 100);
            assert_eq!(*block.prev_hash(), [0xAA; 32]);
        }
        other => panic!("expected Byron, got {other:?}"),
    }
}

#[test]
fn decode_multi_era_block_byron_main() {
    let body = build_byron_main_body(5, 42, 200, &[0xBB; 32]);
    let envelope = build_multi_era_envelope(1, &body);
    let result = decode_multi_era_block(&envelope).expect("decode");
    match result {
        MultiEraBlock::Byron { block, era_tag } => {
            assert_eq!(era_tag, 1);
            assert!(!block.is_ebb());
            assert_eq!(block.epoch(), 5);
            assert_eq!(block.chain_difficulty(), 200);
            assert_eq!(*block.prev_hash(), [0xBB; 32]);
        }
        other => panic!("expected Byron, got {other:?}"),
    }
}

#[test]
fn decode_multi_era_block_shelley() {
    let block_body = sample_block_bytes();
    let envelope = build_multi_era_envelope(2, &block_body);
    let result = decode_multi_era_block(&envelope).expect("decode shelley");
    match result {
        MultiEraBlock::Shelley(block) => {
            assert_eq!(block.header.body.slot, 500);
            assert_eq!(block.header.body.block_number, 1);
        }
        other => panic!("expected Shelley, got {other:?}"),
    }
}

#[test]
fn decode_multi_era_block_unsupported_tag() {
    let envelope = build_multi_era_envelope(99, &[0x80]); // unknown tag
    let result = decode_multi_era_block(&envelope);
    assert!(result.is_err());
}

#[test]
fn decode_multi_era_block_empty_input() {
    let result = decode_multi_era_block(&[]);
    assert!(result.is_err());
}

#[test]
fn decode_multi_era_blocks_batch() {
    let shelley_envelope = build_multi_era_envelope(2, &sample_block_bytes());
    let byron_envelope = build_multi_era_envelope(0, &build_byron_ebb_body(0, 0, &[0; 32]));
    let blocks =
        decode_multi_era_blocks(&[shelley_envelope, byron_envelope]).expect("decode batch");
    assert_eq!(blocks.len(), 2);
    assert!(matches!(blocks[0], MultiEraBlock::Shelley(_)));
    assert!(matches!(blocks[1], MultiEraBlock::Byron { .. }));
}

// ---------------------------------------------------------------------------
// Phase 37: Verified multi-era sync pipeline
// ---------------------------------------------------------------------------

#[test]
fn multi_era_block_to_block_shelley() {
    let block = ShelleyBlock {
        header: ShelleyHeader {
            body: sample_header_body(),
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        transaction_metadata_set: std::collections::HashMap::new(),
    };
    let me = MultiEraBlock::Shelley(Box::new(block));
    let generic = multi_era_block_to_block(&me, &[]);
    assert_eq!(generic.era, yggdrasil_ledger::Era::Shelley);
    assert_eq!(generic.header.slot_no, SlotNo(500));
    assert_eq!(generic.header.block_no, yggdrasil_ledger::BlockNo(1));
    assert_eq!(generic.header.hash, sample_header_hash());
}

#[test]
fn multi_era_block_to_block_byron() {
    let byron = ByronBlock::EpochBoundary {
        epoch: 2,
        chain_difficulty: 10,
        prev_hash: [0x33; 32],
        raw_header: vec![0xAA, 0xBB],
    };
    let expected_hash = byron.header_hash();
    let me = MultiEraBlock::Byron {
        block: byron,
        era_tag: 0,
    };
    let generic = multi_era_block_to_block(&me, &[]);
    assert_eq!(generic.era, yggdrasil_ledger::Era::Byron);
    assert_eq!(generic.header.hash, expected_hash);
    assert!(generic.transactions.is_empty());
}

#[test]
fn verify_multi_era_block_byron_is_noop() {
    let me = MultiEraBlock::Byron {
        block: ByronBlock::EpochBoundary {
            epoch: 0,
            chain_difficulty: 0,
            prev_hash: [0; 32],
            raw_header: vec![],
        },
        era_tag: 0,
    };
    let config = VerificationConfig {
        slots_per_kes_period: 129600,
        max_kes_evolutions: 62,
        verify_body_hash: false,
        max_major_protocol_version: None,
        future_check: None,
        ocert_counters: None,
        pp_major_protocol_version: None,
        network_magic: None,
    };
    assert!(verify_multi_era_block(&me, &config).is_ok());
}

#[test]
fn apply_multi_era_step_rollforward_adds_block() {
    let block = ShelleyBlock {
        header: ShelleyHeader {
            body: sample_header_body(),
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        transaction_metadata_set: std::collections::HashMap::new(),
    };
    let step = MultiEraSyncStep::RollForward {
        raw_header: vec![],
        tip: Point::BlockPoint(SlotNo(500), sample_header_hash()),
        blocks: vec![MultiEraBlock::Shelley(Box::new(block))],
        raw_blocks: Vec::new(),
        block_spans: Vec::new(),
    };
    let mut store = InMemoryVolatile::default();
    apply_multi_era_step_to_volatile(&mut store, &step).expect("apply");
    assert_eq!(
        store.tip(),
        Point::BlockPoint(SlotNo(500), sample_header_hash())
    );
}

#[test]
fn apply_multi_era_step_rollbackward_resets_tip() {
    let block = ShelleyBlock {
        header: ShelleyHeader {
            body: sample_header_body(),
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        transaction_metadata_set: std::collections::HashMap::new(),
    };
    let fwd_step = MultiEraSyncStep::RollForward {
        raw_header: vec![],
        tip: Point::BlockPoint(SlotNo(500), sample_header_hash()),
        blocks: vec![MultiEraBlock::Shelley(Box::new(block))],
        raw_blocks: Vec::new(),
        block_spans: Vec::new(),
    };
    let mut store = InMemoryVolatile::default();
    apply_multi_era_step_to_volatile(&mut store, &fwd_step).expect("apply fwd");
    assert_eq!(
        store.tip(),
        Point::BlockPoint(SlotNo(500), sample_header_hash())
    );

    let rb_step = MultiEraSyncStep::RollBackward {
        point: Point::Origin,
        tip: Point::Origin,
    };
    apply_multi_era_step_to_volatile(&mut store, &rb_step).expect("apply rb");
    assert_eq!(store.tip(), Point::Origin);
}

#[tokio::test]
async fn sync_step_multi_era_rollforward() {
    let magic = 501;
    let header = ShelleyHeader {
        body: sample_header_body(),
        signature: vec![0xDD; 448],
    };
    let tip = Point::BlockPoint(SlotNo(500), sample_header_hash());
    // Wrap the raw block in a multi-era envelope [2, block_body] for Shelley.
    let block_bytes = build_multi_era_envelope(2, &sample_block_bytes());
    let addr = spawn_typed_rollforward_responder(
        magic,
        header.to_cbor_bytes(),
        Tip::Tip(tip, BlockNo(0)).to_cbor_bytes(),
        block_bytes,
    )
    .await;

    let config = NodeConfig {
        peer_addr: addr,
        network_magic: magic,
        protocol_versions: vec![HandshakeVersion(15)],
        peer_sharing: 1,
    };

    let mut session = bootstrap(&config).await.expect("bootstrap");
    let step = sync_step_multi_era(
        &mut session.chain_sync,
        session.block_fetch.as_mut().expect("block_fetch migrated"),
        Point::Origin,
    )
    .await
    .expect("sync step multi era");

    match step {
        MultiEraSyncStep::RollForward { blocks, .. } => {
            assert_eq!(blocks.len(), 1);
            assert!(matches!(blocks[0], MultiEraBlock::Shelley(_)));
        }
        other => panic!("expected RollForward, got {other:?}"),
    }

    session.mux.abort();
}

// ---------------------------------------------------------------------------
// Phase 40: Mempool sync eviction tests
// ---------------------------------------------------------------------------

fn make_tx_body(fee: u64, ttl: u64) -> ShelleyTxBody {
    ShelleyTxBody {
        inputs: vec![],
        outputs: vec![],
        fee,
        ttl,
        certificates: None,
        withdrawals: None,
        update: None,
        auxiliary_data_hash: None,
    }
}

/// Compute the TxId for a given ShelleyTxBody (same as the sync module does).
fn tx_id_for(body: &ShelleyTxBody) -> TxId {
    let raw = body.to_cbor_bytes();
    TxId(yggdrasil_crypto::hash_bytes_256(&raw).0)
}

#[test]
fn extract_tx_ids_from_shelley_block() {
    let body1 = make_tx_body(100, 1000);
    let body2 = make_tx_body(200, 2000);
    let id1 = tx_id_for(&body1);
    let id2 = tx_id_for(&body2);

    let sb = ShelleyBlock {
        header: ShelleyHeader {
            body: sample_header_body(),
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![body1, body2],
        transaction_witness_sets: vec![
            ShelleyWitnessSet {
                vkey_witnesses: vec![],
                native_scripts: vec![],
                bootstrap_witnesses: vec![],
                plutus_v1_scripts: vec![],
                plutus_data: vec![],
                redeemers: vec![],
                plutus_v2_scripts: vec![],
                plutus_v3_scripts: vec![],
            },
            ShelleyWitnessSet {
                vkey_witnesses: vec![],
                native_scripts: vec![],
                bootstrap_witnesses: vec![],
                plutus_v1_scripts: vec![],
                plutus_data: vec![],
                redeemers: vec![],
                plutus_v2_scripts: vec![],
                plutus_v3_scripts: vec![],
            },
        ],
        transaction_metadata_set: std::collections::HashMap::new(),
    };
    let raw = sb.to_cbor_bytes();
    let block = MultiEraBlock::Shelley(Box::new(sb));
    let spans = yggdrasil_ledger::extract_block_tx_byte_spans(&raw).expect("extract spans");

    let ids = extract_tx_ids(&block, Some(&spans));
    assert_eq!(ids, vec![id1, id2]);
}

#[test]
fn extract_tx_ids_from_byron_ebb_is_empty() {
    let block = MultiEraBlock::Byron {
        block: ByronBlock::EpochBoundary {
            epoch: 0,
            chain_difficulty: 0,
            prev_hash: [0; 32],
            raw_header: vec![],
        },
        era_tag: 0,
    };
    // Byron arm doesn't consult spans.
    assert!(extract_tx_ids(&block, None).is_empty());
}

#[test]
fn extract_tx_ids_from_byron_main_block() {
    use yggdrasil_ledger::{ByronTx, ByronTxAux, ByronTxIn, ByronTxOut};

    let tx1 = ByronTx {
        inputs: vec![ByronTxIn {
            txid: [0xAA; 32],
            index: 0,
        }],
        outputs: vec![ByronTxOut {
            address: vec![0x01; 20],
            amount: 1_000_000,
        }],
        attributes: vec![0xa0], // empty CBOR map
    };
    let tx2 = ByronTx {
        inputs: vec![ByronTxIn {
            txid: [0xBB; 32],
            index: 1,
        }],
        outputs: vec![ByronTxOut {
            address: vec![0x02; 20],
            amount: 2_000_000,
        }],
        attributes: vec![0xa0],
    };

    let expected_id1 = TxId(tx1.tx_id());
    let expected_id2 = TxId(tx2.tx_id());

    let block = MultiEraBlock::Byron {
        block: ByronBlock::MainBlock {
            epoch: 1,
            slot_in_epoch: 5,
            chain_difficulty: 10,
            prev_hash: [0; 32],
            issuer_vkey: [0x11; 32],
            raw_header: vec![],
            transactions: vec![
                ByronTxAux {
                    tx: tx1,
                    witnesses: vec![],
                    raw_tx_cbor: Vec::new(),
                },
                ByronTxAux {
                    tx: tx2,
                    witnesses: vec![],
                    raw_tx_cbor: Vec::new(),
                },
            ],
        },
        era_tag: 1,
    };

    // Byron arm doesn't consult spans.
    let ids = extract_tx_ids(&block, None);
    assert_eq!(ids.len(), 2);
    assert_eq!(ids[0], expected_id1);
    assert_eq!(ids[1], expected_id2);
}

#[test]
fn evict_confirmed_removes_matching_mempool_entries() {
    let body = make_tx_body(500, 10_000);
    let id = tx_id_for(&body);

    let mut mempool = Mempool::with_capacity(1_000_000);
    let entry = MempoolEntry {
        era: Era::Shelley,
        tx_id: id,
        fee: 500,
        body: body.to_cbor_bytes(),
        raw_tx: body.to_cbor_bytes(),
        size_bytes: 100,
        ttl: SlotNo(10_000),
        inputs: vec![],
    };
    mempool.insert(entry).expect("insert");
    assert_eq!(mempool.len(), 1);

    let step = MultiEraSyncStep::RollForward {
        raw_header: vec![],
        tip: Point::BlockPoint(SlotNo(500), sample_header_hash()),
        blocks: vec![MultiEraBlock::Shelley(Box::new(ShelleyBlock {
            header: ShelleyHeader {
                body: sample_header_body(),
                signature: vec![0xDD; 448],
            },
            transaction_bodies: vec![body],
            transaction_witness_sets: vec![ShelleyWitnessSet {
                vkey_witnesses: vec![],
                native_scripts: vec![],
                bootstrap_witnesses: vec![],
                plutus_v1_scripts: vec![],
                plutus_data: vec![],
                redeemers: vec![],
                plutus_v2_scripts: vec![],
                plutus_v3_scripts: vec![],
            }],
            transaction_metadata_set: std::collections::HashMap::new(),
        }))],
        raw_blocks: Vec::new(),
        block_spans: Vec::new(),
    };

    let evicted = evict_confirmed_from_mempool(&mut mempool, &step);
    assert_eq!(evicted, 1);
    assert_eq!(mempool.len(), 0);
}

#[test]
fn evict_confirmed_also_purges_expired() {
    let body1 = make_tx_body(100, 5); // expires at slot 5
    let body2 = make_tx_body(200, 10_000); // valid for a long time
    let id1 = tx_id_for(&body1);
    let id2 = tx_id_for(&body2);

    let mut mempool = Mempool::with_capacity(1_000_000);
    mempool
        .insert(MempoolEntry {
            era: Era::Shelley,
            tx_id: id1,
            fee: 100,
            body: body1.to_cbor_bytes(),
            raw_tx: body1.to_cbor_bytes(),
            size_bytes: 50,
            ttl: SlotNo(5),
            inputs: vec![],
        })
        .expect("insert body1");
    mempool
        .insert(MempoolEntry {
            era: Era::Shelley,
            tx_id: id2,
            fee: 200,
            body: body2.to_cbor_bytes(),
            raw_tx: body2.to_cbor_bytes(),
            size_bytes: 50,
            ttl: SlotNo(10_000),
            inputs: vec![],
        })
        .expect("insert body2");
    assert_eq!(mempool.len(), 2);

    // Roll forward to slot 500 — body1 is expired (ttl 5 < 500),
    // body2 is not in the block and not expired, so stays.
    let step = MultiEraSyncStep::RollForward {
        raw_header: vec![],
        tip: Point::BlockPoint(SlotNo(500), sample_header_hash()),
        blocks: vec![MultiEraBlock::Shelley(Box::new(ShelleyBlock {
            header: ShelleyHeader {
                body: sample_header_body(),
                signature: vec![0xDD; 448],
            },
            transaction_bodies: vec![],
            transaction_witness_sets: vec![],
            transaction_metadata_set: std::collections::HashMap::new(),
        }))],
        raw_blocks: Vec::new(),
        block_spans: Vec::new(),
    };

    let evicted = evict_confirmed_from_mempool(&mut mempool, &step);
    assert_eq!(evicted, 1); // body1 expired
    assert_eq!(mempool.len(), 1);
    assert!(mempool.contains(&id2));
}

#[test]
fn evict_confirmed_rollback_does_nothing() {
    let body = make_tx_body(100, 10_000);
    let id = tx_id_for(&body);

    let mut mempool = Mempool::with_capacity(1_000_000);
    mempool
        .insert(MempoolEntry {
            era: Era::Shelley,
            tx_id: id,
            fee: 100,
            body: body.to_cbor_bytes(),
            raw_tx: body.to_cbor_bytes(),
            size_bytes: 50,
            ttl: SlotNo(10_000),
            inputs: vec![],
        })
        .expect("insert");
    assert_eq!(mempool.len(), 1);

    let step = MultiEraSyncStep::RollBackward {
        point: Point::Origin,
        tip: Point::Origin,
    };

    let evicted = evict_confirmed_from_mempool(&mut mempool, &step);
    assert_eq!(evicted, 0);
    assert_eq!(mempool.len(), 1);
}

// ---------------------------------------------------------------------------
// Phase 47: Multi-era block decode expansion
// ---------------------------------------------------------------------------

/// Build a sample Babbage block (no transactions) and return its CBOR bytes.
fn sample_babbage_block_bytes() -> Vec<u8> {
    let block = BabbageBlock {
        header: PraosHeader {
            body: sample_praos_header_body(),
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        auxiliary_data_set: std::collections::HashMap::new(),
        invalid_transactions: vec![],
    };
    block.to_cbor_bytes()
}

/// Build a sample Conway block (no transactions) and return its CBOR bytes.
fn sample_conway_block_bytes() -> Vec<u8> {
    let block = ConwayBlock {
        header: PraosHeader {
            body: sample_praos_header_body(),
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        auxiliary_data_set: std::collections::HashMap::new(),
        invalid_transactions: vec![],
    };
    block.to_cbor_bytes()
}

#[test]
fn decode_multi_era_block_babbage() {
    let block_body = sample_babbage_block_bytes();
    let envelope = build_multi_era_envelope(6, &block_body);
    let result = decode_multi_era_block(&envelope).expect("decode babbage");
    match result {
        MultiEraBlock::Babbage(block) => {
            assert_eq!(block.header.body.slot, 500);
            assert_eq!(block.header.body.block_number, 1);
        }
        other => panic!("expected Babbage, got {other:?}"),
    }
}

#[test]
fn decode_multi_era_block_conway() {
    let block_body = sample_conway_block_bytes();
    let envelope = build_multi_era_envelope(7, &block_body);
    let result = decode_multi_era_block(&envelope).expect("decode conway");
    match result {
        MultiEraBlock::Conway(block) => {
            assert_eq!(block.header.body.slot, 500);
            assert_eq!(block.header.body.block_number, 1);
        }
        other => panic!("expected Conway, got {other:?}"),
    }
}

#[test]
fn decode_multi_era_blocks_all_eras() {
    let shelley = build_multi_era_envelope(2, &sample_block_bytes());
    let byron = build_multi_era_envelope(0, &build_byron_ebb_body(0, 0, &[0; 32]));
    let babbage = build_multi_era_envelope(6, &sample_babbage_block_bytes());
    let conway = build_multi_era_envelope(7, &sample_conway_block_bytes());
    let blocks =
        decode_multi_era_blocks(&[shelley, byron, babbage, conway]).expect("decode all eras");
    assert_eq!(blocks.len(), 4);
    assert!(matches!(blocks[0], MultiEraBlock::Shelley(_)));
    assert!(matches!(blocks[1], MultiEraBlock::Byron { .. }));
    assert!(matches!(blocks[2], MultiEraBlock::Babbage(_)));
    assert!(matches!(blocks[3], MultiEraBlock::Conway(_)));
}

#[test]
fn multi_era_block_to_block_babbage() {
    let block = BabbageBlock {
        header: PraosHeader {
            body: sample_praos_header_body(),
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        auxiliary_data_set: std::collections::HashMap::new(),
        invalid_transactions: vec![],
    };
    let me = MultiEraBlock::Babbage(Box::new(block));
    let generic = multi_era_block_to_block(&me, &[]);
    assert_eq!(generic.era, yggdrasil_ledger::Era::Babbage);
    assert_eq!(generic.header.slot_no, SlotNo(500));
    assert_eq!(generic.header.block_no, yggdrasil_ledger::BlockNo(1));
    assert_eq!(generic.header.hash, sample_praos_header_hash());
}

#[test]
fn multi_era_block_to_block_conway() {
    let block = ConwayBlock {
        header: PraosHeader {
            body: sample_praos_header_body(),
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        auxiliary_data_set: std::collections::HashMap::new(),
        invalid_transactions: vec![],
    };
    let me = MultiEraBlock::Conway(Box::new(block));
    let generic = multi_era_block_to_block(&me, &[]);
    assert_eq!(generic.era, yggdrasil_ledger::Era::Conway);
    assert_eq!(generic.header.slot_no, SlotNo(500));
    assert_eq!(generic.header.block_no, yggdrasil_ledger::BlockNo(1));
    assert_eq!(generic.header.hash, sample_praos_header_hash());
}

fn make_babbage_tx_body(fee: u64) -> BabbageTxBody {
    BabbageTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0xAA; 32],
            index: 0,
        }],
        outputs: vec![BabbageTxOut {
            address: vec![0x01; 29],
            amount: yggdrasil_ledger::Value::Coin(1_000_000),
            datum_option: None,
            script_ref: None,
        }],
        fee,
        ttl: None,
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
        collateral_return: None,
        total_collateral: None,
        reference_inputs: None,
    }
}

fn make_conway_tx_body(fee: u64) -> ConwayTxBody {
    ConwayTxBody {
        inputs: vec![ShelleyTxIn {
            transaction_id: [0xBB; 32],
            index: 0,
        }],
        outputs: vec![BabbageTxOut {
            address: vec![0x01; 29],
            amount: yggdrasil_ledger::Value::Coin(2_000_000),
            datum_option: None,
            script_ref: None,
        }],
        fee,
        ttl: None,
        certificates: None,
        withdrawals: None,
        auxiliary_data_hash: None,
        validity_interval_start: None,
        mint: None,
        script_data_hash: None,
        collateral: None,
        required_signers: None,
        network_id: None,
        collateral_return: None,
        total_collateral: None,
        reference_inputs: None,
        voting_procedures: None,
        proposal_procedures: None,
        current_treasury_value: None,
        treasury_donation: None,
    }
}

#[test]
fn extract_tx_ids_babbage() {
    let body = make_babbage_tx_body(200);
    let expected_id = TxId(yggdrasil_crypto::hash_bytes_256(&body.to_cbor_bytes()).0);

    let block = BabbageBlock {
        header: PraosHeader {
            body: sample_praos_header_body(),
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![body],
        transaction_witness_sets: vec![ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![],
            plutus_data: vec![],
            redeemers: vec![],
            plutus_v2_scripts: vec![],
            plutus_v3_scripts: vec![],
        }],
        auxiliary_data_set: std::collections::HashMap::new(),
        invalid_transactions: vec![],
    };
    let raw = block.to_cbor_bytes();
    let spans = yggdrasil_ledger::extract_block_tx_byte_spans(&raw).expect("extract spans");
    let me = MultiEraBlock::Babbage(Box::new(block));
    let ids = extract_tx_ids(&me, Some(&spans));
    assert_eq!(ids.len(), 1);
    assert_eq!(ids[0], expected_id);
}

#[test]
fn extract_tx_ids_conway() {
    let body = make_conway_tx_body(300);
    let expected_id = TxId(yggdrasil_crypto::hash_bytes_256(&body.to_cbor_bytes()).0);

    let block = ConwayBlock {
        header: PraosHeader {
            body: sample_praos_header_body(),
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![body],
        transaction_witness_sets: vec![ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![],
            plutus_data: vec![],
            redeemers: vec![],
            plutus_v2_scripts: vec![],
            plutus_v3_scripts: vec![],
        }],
        auxiliary_data_set: std::collections::HashMap::new(),
        invalid_transactions: vec![],
    };
    let raw = block.to_cbor_bytes();
    let spans = yggdrasil_ledger::extract_block_tx_byte_spans(&raw).expect("extract spans");
    let me = MultiEraBlock::Conway(Box::new(block));
    let ids = extract_tx_ids(&me, Some(&spans));
    assert_eq!(ids.len(), 1);
    assert_eq!(ids[0], expected_id);
}

#[test]
fn babbage_block_round_trip_decode() {
    let body = make_babbage_tx_body(500);
    let block = BabbageBlock {
        header: PraosHeader {
            body: sample_praos_header_body(),
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![body],
        transaction_witness_sets: vec![ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![],
            plutus_data: vec![],
            redeemers: vec![],
            plutus_v2_scripts: vec![],
            plutus_v3_scripts: vec![],
        }],
        auxiliary_data_set: std::collections::HashMap::new(),
        invalid_transactions: vec![],
    };
    let block_bytes = block.to_cbor_bytes();
    let envelope = build_multi_era_envelope(6, &block_bytes);
    let decoded = decode_multi_era_block(&envelope).expect("round-trip decode");
    match decoded {
        MultiEraBlock::Babbage(b) => {
            assert_eq!(b.transaction_bodies.len(), 1);
            assert_eq!(b.transaction_bodies[0].fee, 500);
        }
        other => panic!("expected Babbage, got {other:?}"),
    }
}

#[test]
fn conway_block_round_trip_decode() {
    let body = make_conway_tx_body(700);
    let block = ConwayBlock {
        header: PraosHeader {
            body: sample_praos_header_body(),
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![body],
        transaction_witness_sets: vec![ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![],
            plutus_data: vec![],
            redeemers: vec![],
            plutus_v2_scripts: vec![],
            plutus_v3_scripts: vec![],
        }],
        auxiliary_data_set: std::collections::HashMap::new(),
        invalid_transactions: vec![],
    };
    let block_bytes = block.to_cbor_bytes();
    let envelope = build_multi_era_envelope(7, &block_bytes);
    let decoded = decode_multi_era_block(&envelope).expect("round-trip decode");
    match decoded {
        MultiEraBlock::Conway(b) => {
            assert_eq!(b.transaction_bodies.len(), 1);
            assert_eq!(b.transaction_bodies[0].fee, 700);
        }
        other => panic!("expected Conway, got {other:?}"),
    }
}

#[test]
fn multi_era_block_to_block_babbage_with_txs() {
    let body = make_babbage_tx_body(250);
    let expected_id = TxId(yggdrasil_crypto::hash_bytes_256(&body.to_cbor_bytes()).0);
    let block = BabbageBlock {
        header: PraosHeader {
            body: sample_praos_header_body(),
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![body],
        transaction_witness_sets: vec![ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![],
            plutus_data: vec![],
            redeemers: vec![],
            plutus_v2_scripts: vec![],
            plutus_v3_scripts: vec![],
        }],
        auxiliary_data_set: std::collections::HashMap::new(),
        invalid_transactions: vec![],
    };
    let me = MultiEraBlock::Babbage(Box::new(block));
    let generic = multi_era_block_to_block(&me, &[]);
    assert_eq!(generic.transactions.len(), 1);
    assert_eq!(generic.transactions[0].id, expected_id);
}

#[test]
fn multi_era_block_to_block_conway_with_txs() {
    let body = make_conway_tx_body(350);
    let expected_id = TxId(yggdrasil_crypto::hash_bytes_256(&body.to_cbor_bytes()).0);
    let block = ConwayBlock {
        header: PraosHeader {
            body: sample_praos_header_body(),
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![body],
        transaction_witness_sets: vec![ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![],
            plutus_data: vec![],
            redeemers: vec![],
            plutus_v2_scripts: vec![],
            plutus_v3_scripts: vec![],
        }],
        auxiliary_data_set: std::collections::HashMap::new(),
        invalid_transactions: vec![],
    };
    let me = MultiEraBlock::Conway(Box::new(block));
    let generic = multi_era_block_to_block(&me, &[]);
    assert_eq!(generic.transactions.len(), 1);
    assert_eq!(generic.transactions[0].id, expected_id);
}

#[test]
fn verify_multi_era_block_babbage_passes() {
    let block = BabbageBlock {
        header: PraosHeader {
            body: sample_praos_header_body(),
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        auxiliary_data_set: std::collections::HashMap::new(),
        invalid_transactions: vec![],
    };
    let me = MultiEraBlock::Babbage(Box::new(block));
    // Verification will fail on signature, but the match arm itself is exercised.
    let config = VerificationConfig {
        slots_per_kes_period: 129600,
        max_kes_evolutions: 62,
        verify_body_hash: false,
        max_major_protocol_version: None,
        future_check: None,
        ocert_counters: None,
        pp_major_protocol_version: None,
        network_magic: None,
    };
    let result = verify_multi_era_block(&me, &config);
    // Expect error since the signature is dummy bytes, confirming the
    // Babbage arm delegates to verify_shelley_header.
    assert!(result.is_err());
}

#[test]
fn verify_multi_era_block_conway_passes() {
    let block = ConwayBlock {
        header: PraosHeader {
            body: sample_praos_header_body(),
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        auxiliary_data_set: std::collections::HashMap::new(),
        invalid_transactions: vec![],
    };
    let me = MultiEraBlock::Conway(Box::new(block));
    let config = VerificationConfig {
        slots_per_kes_period: 129600,
        max_kes_evolutions: 62,
        verify_body_hash: false,
        max_major_protocol_version: None,
        future_check: None,
        ocert_counters: None,
        pp_major_protocol_version: None,
        network_magic: None,
    };
    let result = verify_multi_era_block(&me, &config);
    assert!(result.is_err());
}

// ===========================================================================
// Block body hash verification tests
// ===========================================================================

/// Build a Shelley block with a correct block_body_hash in its header.
///
/// We first encode the block with a dummy hash, compute the real body hash,
/// then re-encode with the corrected header.
fn make_shelley_block_with_correct_body_hash() -> ShelleyBlock {
    // Build the block body parts.
    let bodies = vec![make_tx_body(999, 5000)];
    let witnesses = vec![ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![],
        plutus_data: vec![],
        redeemers: vec![],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![],
    }];
    let metadata = std::collections::HashMap::new();

    // First pass: encode with dummy hash to compute the real body hash.
    let dummy_block = ShelleyBlock {
        header: ShelleyHeader {
            body: ShelleyHeaderBody {
                block_body_hash: [0x00; 32],
                ..sample_header_body()
            },
            signature: vec![0xDD; 448],
        },
        transaction_bodies: bodies.clone(),
        transaction_witness_sets: witnesses.clone(),
        transaction_metadata_set: metadata.clone(),
    };
    let dummy_bytes = dummy_block.to_cbor_bytes();
    let real_body_hash = compute_block_body_hash(&dummy_bytes).expect("compute hash");

    // Second pass: the body hash only depends on elements 1..N (not the
    // header), so the hash computed from the dummy block is already correct.
    ShelleyBlock {
        header: ShelleyHeader {
            body: ShelleyHeaderBody {
                block_body_hash: real_body_hash,
                ..sample_header_body()
            },
            signature: vec![0xDD; 448],
        },
        transaction_bodies: bodies,
        transaction_witness_sets: witnesses,
        transaction_metadata_set: metadata,
    }
}

/// Build a Babbage block with a correct block_body_hash in its header.
fn make_babbage_block_with_correct_body_hash() -> BabbageBlock {
    let bodies = vec![make_babbage_tx_body(200)];
    let witnesses = vec![ShelleyWitnessSet {
        vkey_witnesses: vec![],
        native_scripts: vec![],
        bootstrap_witnesses: vec![],
        plutus_v1_scripts: vec![],
        plutus_data: vec![],
        redeemers: vec![],
        plutus_v2_scripts: vec![],
        plutus_v3_scripts: vec![],
    }];

    let dummy_block = BabbageBlock {
        header: PraosHeader {
            body: PraosHeaderBody {
                block_body_hash: [0x00; 32],
                ..sample_praos_header_body()
            },
            signature: vec![0xDD; 448],
        },
        transaction_bodies: bodies.clone(),
        transaction_witness_sets: witnesses.clone(),
        auxiliary_data_set: std::collections::HashMap::new(),
        invalid_transactions: vec![],
    };
    let dummy_bytes = dummy_block.to_cbor_bytes();
    let real_body_hash = compute_block_body_hash(&dummy_bytes).expect("compute hash");

    BabbageBlock {
        header: PraosHeader {
            body: PraosHeaderBody {
                block_body_hash: real_body_hash,
                ..sample_praos_header_body()
            },
            signature: vec![0xDD; 448],
        },
        transaction_bodies: bodies,
        transaction_witness_sets: witnesses,
        auxiliary_data_set: std::collections::HashMap::new(),
        invalid_transactions: vec![],
    }
}

#[test]
fn verify_block_body_hash_shelley_valid() {
    let block = make_shelley_block_with_correct_body_hash();
    let block_bytes = block.to_cbor_bytes();
    let envelope = build_multi_era_envelope(2, &block_bytes);
    verify_block_body_hash(&envelope).expect("valid shelley body hash");
}

#[test]
fn verify_block_body_hash_babbage_valid() {
    let block = make_babbage_block_with_correct_body_hash();
    let block_bytes = block.to_cbor_bytes();
    let envelope = build_multi_era_envelope(6, &block_bytes);
    verify_block_body_hash(&envelope).expect("valid babbage body hash");
}

#[test]
fn verify_block_body_hash_byron_skipped() {
    let envelope = build_multi_era_envelope(0, &[0x80]);
    verify_block_body_hash(&envelope).expect("byron blocks are skipped");
}

#[test]
fn verify_block_body_hash_mismatch_rejected() {
    // Build a block with a wrong body hash in the header.
    let block = ShelleyBlock {
        header: ShelleyHeader {
            body: ShelleyHeaderBody {
                block_body_hash: [0xFF; 32], // deliberately wrong
                ..sample_header_body()
            },
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![make_tx_body(100, 1000)],
        transaction_witness_sets: vec![ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![],
            plutus_data: vec![],
            redeemers: vec![],
            plutus_v2_scripts: vec![],
            plutus_v3_scripts: vec![],
        }],
        transaction_metadata_set: std::collections::HashMap::new(),
    };
    let block_bytes = block.to_cbor_bytes();
    let envelope = build_multi_era_envelope(2, &block_bytes);
    let result = verify_block_body_hash(&envelope);
    assert!(result.is_err(), "should reject mismatched body hash");
}

#[test]
fn verify_block_body_hash_babbage_mismatch_rejected() {
    let block = BabbageBlock {
        header: PraosHeader {
            body: PraosHeaderBody {
                block_body_hash: [0xFF; 32], // deliberately wrong
                ..sample_praos_header_body()
            },
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![make_babbage_tx_body(500)],
        transaction_witness_sets: vec![ShelleyWitnessSet {
            vkey_witnesses: vec![],
            native_scripts: vec![],
            bootstrap_witnesses: vec![],
            plutus_v1_scripts: vec![],
            plutus_data: vec![],
            redeemers: vec![],
            plutus_v2_scripts: vec![],
            plutus_v3_scripts: vec![],
        }],
        auxiliary_data_set: std::collections::HashMap::new(),
        invalid_transactions: vec![],
    };
    let block_bytes = block.to_cbor_bytes();
    let envelope = build_multi_era_envelope(6, &block_bytes);
    let result = verify_block_body_hash(&envelope);
    assert!(
        result.is_err(),
        "should reject mismatched babbage body hash"
    );
}

#[test]
fn compute_block_body_hash_shelley_round_trip() {
    let block = make_shelley_block_with_correct_body_hash();
    let block_bytes = block.to_cbor_bytes();
    let computed = compute_block_body_hash(&block_bytes).expect("compute");
    assert_eq!(computed, block.header.body.block_body_hash);
}

#[test]
fn compute_block_body_hash_babbage_round_trip() {
    let block = make_babbage_block_with_correct_body_hash();
    let block_bytes = block.to_cbor_bytes();
    let computed = compute_block_body_hash(&block_bytes).expect("compute");
    assert_eq!(computed, block.header.body.block_body_hash);
}

// ===========================================================================
// Cross-subsystem parity integration
// ===========================================================================

/// Tests the full pipeline: decode multi-era block → convert to Block →
/// store in volatile → track in consensus ChainState → drain stable to
/// immutable.
#[test]
fn cross_subsystem_block_to_chain_state_to_storage() {
    use yggdrasil_consensus::{ChainEntry, ChainState, SecurityParam};
    use yggdrasil_ledger::{Block, BlockHeader, BlockNo, Era};
    use yggdrasil_storage::{ImmutableStore, InMemoryImmutable, InMemoryVolatile, VolatileStore};

    let k = SecurityParam(2);
    let mut chain_state = ChainState::new(k);
    let mut volatile = InMemoryVolatile::default();
    let mut immutable = InMemoryImmutable::default();

    // Create 4 blocks to push 2 past the stability window (k=2).
    for i in 0u64..4 {
        let hash = {
            let mut h = [0u8; 32];
            h[0] = i as u8;
            HeaderHash(h)
        };
        let block = Block {
            era: Era::Shelley,
            header: BlockHeader {
                hash,
                prev_hash: if i == 0 {
                    HeaderHash([0; 32])
                } else {
                    let mut h = [0u8; 32];
                    h[0] = (i - 1) as u8;
                    HeaderHash(h)
                },
                slot_no: SlotNo(i * 10),
                block_no: BlockNo(i),
                issuer_vkey: [0; 32],
                protocol_version: None,
            },
            transactions: vec![],
            raw_cbor: None,
            header_cbor_size: None,
        };

        volatile.add_block(block.clone()).expect("volatile add");
        chain_state
            .roll_forward(ChainEntry {
                hash,
                slot: SlotNo(i * 10),
                block_no: BlockNo(i),
                prev_hash: None,
            })
            .expect("chain forward");
    }

    assert_eq!(chain_state.stable_count(), 2);
    assert_eq!(chain_state.volatile_len(), 4);

    // Drain stable entries and promote to immutable.
    let stable = chain_state.drain_stable();
    for entry in &stable {
        let block = volatile.get_block(&entry.hash).expect("block in volatile");
        immutable
            .append_block(block.clone())
            .expect("immutable append");
    }

    assert_eq!(immutable.len(), 2);
    assert_eq!(chain_state.volatile_len(), 2);

    // Verify immutable tip matches the last drained entry.
    assert_eq!(
        immutable.get_tip(),
        Point::BlockPoint(stable[1].slot, stable[1].hash)
    );
}

/// Tests rollback through consensus ChainState and volatile storage.
#[test]
fn cross_subsystem_rollback_flow() {
    use yggdrasil_consensus::{ChainEntry, ChainState, SecurityParam};
    use yggdrasil_ledger::{Block, BlockHeader, BlockNo, Era};
    use yggdrasil_storage::{InMemoryVolatile, VolatileStore};

    let k = SecurityParam(5);
    let mut chain_state = ChainState::new(k);
    let mut volatile = InMemoryVolatile::default();

    // Forward 3 blocks.
    for i in 0u64..3 {
        let hash = {
            let mut h = [0u8; 32];
            h[0] = i as u8;
            HeaderHash(h)
        };
        let block = Block {
            era: Era::Shelley,
            header: BlockHeader {
                hash,
                prev_hash: HeaderHash([0; 32]),
                slot_no: SlotNo(i * 10),
                block_no: BlockNo(i),
                issuer_vkey: [0; 32],
                protocol_version: None,
            },
            transactions: vec![],
            raw_cbor: None,
            header_cbor_size: None,
        };
        volatile.add_block(block).expect("add");
        chain_state
            .roll_forward(ChainEntry {
                hash,
                slot: SlotNo(i * 10),
                block_no: BlockNo(i),
                prev_hash: None,
            })
            .expect("forward");
    }

    // Rollback to block 1 (slot 10).
    let rollback_point = Point::BlockPoint(SlotNo(10), {
        let mut h = [0u8; 32];
        h[0] = 1;
        HeaderHash(h)
    });
    chain_state
        .roll_backward(&rollback_point)
        .expect("rollback");
    volatile.rollback_to(&rollback_point);

    assert_eq!(chain_state.tip(), rollback_point);
    assert_eq!(volatile.tip(), rollback_point);

    // Block at slot 20 should be gone from volatile.
    let removed_hash = {
        let mut h = [0u8; 32];
        h[0] = 2;
        HeaderHash(h)
    };
    assert!(volatile.get_block(&removed_hash).is_none());
}

// ---------------------------------------------------------------------------
// Nonce evolution integration with MultiEraBlock
// ---------------------------------------------------------------------------

use yggdrasil_consensus::{
    EpochSize, NonceEvolutionConfig, NonceEvolutionState, praos_vrf_output_to_nonce,
    vrf_output_to_nonce,
};

fn nonce_test_config() -> NonceEvolutionConfig {
    NonceEvolutionConfig {
        epoch_size: EpochSize(100),
        stability_window: 30,
        extra_entropy: Nonce::Neutral,
    }
}

fn make_shelley_block(slot: u64, nonce_vrf_seed: u8, prev_hash: Option<[u8; 32]>) -> ShelleyBlock {
    let mut nonce_out = vec![0u8; 64];
    nonce_out[0] = nonce_vrf_seed;

    ShelleyBlock {
        header: ShelleyHeader {
            body: ShelleyHeaderBody {
                block_number: 1,
                slot,
                prev_hash,
                issuer_vkey: [0u8; 32],
                vrf_vkey: [0u8; 32],
                nonce_vrf: ShelleyVrfCert {
                    output: nonce_out,
                    proof: [0u8; 80],
                },
                leader_vrf: ShelleyVrfCert {
                    output: vec![0u8; 64],
                    proof: [0u8; 80],
                },
                block_body_size: 0,
                block_body_hash: [0u8; 32],
                operational_cert: ShelleyOpCert {
                    hot_vkey: [0u8; 32],
                    sequence_number: 0,
                    kes_period: 0,
                    sigma: [0u8; 64],
                },
                protocol_version: (2, 0),
            },
            signature: vec![0u8; 448],
        },
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        transaction_metadata_set: std::collections::HashMap::new(),
    }
}

fn make_babbage_block(slot: u64, vrf_seed: u8, prev_hash: Option<[u8; 32]>) -> BabbageBlock {
    let mut vrf_out = vec![0u8; 64];
    vrf_out[0] = vrf_seed;

    BabbageBlock {
        header: PraosHeader {
            body: PraosHeaderBody {
                block_number: 1,
                slot,
                prev_hash,
                issuer_vkey: [0u8; 32],
                vrf_vkey: [0u8; 32],
                vrf_result: ShelleyVrfCert {
                    output: vrf_out,
                    proof: [0u8; 80],
                },
                block_body_size: 0,
                block_body_hash: [0u8; 32],
                operational_cert: ShelleyOpCert {
                    hot_vkey: [0u8; 32],
                    sequence_number: 0,
                    kes_period: 0,
                    sigma: [0u8; 64],
                },
                protocol_version: (7, 0),
            },
            signature: vec![0u8; 448],
        },
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        auxiliary_data_set: std::collections::HashMap::new(),
        invalid_transactions: vec![],
    }
}

#[test]
fn apply_nonce_evolution_shelley_block() {
    let config = nonce_test_config();
    let init = Nonce::Hash([0u8; 32]);
    let mut state = NonceEvolutionState::new(init);

    let prev = [0xAA; 32];
    let block = make_shelley_block(5, 42, Some(prev));
    let me_block = MultiEraBlock::Shelley(Box::new(block.clone()));

    apply_nonce_evolution(&mut state, &me_block, &config);

    let eta = vrf_output_to_nonce(&block.header.body.nonce_vrf.output);
    assert_eq!(state.evolving_nonce, init.combine(eta));
    assert_eq!(state.lab_nonce, Nonce::from_header_hash(HeaderHash(prev)));
}

#[test]
fn apply_nonce_evolution_babbage_block() {
    let config = nonce_test_config();
    let init = Nonce::Hash([0u8; 32]);
    let mut state = NonceEvolutionState::new(init);

    let prev = [0xBB; 32];
    let block = make_babbage_block(10, 99, Some(prev));
    let me_block = MultiEraBlock::Babbage(Box::new(block.clone()));

    apply_nonce_evolution(&mut state, &me_block, &config);

    // Babbage uses Praos derivation: Blake2b-256(Blake2b-256("N" || output))
    let eta = praos_vrf_output_to_nonce(&block.header.body.vrf_result.output);
    assert_eq!(state.evolving_nonce, init.combine(eta));
    assert_eq!(state.lab_nonce, Nonce::from_header_hash(HeaderHash(prev)));
}

#[test]
fn apply_nonce_evolution_byron_is_no_op() {
    let config = nonce_test_config();
    let init = Nonce::Hash([0x11; 32]);
    let mut state = NonceEvolutionState::new(init);
    let before = state.clone();

    let byron = MultiEraBlock::Byron {
        block: ByronBlock::MainBlock {
            epoch: 0,
            slot_in_epoch: 0,
            chain_difficulty: 0,
            prev_hash: [0; 32],
            issuer_vkey: [0u8; 32],
            raw_header: vec![],
            transactions: vec![],
        },
        era_tag: 1,
    };
    apply_nonce_evolution(&mut state, &byron, &config);

    assert_eq!(state, before);
}

#[test]
fn apply_nonce_evolution_epoch_transition_via_shelley_blocks() {
    // epoch_size=100, stability_window=30.
    let config = nonce_test_config();
    let init = Nonce::Hash([0u8; 32]);
    let mut state = NonceEvolutionState::new(init);

    // Feed blocks in epoch 0 (slots 0..99).
    for i in 0u8..10 {
        let block = make_shelley_block(i as u64 * 5, i + 1, Some([i; 32]));
        apply_nonce_evolution(
            &mut state,
            &MultiEraBlock::Shelley(Box::new(block)),
            &config,
        );
    }
    assert_eq!(state.current_epoch, yggdrasil_ledger::EpochNo(0));

    let epoch_nonce_before = state.epoch_nonce;

    // Feed a block in epoch 1 (slot 100) to trigger TICKN.
    let block = make_shelley_block(100, 200, Some([0xFF; 32]));
    apply_nonce_evolution(
        &mut state,
        &MultiEraBlock::Shelley(Box::new(block)),
        &config,
    );

    assert_eq!(state.current_epoch, yggdrasil_ledger::EpochNo(1));
    // Epoch nonce should have changed.
    assert_ne!(state.epoch_nonce, epoch_nonce_before);
}

#[test]
fn apply_nonce_evolution_mixed_eras() {
    // Transition from Shelley to Babbage blocks — nonce evolution should
    // be continuous.
    let config = nonce_test_config();
    let init = Nonce::Hash([0u8; 32]);
    let mut state = NonceEvolutionState::new(init);

    // Shelley block at slot 0.
    let s_block = make_shelley_block(0, 1, Some([0xAA; 32]));
    apply_nonce_evolution(
        &mut state,
        &MultiEraBlock::Shelley(Box::new(s_block)),
        &config,
    );
    let nonce_after_shelley = state.evolving_nonce;

    // Babbage block at slot 1.
    let b_block = make_babbage_block(1, 2, Some([0xBB; 32]));
    apply_nonce_evolution(
        &mut state,
        &MultiEraBlock::Babbage(Box::new(b_block.clone())),
        &config,
    );

    // Evolving nonce should accumulate over both blocks.
    // Babbage uses Praos derivation: Blake2b-256(Blake2b-256("N" || output))
    let eta_b = praos_vrf_output_to_nonce(&b_block.header.body.vrf_result.output);
    assert_eq!(state.evolving_nonce, nonce_after_shelley.combine(eta_b));
}

// ---------------------------------------------------------------------------
// ChainState integration with sync pipeline
// ---------------------------------------------------------------------------

/// Build a Shelley block with a specified block number and slot.
fn make_shelley_block_with_number(block_no: u64, slot: u64) -> ShelleyBlock {
    ShelleyBlock {
        header: ShelleyHeader {
            body: ShelleyHeaderBody {
                block_number: block_no,
                slot,
                prev_hash: None,
                issuer_vkey: [0u8; 32],
                vrf_vkey: [0u8; 32],
                nonce_vrf: ShelleyVrfCert {
                    output: vec![0u8; 64],
                    proof: [0u8; 80],
                },
                leader_vrf: ShelleyVrfCert {
                    output: vec![0u8; 64],
                    proof: [0u8; 80],
                },
                block_body_size: 0,
                block_body_hash: [0u8; 32],
                operational_cert: ShelleyOpCert {
                    hot_vkey: [0u8; 32],
                    sequence_number: 0,
                    kes_period: 0,
                    sigma: [0u8; 64],
                },
                protocol_version: (2, 0),
            },
            signature: vec![0u8; 448],
        },
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        transaction_metadata_set: std::collections::HashMap::new(),
    }
}

#[test]
fn chain_entry_from_shelley_block() {
    let block = make_shelley_block_with_number(42, 100);
    let me = MultiEraBlock::Shelley(Box::new(block.clone()));
    let entry = multi_era_block_to_chain_entry(&me).expect("shelley should produce entry");
    assert_eq!(entry.block_no, BlockNo(42));
    assert_eq!(entry.slot, SlotNo(100));
    assert_eq!(entry.hash, block.header_hash());
}

#[test]
fn chain_entry_from_byron_returns_some() {
    let me = MultiEraBlock::Byron {
        block: ByronBlock::MainBlock {
            epoch: 1,
            slot_in_epoch: 5,
            chain_difficulty: 42,
            prev_hash: [0x11; 32],
            issuer_vkey: [0u8; 32],
            raw_header: vec![0xCC],
            transactions: vec![],
        },
        era_tag: 1,
    };
    let entry = multi_era_block_to_chain_entry(&me).expect("Byron should return Some");
    assert_eq!(entry.block_no, BlockNo(42));
    // epoch=1, slot_in_epoch=5, slots_per_epoch=21600 → slot = 21605
    assert_eq!(entry.slot, SlotNo(21605));
}

#[test]
fn track_chain_state_roll_forward() {
    let mut cs = ChainState::new(SecurityParam(3));
    let blocks: Vec<MultiEraBlock> = (1..=5)
        .map(|i| MultiEraBlock::Shelley(Box::new(make_shelley_block_with_number(i, i * 10))))
        .collect();

    let step = MultiEraSyncStep::RollForward {
        raw_header: vec![],
        tip: Point::Origin,
        blocks,
        raw_blocks: Vec::new(),
        block_spans: Vec::new(),
    };
    let stable = track_chain_state(&mut cs, &step).expect("roll forward");
    // 5 blocks, k=3 → 2 stable
    assert_eq!(stable, 2);
    assert_eq!(cs.volatile_len(), 3);
}

#[test]
fn track_chain_state_entries_returns_stable_prefix() {
    let mut cs = ChainState::new(SecurityParam(3));
    let blocks: Vec<MultiEraBlock> = (1..=5)
        .map(|i| MultiEraBlock::Shelley(Box::new(make_shelley_block_with_number(i, i * 10))))
        .collect();

    let step = MultiEraSyncStep::RollForward {
        raw_header: vec![],
        tip: Point::Origin,
        blocks,
        raw_blocks: Vec::new(),
        block_spans: Vec::new(),
    };
    let stable_entries = track_chain_state_entries(&mut cs, &step).expect("roll forward entries");
    assert_eq!(stable_entries.len(), 2);
    assert_eq!(stable_entries[0].block_no, BlockNo(1));
    assert_eq!(stable_entries[1].block_no, BlockNo(2));
    assert_eq!(cs.volatile_len(), 3);
}

#[test]
fn track_chain_state_roll_backward() {
    let mut cs = ChainState::new(SecurityParam(10));
    // Insert blocks 1..=4
    let blocks: Vec<MultiEraBlock> = (1..=4)
        .map(|i| MultiEraBlock::Shelley(Box::new(make_shelley_block_with_number(i, i * 10))))
        .collect();
    let fwd = MultiEraSyncStep::RollForward {
        raw_header: vec![],
        tip: Point::Origin,
        blocks,
        raw_blocks: Vec::new(),
        block_spans: Vec::new(),
    };
    track_chain_state(&mut cs, &fwd).expect("roll forward");
    assert_eq!(cs.volatile_len(), 4);

    // Remember block 2's hash for rollback target.
    let block2 = make_shelley_block_with_number(2, 20);
    let hash2 = block2.header_hash();

    let back = MultiEraSyncStep::RollBackward {
        point: Point::BlockPoint(SlotNo(20), hash2),
        tip: Point::Origin,
    };
    let stable = track_chain_state(&mut cs, &back).expect("roll backward");
    assert_eq!(stable, 0);
    assert_eq!(cs.volatile_len(), 2);
}

#[test]
fn promote_stable_blocks_moves_to_immutable() {
    // Build 5 blocks and add to volatile store.
    let blocks: Vec<_> = (1..=5)
        .map(|i| {
            let sb = make_shelley_block_with_number(i, i * 10);
            multi_era_block_to_block(&MultiEraBlock::Shelley(Box::new(sb)), &[])
        })
        .collect();

    let mut volatile = InMemoryVolatile::default();
    for b in &blocks {
        volatile.add_block(b.clone()).expect("add block");
    }

    // Build stable entries for the first 2 blocks.
    let stable_entries: Vec<_> = blocks[..2]
        .iter()
        .map(|b| yggdrasil_consensus::ChainEntry {
            hash: b.header.hash,
            slot: b.header.slot_no,
            block_no: b.header.block_no,
            prev_hash: None,
        })
        .collect();

    let mut immutable = InMemoryImmutable::default();
    let promoted =
        promote_stable_blocks(&stable_entries, &volatile, &mut immutable).expect("promote");
    assert_eq!(promoted, 2);
    assert_eq!(immutable.len(), 2);
    // The promoted blocks are accessible in the immutable store.
    assert!(immutable.get_block(&blocks[0].header.hash).is_some());
    assert!(immutable.get_block(&blocks[1].header.hash).is_some());
}

#[test]
fn chaindb_promote_volatile_prefix_moves_to_immutable_and_prunes_volatile() {
    let blocks: Vec<_> = (1..=5)
        .map(|i| {
            let sb = make_shelley_block_with_number(i, i * 10);
            multi_era_block_to_block(&MultiEraBlock::Shelley(Box::new(sb)), &[])
        })
        .collect();

    let mut chain_db = ChainDb::new(
        InMemoryImmutable::default(),
        InMemoryVolatile::default(),
        InMemoryLedgerStore::default(),
    );
    for block in &blocks {
        chain_db
            .add_volatile_block(block.clone())
            .expect("add block to volatile");
    }

    let stable_entries: Vec<_> = blocks[..2]
        .iter()
        .map(|b| yggdrasil_consensus::ChainEntry {
            hash: b.header.hash,
            slot: b.header.slot_no,
            block_no: b.header.block_no,
            prev_hash: None,
        })
        .collect();

    let point = Point::BlockPoint(stable_entries[1].slot, stable_entries[1].hash);
    let promoted = chain_db
        .promote_volatile_prefix(&point)
        .expect("promote via chaindb");
    assert_eq!(promoted, 2);
    assert_eq!(chain_db.immutable().len(), 2);
    assert!(
        chain_db
            .immutable()
            .get_block(&blocks[0].header.hash)
            .is_some()
    );
    assert!(
        chain_db
            .immutable()
            .get_block(&blocks[1].header.hash)
            .is_some()
    );
    assert!(
        chain_db
            .volatile()
            .get_block(&blocks[0].header.hash)
            .is_none()
    );
    assert!(
        chain_db
            .volatile()
            .get_block(&blocks[1].header.hash)
            .is_none()
    );
    assert!(
        chain_db
            .volatile()
            .get_block(&blocks[2].header.hash)
            .is_some()
    );
}

#[test]
fn recover_ledger_state_chaindb_restores_checkpoint_and_replays_volatile_suffix() {
    let immutable = InMemoryImmutable::default();
    let volatile = InMemoryVolatile::default();
    let ledger = InMemoryLedgerStore::default();
    let mut chain_db = ChainDb::new(immutable, volatile, ledger);

    let mut checkpoint_state = LedgerState::new(Era::Shelley);
    checkpoint_state.tip = Point::BlockPoint(SlotNo(10), HeaderHash([0x0A; 32]));

    chain_db
        .save_ledger_checkpoint(SlotNo(10), &checkpoint_state.checkpoint())
        .expect("save checkpoint");
    chain_db
        .add_volatile_block(test_store_block(0x14, 20))
        .expect("add volatile 20");
    chain_db
        .add_volatile_block(test_store_block(0x1E, 30))
        .expect("add volatile 30");

    let recovered = recover_ledger_state_chaindb(&chain_db, LedgerState::new(Era::Shelley))
        .expect("recover ledger state from chaindb");

    assert_eq!(recovered.checkpoint_slot, Some(SlotNo(10)));
    assert_eq!(recovered.replayed_volatile_blocks, 2);
    assert_eq!(
        recovered.point,
        Point::BlockPoint(SlotNo(30), HeaderHash([0x1E; 32]))
    );
    assert_eq!(
        recovered.ledger_state.tip,
        Point::BlockPoint(SlotNo(30), HeaderHash([0x1E; 32]))
    );
}

#[test]
fn recover_ledger_state_chaindb_replays_immutable_blocks_after_checkpoint() {
    let immutable = InMemoryImmutable::default();
    let volatile = InMemoryVolatile::default();
    let ledger = InMemoryLedgerStore::default();
    let mut chain_db = ChainDb::new(immutable, volatile, ledger);

    let mut checkpoint_state = LedgerState::new(Era::Shelley);
    checkpoint_state.tip = Point::BlockPoint(SlotNo(10), HeaderHash([0x0A; 32]));

    chain_db
        .save_ledger_checkpoint(SlotNo(10), &checkpoint_state.checkpoint())
        .expect("save checkpoint");
    chain_db
        .immutable_mut()
        .append_block(test_store_block(0x14, 20))
        .expect("append immutable 20");
    chain_db
        .immutable_mut()
        .append_block(test_store_block(0x1E, 30))
        .expect("append immutable 30");

    let recovered = recover_ledger_state_chaindb(&chain_db, LedgerState::new(Era::Shelley))
        .expect("recover ledger state across immutable replay");

    assert_eq!(recovered.checkpoint_slot, Some(SlotNo(10)));
    assert_eq!(recovered.replayed_volatile_blocks, 0);
    assert_eq!(
        recovered.point,
        Point::BlockPoint(SlotNo(30), HeaderHash([0x1E; 32]))
    );
}

#[test]
fn recover_ledger_state_chaindb_bootstraps_initial_funds_on_first_shelley_block() {
    let immutable = InMemoryImmutable::default();
    let volatile = InMemoryVolatile::default();
    let ledger = InMemoryLedgerStore::default();
    let mut chain_db = ChainDb::new(immutable, volatile, ledger);

    let address = test_shelley_initial_funds_address(0x44);
    let txin = yggdrasil_node::genesis::initial_funds_pseudo_txin(&address);
    let txout = yggdrasil_ledger::ShelleyTxOut {
        address: address.clone(),
        amount: 1_000,
    };

    let mut base_state = LedgerState::new(Era::Byron);
    base_state.configure_pending_shelley_genesis_utxo(vec![(txin.clone(), txout.clone())]);

    chain_db
        .immutable_mut()
        .append_block(multi_era_block_to_block(
            &MultiEraBlock::Byron {
                block: ByronBlock::MainBlock {
                    epoch: 0,
                    slot_in_epoch: 1,
                    chain_difficulty: 1,
                    prev_hash: [0; 32],
                    issuer_vkey: [0u8; 32],
                    raw_header: vec![0xAA],
                    transactions: vec![],
                },
                era_tag: 1,
            },
            &[],
        ))
        .expect("append byron block");
    chain_db
        .immutable_mut()
        .append_block(multi_era_block_to_block(
            &MultiEraBlock::Shelley(Box::new(make_shelley_block_with_number(2, 10))),
            &[],
        ))
        .expect("append shelley block");

    let recovered = recover_ledger_state_chaindb(&chain_db, base_state)
        .expect("recover ledger state with Shelley bootstrap");

    assert_eq!(recovered.ledger_state.current_era(), Era::Shelley);
    assert_eq!(recovered.ledger_state.utxo().get(&txin), Some(&txout));
    assert!(recovered.ledger_state.multi_era_utxo().get(&txin).is_some());
}

#[test]
fn recover_ledger_state_chaindb_keeps_initial_funds_hidden_before_shelley() {
    let immutable = InMemoryImmutable::default();
    let volatile = InMemoryVolatile::default();
    let ledger = InMemoryLedgerStore::default();
    let mut chain_db = ChainDb::new(immutable, volatile, ledger);

    let address = test_shelley_initial_funds_address(0x55);
    let txin = yggdrasil_node::genesis::initial_funds_pseudo_txin(&address);

    let mut base_state = LedgerState::new(Era::Byron);
    base_state.configure_pending_shelley_genesis_utxo(vec![(
        txin.clone(),
        yggdrasil_ledger::ShelleyTxOut {
            address,
            amount: 2_000,
        },
    )]);

    chain_db
        .immutable_mut()
        .append_block(multi_era_block_to_block(
            &MultiEraBlock::Byron {
                block: ByronBlock::MainBlock {
                    epoch: 0,
                    slot_in_epoch: 1,
                    chain_difficulty: 1,
                    prev_hash: [0; 32],
                    issuer_vkey: [0u8; 32],
                    raw_header: vec![0xAA],
                    transactions: vec![],
                },
                era_tag: 1,
            },
            &[],
        ))
        .expect("append byron block");

    let recovered = recover_ledger_state_chaindb(&chain_db, base_state)
        .expect("recover ledger state before Shelley");

    assert_eq!(recovered.ledger_state.current_era(), Era::Byron);
    assert!(recovered.ledger_state.utxo().get(&txin).is_none());
    assert!(recovered.ledger_state.multi_era_utxo().get(&txin).is_none());
}

#[test]
fn recover_ledger_state_chaindb_bootstraps_genesis_stake_on_first_shelley_block() {
    let immutable = InMemoryImmutable::default();
    let volatile = InMemoryVolatile::default();
    let ledger = InMemoryLedgerStore::default();
    let mut chain_db = ChainDb::new(immutable, volatile, ledger);

    let credential = StakeCredential::AddrKeyHash([0x66; 28]);
    let pool = [0x77; 28];

    let mut base_state = LedgerState::new(Era::Byron);
    base_state.configure_pending_shelley_genesis_stake(vec![(credential, pool)]);

    chain_db
        .immutable_mut()
        .append_block(multi_era_block_to_block(
            &MultiEraBlock::Byron {
                block: ByronBlock::MainBlock {
                    epoch: 0,
                    slot_in_epoch: 1,
                    chain_difficulty: 1,
                    prev_hash: [0; 32],
                    issuer_vkey: [0u8; 32],
                    raw_header: vec![0xAA],
                    transactions: vec![],
                },
                era_tag: 1,
            },
            &[],
        ))
        .expect("append byron block");
    chain_db
        .immutable_mut()
        .append_block(multi_era_block_to_block(
            &MultiEraBlock::Shelley(Box::new(make_shelley_block_with_number(2, 10))),
            &[],
        ))
        .expect("append shelley block");

    let recovered = recover_ledger_state_chaindb(&chain_db, base_state)
        .expect("recover ledger state with Shelley stake bootstrap");

    assert_eq!(recovered.ledger_state.current_era(), Era::Shelley);
    let registered = recovered
        .ledger_state
        .stake_credential_state(&credential)
        .expect("stake credential should be bootstrapped");
    assert_eq!(registered.delegated_pool(), Some(pool));
}

#[test]
fn track_chain_state_includes_byron_blocks() {
    let mut cs = ChainState::new(SecurityParam(10));
    let blocks = vec![
        MultiEraBlock::Byron {
            block: ByronBlock::MainBlock {
                epoch: 0,
                slot_in_epoch: 1,
                chain_difficulty: 1,
                prev_hash: [0; 32],
                issuer_vkey: [0u8; 32],
                raw_header: vec![0xDD],
                transactions: vec![],
            },
            era_tag: 1,
        },
        MultiEraBlock::Shelley(Box::new(make_shelley_block_with_number(2, 10))),
    ];
    let step = MultiEraSyncStep::RollForward {
        raw_header: vec![],
        tip: Point::Origin,
        blocks,
        raw_blocks: Vec::new(),
        block_spans: Vec::new(),
    };
    let stable = track_chain_state(&mut cs, &step).expect("roll forward with byron");
    assert_eq!(stable, 0);
    // Both the Byron and the Shelley block were tracked.
    assert_eq!(cs.volatile_len(), 2);
}

// ---------------------------------------------------------------------------
// collect_rolled_back_tx_ids
// ---------------------------------------------------------------------------

#[test]
fn collect_rolled_back_tx_ids_returns_txs_after_target() {
    let mut store = InMemoryVolatile::default();

    // Block 1: no transactions (the rollback target)
    store.add_block(test_store_block(0x01, 10)).unwrap();

    // Block 2: 2 transactions
    let tx_a = TxId([0xA0; 32]);
    let tx_b = TxId([0xB0; 32]);
    let mut blk2 = test_store_block(0x02, 11);
    blk2.transactions = vec![
        Tx {
            id: tx_a,
            body: vec![],
            witnesses: None,
            auxiliary_data: None,
            is_valid: None,
        },
        Tx {
            id: tx_b,
            body: vec![],
            witnesses: None,
            auxiliary_data: None,
            is_valid: None,
        },
    ];
    store.add_block(blk2).unwrap();

    // Block 3: 1 transaction
    let tx_c = TxId([0xC0; 32]);
    let mut blk3 = test_store_block(0x03, 12);
    blk3.transactions = vec![Tx {
        id: tx_c,
        body: vec![],
        witnesses: None,
        auxiliary_data: None,
        is_valid: None,
    }];
    store.add_block(blk3).unwrap();

    // Rolling back to block 1 should yield tx_a, tx_b, tx_c.
    let target = Point::BlockPoint(SlotNo(10), HeaderHash([0x01; 32]));
    let ids = collect_rolled_back_tx_ids(&store, &target);
    assert_eq!(ids, vec![tx_a, tx_b, tx_c]);
}

#[test]
fn collect_rolled_back_tx_ids_empty_when_at_tip() {
    let mut store = InMemoryVolatile::default();
    store.add_block(test_store_block(0x01, 10)).unwrap();

    let target = Point::BlockPoint(SlotNo(10), HeaderHash([0x01; 32]));
    let ids = collect_rolled_back_tx_ids(&store, &target);
    assert!(ids.is_empty());
}

#[test]
fn collect_rolled_back_tx_ids_origin_returns_all() {
    let mut store = InMemoryVolatile::default();

    let tx_a = TxId([0xAA; 32]);
    let mut blk1 = test_store_block(0x01, 10);
    blk1.transactions = vec![Tx {
        id: tx_a,
        body: vec![],
        witnesses: None,
        auxiliary_data: None,
        is_valid: None,
    }];
    store.add_block(blk1).unwrap();

    let ids = collect_rolled_back_tx_ids(&store, &Point::Origin);
    assert_eq!(ids, vec![tx_a]);
}

// ---------------------------------------------------------------------------
// Rollback + mempool integration
// ---------------------------------------------------------------------------

/// After a rollback, the tx IDs collected from discarded blocks should not
/// match anything the mempool considers confirmed (since they are now
/// *un-confirmed*). This validates the dual flow: evict_confirmed_from_mempool
/// removes confirmed txs, while collect_rolled_back_tx_ids identifies txs
/// that should be re-admitted.
#[test]
fn rollback_collected_tx_ids_not_evicted_from_mempool() {
    let mut store = InMemoryVolatile::default();
    let mut mempool = Mempool::with_capacity(1_000_000);

    // Build two blocks with transactions.
    let tx_a = TxId([0xA0; 32]);
    let tx_b = TxId([0xB0; 32]);
    let mut blk1 = test_store_block(0x01, 10);
    blk1.transactions = vec![Tx {
        id: tx_a,
        body: vec![],
        witnesses: None,
        auxiliary_data: None,
        is_valid: None,
    }];
    store.add_block(blk1).unwrap();

    let mut blk2 = test_store_block(0x02, 20);
    blk2.transactions = vec![Tx {
        id: tx_b,
        body: vec![],
        witnesses: None,
        auxiliary_data: None,
        is_valid: None,
    }];
    store.add_block(blk2).unwrap();

    // Simulate: tx_a was in our mempool before being confirmed.
    let entry_a = MempoolEntry {
        era: Era::Shelley,
        tx_id: tx_a,
        fee: 200_000,
        body: vec![0xCA, 0xFE],
        raw_tx: vec![0xDE, 0xAD],
        size_bytes: 256,
        ttl: SlotNo(100),
        inputs: vec![],
    };
    mempool.insert(entry_a).unwrap();

    // Before rollback: evict_confirmed_from_mempool removes tx_a
    // (it is confirmed in block 1).
    let blocks: Vec<_> = store
        .prefix_up_to(&Point::BlockPoint(SlotNo(20), HeaderHash([0x02; 32])))
        .unwrap();
    let confirmed_ids: Vec<TxId> = blocks
        .iter()
        .flat_map(|b| b.transactions.iter().map(|tx| tx.id))
        .collect();
    let evicted = mempool.remove_confirmed(&confirmed_ids);
    assert_eq!(evicted, 1, "tx_a should be evicted as confirmed");

    // Now simulate a rollback to origin, collecting rolled-back tx IDs.
    let rolled_back = collect_rolled_back_tx_ids(
        &store,
        &Point::BlockPoint(SlotNo(10), HeaderHash([0x01; 32])),
    );
    assert_eq!(rolled_back.len(), 1, "block 2's tx_b should appear");
    assert_eq!(rolled_back[0], tx_b);

    // The mempool shouldn't contain tx_b (it was never submitted there),
    // but if it were, it should NOT be evicted since the block was rolled back.
    assert!(!mempool.contains(&tx_b));

    // Apply the rollback.
    store.rollback_to(&Point::BlockPoint(SlotNo(10), HeaderHash([0x01; 32])));
    assert_eq!(
        store.tip(),
        Point::BlockPoint(SlotNo(10), HeaderHash([0x01; 32]))
    );
}

/// Verifies that apply_multi_era_step_to_volatile handles rollback steps
/// by truncating the volatile store and that transactions from the removed
/// blocks can be identified beforehand via collect_rolled_back_tx_ids.
#[test]
fn apply_rollback_step_discards_volatile_suffix() {
    let mut store = InMemoryVolatile::default();

    let tx_a = TxId([0xAA; 32]);
    let mut blk1 = test_store_block(0x01, 10);
    blk1.transactions = vec![Tx {
        id: tx_a,
        body: vec![],
        witnesses: None,
        auxiliary_data: None,
        is_valid: None,
    }];
    store.add_block(blk1).unwrap();

    let tx_b = TxId([0xBB; 32]);
    let mut blk2 = test_store_block(0x02, 20);
    blk2.transactions = vec![Tx {
        id: tx_b,
        body: vec![],
        witnesses: None,
        auxiliary_data: None,
        is_valid: None,
    }];
    store.add_block(blk2).unwrap();

    let target = Point::BlockPoint(SlotNo(10), HeaderHash([0x01; 32]));

    // Collect tx IDs before applying the rollback step.
    let rolled_back = collect_rolled_back_tx_ids(&store, &target);
    assert_eq!(rolled_back, vec![tx_b]);

    // Apply the rollback step.
    let step = MultiEraSyncStep::RollBackward {
        point: target,
        tip: Point::BlockPoint(SlotNo(10), HeaderHash([0x01; 32])),
    };
    apply_multi_era_step_to_volatile(&mut store, &step).unwrap();

    // Store is now truncated.
    assert_eq!(
        store.tip(),
        Point::BlockPoint(SlotNo(10), HeaderHash([0x01; 32]))
    );
    assert!(store.get_block(&HeaderHash([0x02; 32])).is_none());
}

/// After promotion and rollback, the immutable portion is preserved and
/// only volatile blocks are discarded. Collected tx IDs should come only
/// from the discarded volatile suffix.
#[test]
fn promote_then_rollback_collects_only_volatile_tx_ids() {
    let mut chain_db = ChainDb::new(
        InMemoryImmutable::default(),
        InMemoryVolatile::default(),
        InMemoryLedgerStore::default(),
    );

    let tx_imm = TxId([0x11; 32]);
    let mut blk1 = test_store_block(0x01, 10);
    blk1.transactions = vec![Tx {
        id: tx_imm,
        body: vec![],
        witnesses: None,
        auxiliary_data: None,
        is_valid: None,
    }];
    chain_db.add_volatile_block(blk1).unwrap();

    let tx_vol_a = TxId([0x22; 32]);
    let mut blk2 = test_store_block(0x02, 20);
    blk2.transactions = vec![Tx {
        id: tx_vol_a,
        body: vec![],
        witnesses: None,
        auxiliary_data: None,
        is_valid: None,
    }];
    chain_db.add_volatile_block(blk2).unwrap();

    let tx_vol_b = TxId([0x33; 32]);
    let mut blk3 = test_store_block(0x03, 30);
    blk3.transactions = vec![Tx {
        id: tx_vol_b,
        body: vec![],
        witnesses: None,
        auxiliary_data: None,
        is_valid: None,
    }];
    chain_db.add_volatile_block(blk3).unwrap();

    // Promote block 1 to immutable.
    chain_db
        .promote_volatile_prefix(&Point::BlockPoint(SlotNo(10), HeaderHash([0x01; 32])))
        .unwrap();
    assert_eq!(chain_db.immutable().len(), 1);

    // Collect tx IDs that would be rolled back if we roll to block 2.
    let target = Point::BlockPoint(SlotNo(20), HeaderHash([0x02; 32]));
    let rolled_back = collect_rolled_back_tx_ids(chain_db.volatile(), &target);
    // Only block 3's tx (tx_vol_b) is in the volatile suffix after the target.
    assert_eq!(rolled_back, vec![tx_vol_b]);

    // Apply the rollback.
    chain_db.volatile_mut().rollback_to(&target);
    assert_eq!(
        chain_db.volatile().tip(),
        Point::BlockPoint(SlotNo(20), HeaderHash([0x02; 32]))
    );
    // Immutable block is preserved.
    assert!(
        chain_db
            .immutable()
            .get_block(&HeaderHash([0x01; 32]))
            .is_some()
    );
}

// ---------------------------------------------------------------------------
// total_transaction_fees
// ---------------------------------------------------------------------------

#[test]
fn total_transaction_fees_shelley_sums_all_txs() {
    let block = MultiEraBlock::Shelley(Box::new(ShelleyBlock {
        header: ShelleyHeader {
            body: sample_header_body(),
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![
            make_tx_body(100, 999),
            make_tx_body(250, 999),
            make_tx_body(50, 999),
        ],
        transaction_witness_sets: vec![],
        transaction_metadata_set: std::collections::HashMap::new(),
    }));
    assert_eq!(block.total_transaction_fees(), 400);
}

#[test]
fn total_transaction_fees_shelley_empty_block() {
    let block = MultiEraBlock::Shelley(Box::new(ShelleyBlock {
        header: ShelleyHeader {
            body: sample_header_body(),
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        transaction_metadata_set: std::collections::HashMap::new(),
    }));
    assert_eq!(block.total_transaction_fees(), 0);
}

#[test]
fn total_transaction_fees_byron_always_zero() {
    let block = MultiEraBlock::Byron {
        block: ByronBlock::EpochBoundary {
            epoch: 0,
            chain_difficulty: 0,
            prev_hash: [0; 32],
            raw_header: vec![],
        },
        era_tag: 0,
    };
    assert_eq!(block.total_transaction_fees(), 0);
}

// ===========================================================================
// OpCert counter validation tests
// ===========================================================================

#[test]
fn validate_block_opcert_counter_skips_byron() {
    use yggdrasil_consensus::OcertCounters;
    use yggdrasil_ledger::PoolStakeDistribution;
    use yggdrasil_node::validate_block_opcert_counter;

    let block = MultiEraBlock::Byron {
        block: ByronBlock::EpochBoundary {
            epoch: 0,
            chain_difficulty: 0,
            prev_hash: [0; 32],
            raw_header: vec![],
        },
        era_tag: 0,
    };
    let dist = PoolStakeDistribution::from_raw(Default::default(), 0);
    let mut counters = OcertCounters::new();
    assert!(validate_block_opcert_counter(&block, &mut counters, &dist).is_ok());
    assert!(counters.is_empty());
}

#[test]
fn validate_block_opcert_counter_accepts_new_pool_in_dist() {
    use std::collections::BTreeMap;
    use yggdrasil_consensus::OcertCounters;
    use yggdrasil_ledger::PoolStakeDistribution;
    use yggdrasil_node::{block_issuer_vkey, validate_block_opcert_counter};

    let block = MultiEraBlock::Shelley(Box::new(ShelleyBlock {
        header: ShelleyHeader {
            body: sample_header_body(),
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        transaction_metadata_set: std::collections::HashMap::new(),
    }));

    let issuer_vkey = block_issuer_vkey(&block).unwrap();
    let pool_hash = yggdrasil_crypto::blake2b::hash_bytes_224(&issuer_vkey).0;

    let mut pool_stakes = BTreeMap::new();
    pool_stakes.insert(pool_hash, 1_000_000);
    let dist = PoolStakeDistribution::from_raw(pool_stakes, 1_000_000);

    let mut counters = OcertCounters::new();
    assert!(validate_block_opcert_counter(&block, &mut counters, &dist).is_ok());
    assert_eq!(counters.get(&pool_hash), Some(42)); // sequence_number from sample_opcert is 42
}

#[test]
fn validate_block_opcert_counter_rejects_unknown_pool() {
    use yggdrasil_consensus::OcertCounters;
    use yggdrasil_ledger::PoolStakeDistribution;
    use yggdrasil_node::validate_block_opcert_counter;

    let block = MultiEraBlock::Shelley(Box::new(ShelleyBlock {
        header: ShelleyHeader {
            body: sample_header_body(),
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        transaction_metadata_set: std::collections::HashMap::new(),
    }));

    // Empty stake distribution — pool not known.
    let dist = PoolStakeDistribution::from_raw(Default::default(), 0);
    let mut counters = OcertCounters::new();
    let result = validate_block_opcert_counter(&block, &mut counters, &dist);
    assert!(result.is_err());
}

// ===========================================================================
// VRF key cross-check tests
// ===========================================================================

#[test]
fn block_vrf_vkey_extracts_shelley() {
    use yggdrasil_node::block_vrf_vkey;

    let block = MultiEraBlock::Shelley(Box::new(ShelleyBlock {
        header: ShelleyHeader {
            body: sample_header_body(),
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        transaction_metadata_set: std::collections::HashMap::new(),
    }));
    let vrf_vkey = block_vrf_vkey(&block).unwrap();
    assert_eq!(vrf_vkey, sample_header_body().vrf_vkey);
}

#[test]
fn block_vrf_vkey_returns_none_for_byron() {
    use yggdrasil_node::block_vrf_vkey;

    let block = MultiEraBlock::Byron {
        block: ByronBlock::EpochBoundary {
            epoch: 0,
            chain_difficulty: 0,
            prev_hash: [0; 32],
            raw_header: vec![],
        },
        era_tag: 0,
    };
    assert!(block_vrf_vkey(&block).is_none());
}

// ---------------------------------------------------------------------------
// Protocol version validation
// ---------------------------------------------------------------------------

#[test]
fn validate_protocol_version_shelley_accepts_major_2() {
    let block = MultiEraBlock::Shelley(Box::new(ShelleyBlock {
        header: ShelleyHeader {
            body: ShelleyHeaderBody {
                protocol_version: (2, 0),
                ..sample_header_body()
            },
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        transaction_metadata_set: std::collections::HashMap::new(),
    }));
    assert!(validate_block_protocol_version(&block).is_ok());
}

#[test]
fn validate_protocol_version_allegra_accepts_major_3() {
    // Allegra is still MultiEraBlock::Shelley but protocol_version major=3.
    let block = MultiEraBlock::Shelley(Box::new(ShelleyBlock {
        header: ShelleyHeader {
            body: ShelleyHeaderBody {
                protocol_version: (3, 0),
                ..sample_header_body()
            },
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        transaction_metadata_set: std::collections::HashMap::new(),
    }));
    assert!(validate_block_protocol_version(&block).is_ok());
}

#[test]
fn validate_protocol_version_mary_accepts_major_4() {
    let block = MultiEraBlock::Shelley(Box::new(ShelleyBlock {
        header: ShelleyHeader {
            body: ShelleyHeaderBody {
                protocol_version: (4, 0),
                ..sample_header_body()
            },
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        transaction_metadata_set: std::collections::HashMap::new(),
    }));
    assert!(validate_block_protocol_version(&block).is_ok());
}

#[test]
fn validate_protocol_version_alonzo_accepts_major_5_and_6() {
    for major in [5u64, 6] {
        let block = MultiEraBlock::Alonzo(Box::new(AlonzoBlock {
            header: ShelleyHeader {
                body: ShelleyHeaderBody {
                    protocol_version: (major, 0),
                    ..sample_header_body()
                },
                signature: vec![0xDD; 448],
            },
            transaction_bodies: vec![],
            transaction_witness_sets: vec![],
            auxiliary_data_set: std::collections::HashMap::new(),
            invalid_transactions: vec![],
        }));
        assert!(
            validate_block_protocol_version(&block).is_ok(),
            "Alonzo should accept major={major}"
        );
    }
}

#[test]
fn validate_protocol_version_babbage_accepts_major_7_and_8() {
    for major in [7u64, 8] {
        let block = MultiEraBlock::Babbage(Box::new(BabbageBlock {
            header: PraosHeader {
                body: PraosHeaderBody {
                    protocol_version: (major, 0),
                    ..sample_praos_header_body()
                },
                signature: vec![0xDD; 448],
            },
            transaction_bodies: vec![],
            transaction_witness_sets: vec![],
            auxiliary_data_set: std::collections::HashMap::new(),
            invalid_transactions: vec![],
        }));
        assert!(
            validate_block_protocol_version(&block).is_ok(),
            "Babbage should accept major={major}"
        );
    }
}

#[test]
fn validate_protocol_version_conway_accepts_major_9_and_10() {
    for major in [9u64, 10] {
        let block = MultiEraBlock::Conway(Box::new(ConwayBlock {
            header: PraosHeader {
                body: PraosHeaderBody {
                    protocol_version: (major, 0),
                    ..sample_praos_header_body()
                },
                signature: vec![0xDD; 448],
            },
            transaction_bodies: vec![],
            transaction_witness_sets: vec![],
            auxiliary_data_set: std::collections::HashMap::new(),
            invalid_transactions: vec![],
        }));
        assert!(
            validate_block_protocol_version(&block).is_ok(),
            "Conway should accept major={major}"
        );
    }
}

#[test]
fn validate_protocol_version_rejects_wrong_shelley_major() {
    // A Shelley-era block (protocol_version major=2) carrying major=7
    // should be rejected.
    let block = MultiEraBlock::Shelley(Box::new(ShelleyBlock {
        header: ShelleyHeader {
            body: ShelleyHeaderBody {
                protocol_version: (7, 0),
                ..sample_header_body()
            },
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        transaction_metadata_set: std::collections::HashMap::new(),
    }));
    let err = validate_block_protocol_version(&block).unwrap_err();
    assert!(
        format!("{err}").contains("protocol version mismatch"),
        "expected protocol-version mismatch error, got: {err}"
    );
}

#[test]
fn validate_protocol_version_rejects_babbage_with_major_2() {
    let block = MultiEraBlock::Babbage(Box::new(BabbageBlock {
        header: PraosHeader {
            body: PraosHeaderBody {
                protocol_version: (2, 0),
                ..sample_praos_header_body()
            },
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        auxiliary_data_set: std::collections::HashMap::new(),
        invalid_transactions: vec![],
    }));
    let err = validate_block_protocol_version(&block).unwrap_err();
    assert!(
        format!("{err}").contains("protocol version mismatch"),
        "expected protocol-version mismatch error, got: {err}"
    );
}

#[test]
fn validate_protocol_version_skips_byron() {
    let block = MultiEraBlock::Byron {
        block: ByronBlock::EpochBoundary {
            epoch: 0,
            chain_difficulty: 0,
            prev_hash: [0; 32],
            raw_header: vec![],
        },
        era_tag: 0,
    };
    assert!(validate_block_protocol_version(&block).is_ok());
}

// ---------------------------------------------------------------------------
// Block body size validation
// ---------------------------------------------------------------------------

#[test]
fn validate_body_size_accepts_matching_shelley_block() {
    // Build a Shelley block, serialize it, then compute the actual body size
    // and set the header's block_body_size to match.
    let inner_block = ShelleyBlock {
        header: ShelleyHeader {
            body: ShelleyHeaderBody {
                block_body_size: 0, // placeholder
                ..sample_header_body()
            },
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        transaction_metadata_set: std::collections::HashMap::new(),
    };

    // Serialize to get the raw block CBOR.
    let raw = inner_block.to_cbor_bytes();
    // Compute actual body size by skipping header in the CBOR array.
    let mut dec = yggdrasil_ledger::cbor::Decoder::new(&raw);
    let _arr_len = dec.array().unwrap();
    dec.skip().unwrap(); // skip header
    let body_start = dec.position();
    let actual_body_size = (raw.len() - body_start) as u32;

    // Re-build with the correct body size.
    let block = ShelleyBlock {
        header: ShelleyHeader {
            body: ShelleyHeaderBody {
                block_body_size: actual_body_size,
                ..sample_header_body()
            },
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        transaction_metadata_set: std::collections::HashMap::new(),
    };
    let raw = block.to_cbor_bytes();
    let me = MultiEraBlock::Shelley(Box::new(block));

    assert!(validate_block_body_size(&me, &raw).is_ok());
}

#[test]
fn validate_body_size_rejects_wrong_size() {
    // Build a block with a deliberately wrong block_body_size.
    let block = ShelleyBlock {
        header: ShelleyHeader {
            body: ShelleyHeaderBody {
                block_body_size: 99999,
                ..sample_header_body()
            },
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        transaction_metadata_set: std::collections::HashMap::new(),
    };
    let raw = block.to_cbor_bytes();
    let me = MultiEraBlock::Shelley(Box::new(block));

    let err = validate_block_body_size(&me, &raw).unwrap_err();
    assert!(
        format!("{err}").contains("wrong block body size"),
        "expected body size mismatch error, got: {err}"
    );
}

#[test]
fn validate_body_size_skips_byron() {
    let block = MultiEraBlock::Byron {
        block: ByronBlock::EpochBoundary {
            epoch: 0,
            chain_difficulty: 0,
            prev_hash: [0; 32],
            raw_header: vec![],
        },
        era_tag: 0,
    };
    // Any raw bytes are fine — Byron should be skipped.
    assert!(validate_block_body_size(&block, &[]).is_ok());
}

// ---------------------------------------------------------------------------
// MultiEraBlock::era() disambiguation
// ---------------------------------------------------------------------------

#[test]
fn multi_era_block_era_shelley() {
    let block = MultiEraBlock::Shelley(Box::new(ShelleyBlock {
        header: ShelleyHeader {
            body: ShelleyHeaderBody {
                protocol_version: (2, 0),
                ..sample_header_body()
            },
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        transaction_metadata_set: std::collections::HashMap::new(),
    }));
    assert_eq!(block.era(), Era::Shelley);
}

#[test]
fn multi_era_block_era_allegra() {
    let block = MultiEraBlock::Shelley(Box::new(ShelleyBlock {
        header: ShelleyHeader {
            body: ShelleyHeaderBody {
                protocol_version: (3, 0),
                ..sample_header_body()
            },
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        transaction_metadata_set: std::collections::HashMap::new(),
    }));
    assert_eq!(block.era(), Era::Allegra);
}

#[test]
fn multi_era_block_era_mary() {
    let block = MultiEraBlock::Shelley(Box::new(ShelleyBlock {
        header: ShelleyHeader {
            body: ShelleyHeaderBody {
                protocol_version: (4, 0),
                ..sample_header_body()
            },
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        transaction_metadata_set: std::collections::HashMap::new(),
    }));
    assert_eq!(block.era(), Era::Mary);
}

#[test]
fn multi_era_block_era_alonzo() {
    let block = MultiEraBlock::Alonzo(Box::new(AlonzoBlock {
        header: ShelleyHeader {
            body: ShelleyHeaderBody {
                protocol_version: (6, 0),
                ..sample_header_body()
            },
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        auxiliary_data_set: std::collections::HashMap::new(),
        invalid_transactions: vec![],
    }));
    assert_eq!(block.era(), Era::Alonzo);
}

#[test]
fn multi_era_block_era_babbage() {
    let block = MultiEraBlock::Babbage(Box::new(BabbageBlock {
        header: PraosHeader {
            body: PraosHeaderBody {
                protocol_version: (7, 0),
                ..sample_praos_header_body()
            },
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        auxiliary_data_set: std::collections::HashMap::new(),
        invalid_transactions: vec![],
    }));
    assert_eq!(block.era(), Era::Babbage);
}

#[test]
fn multi_era_block_era_conway() {
    let block = MultiEraBlock::Conway(Box::new(ConwayBlock {
        header: PraosHeader {
            body: PraosHeaderBody {
                protocol_version: (9, 0),
                ..sample_praos_header_body()
            },
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        auxiliary_data_set: std::collections::HashMap::new(),
        invalid_transactions: vec![],
    }));
    assert_eq!(block.era(), Era::Conway);
}

#[test]
fn multi_era_block_era_byron() {
    let block = MultiEraBlock::Byron {
        block: ByronBlock::EpochBoundary {
            epoch: 0,
            chain_difficulty: 0,
            prev_hash: [0; 32],
            raw_header: vec![],
        },
        era_tag: 0,
    };
    assert_eq!(block.era(), Era::Byron);
}

// ---------------------------------------------------------------------------
// is_peer_attributable for new error variants
// ---------------------------------------------------------------------------

#[test]
fn sync_error_wrong_body_size_is_peer_attributable() {
    use yggdrasil_node::SyncError;
    let err = SyncError::WrongBlockBodySize {
        declared: 100,
        actual: 200,
    };
    assert!(err.is_peer_attributable());
}

#[test]
fn sync_error_protocol_version_mismatch_is_peer_attributable() {
    use yggdrasil_node::SyncError;
    let err = SyncError::ProtocolVersionMismatch {
        era: Era::Shelley,
        major: 7,
        minor: 0,
        expected_range: "2".to_string(),
    };
    assert!(err.is_peer_attributable());
}

// ---------------------------------------------------------------------------
// verify_multi_era_block integrates protocol version check
// ---------------------------------------------------------------------------

#[test]
fn verify_multi_era_block_rejects_bad_protocol_version() {
    // A Babbage block with protocol version 2 should fail.
    let block = MultiEraBlock::Babbage(Box::new(BabbageBlock {
        header: PraosHeader {
            body: PraosHeaderBody {
                protocol_version: (2, 0),
                ..sample_praos_header_body()
            },
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        auxiliary_data_set: std::collections::HashMap::new(),
        invalid_transactions: vec![],
    }));
    let config = VerificationConfig {
        slots_per_kes_period: 129600,
        max_kes_evolutions: 62,
        verify_body_hash: false,
        max_major_protocol_version: None,
        future_check: None,
        ocert_counters: None,
        pp_major_protocol_version: None,
        network_magic: None,
    };
    let err = verify_multi_era_block(&block, &config).unwrap_err();
    assert!(
        format!("{err}").contains("protocol version mismatch"),
        "verify_multi_era_block should catch protocol version error, got: {err}"
    );
}

/// Conway BBODY parity: header major version must be ≤ pp major + 1.
///
/// Reference: `Cardano.Ledger.Conway.Rules.Bbody` — `HeaderProtVerTooHigh`.
#[test]
fn verify_multi_era_block_rejects_header_prot_ver_too_high() {
    // Conway block claiming major 12 when PP says major 10 → rejected.
    let block = MultiEraBlock::Conway(Box::new(ConwayBlock {
        header: PraosHeader {
            body: PraosHeaderBody {
                protocol_version: (12, 0),
                ..sample_praos_header_body()
            },
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        auxiliary_data_set: std::collections::HashMap::new(),
        invalid_transactions: vec![],
    }));
    let config = VerificationConfig {
        slots_per_kes_period: 129600,
        max_kes_evolutions: 62,
        verify_body_hash: false,
        max_major_protocol_version: Some(15), // high ceiling so this check doesn't fire
        future_check: None,
        ocert_counters: None,
        pp_major_protocol_version: Some(10), // PP says major 10 → header 12 > 11
        network_magic: None,
    };
    let err = verify_multi_era_block(&block, &config).unwrap_err();
    assert!(
        format!("{err}").contains("header protocol version too high"),
        "expected HeaderProtVerTooHigh, got: {err}"
    );
}

/// Header major == pp major + 1 should be accepted.
#[test]
fn verify_multi_era_block_accepts_header_prot_ver_at_successor() {
    let block = MultiEraBlock::Conway(Box::new(ConwayBlock {
        header: PraosHeader {
            body: PraosHeaderBody {
                protocol_version: (11, 0),
                ..sample_praos_header_body()
            },
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        auxiliary_data_set: std::collections::HashMap::new(),
        invalid_transactions: vec![],
    }));
    let config = VerificationConfig {
        slots_per_kes_period: 129600,
        max_kes_evolutions: 62,
        verify_body_hash: false,
        max_major_protocol_version: Some(15),
        future_check: None,
        ocert_counters: None,
        pp_major_protocol_version: Some(10), // PP 10, header 11 = 10+1 → ok
        network_magic: None,
    };
    // This should NOT fail on protocol version — it may fail on KES/VRF
    // checks, but not on HeaderProtVerTooHigh.
    let result = verify_multi_era_block(&block, &config);
    if let Err(e) = &result {
        assert!(
            !format!("{e}").contains("header protocol version too high"),
            "header major 11 with PP major 10 should be accepted, got: {e}"
        );
    }
}
