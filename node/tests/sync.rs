use std::net::SocketAddr;

use yggdrasil_network::{
    BlockFetchMessage, ChainSyncMessage, HandshakeVersion, KeepAliveMessage, MiniProtocolNum,
    peer_accept,
};
use yggdrasil_ledger::{
    CborEncode, Encoder, HeaderHash, Point, ShelleyBlock, ShelleyHeader, ShelleyHeaderBody,
    ShelleyOpCert, ShelleyVrfCert, SlotNo,
};
use yggdrasil_node::{
    DecodedSyncStep, MultiEraBlock, NodeConfig, SyncServiceConfig, SyncStep,
    TypedIntersectResult, TypedSyncStep, apply_typed_progress_to_volatile, bootstrap,
    decode_multi_era_block, decode_multi_era_blocks, keepalive_heartbeat, run_sync_service,
    shelley_block_to_block, shelley_header_body_to_consensus, shelley_header_to_consensus,
    shelley_opcert_to_consensus, sync_batch_apply, sync_step, sync_step_decoded, sync_step_typed,
    sync_steps, sync_steps_typed, sync_until_typed, typed_find_intersect, verify_shelley_header,
    SHELLEY_KES_DEPTH,
};
use yggdrasil_storage::{InMemoryVolatile, VolatileStore};

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
                header: b"hdr-1".to_vec(),
                tip: b"tip-1".to_vec(),
            }
            .to_cbor(),
        )
        .await
        .expect("send cs rollforward");

        let bf_req = bf.recv().await.expect("bf recv");
        let bf_msg = BlockFetchMessage::from_cbor(&bf_req).expect("decode bf request");
        match bf_msg {
            BlockFetchMessage::MsgRequestRange(range) => {
                assert_eq!(range.lower, b"origin".to_vec());
                assert_eq!(range.upper, b"tip-1".to_vec());
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
                point: b"rollback-point".to_vec(),
                tip: b"tip-after-rollback".to_vec(),
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
                header: b"hdr-1".to_vec(),
                tip: b"tip-1".to_vec(),
            }
            .to_cbor(),
        )
        .await
        .expect("send cs rollforward 1");

        let bf_req_1 = bf.recv().await.expect("bf recv 1");
        let bf_msg_1 = BlockFetchMessage::from_cbor(&bf_req_1).expect("decode bf request 1");
        match bf_msg_1 {
            BlockFetchMessage::MsgRequestRange(range) => {
                assert_eq!(range.lower, b"origin".to_vec());
                assert_eq!(range.upper, b"tip-1".to_vec());
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
                point: b"tip-0".to_vec(),
                tip: b"tip-2".to_vec(),
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

#[tokio::test]
async fn sync_step_rollforward_fetches_blocks() {
    let magic = 42;
    let addr = spawn_rollforward_responder(magic).await;

    let config = NodeConfig {
        peer_addr: addr,
        network_magic: magic,
        protocol_versions: vec![HandshakeVersion(15)],
    };

    let mut session = bootstrap(&config).await.expect("bootstrap");

    let step = sync_step(
        &mut session.chain_sync,
        &mut session.block_fetch,
        b"origin".to_vec(),
    )
    .await
    .expect("sync step");

    assert_eq!(
        step,
        SyncStep::RollForward {
            header: b"hdr-1".to_vec(),
            tip: b"tip-1".to_vec(),
            blocks: vec![b"block-1".to_vec(), b"block-2".to_vec()],
        }
    );

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
    };

    let mut session = bootstrap(&config).await.expect("bootstrap");

    let step = sync_step(
        &mut session.chain_sync,
        &mut session.block_fetch,
        b"origin".to_vec(),
    )
    .await
    .expect("sync step");

    assert_eq!(
        step,
        SyncStep::RollBackward {
            point: b"rollback-point".to_vec(),
            tip: b"tip-after-rollback".to_vec(),
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
    };

    let mut session = bootstrap(&config).await.expect("bootstrap");

    let progress = sync_steps(
        &mut session.chain_sync,
        &mut session.block_fetch,
        b"origin".to_vec(),
        2,
    )
    .await
    .expect("sync steps");

    assert_eq!(progress.fetched_blocks, 1);
    assert_eq!(progress.current_point, b"tip-0".to_vec());
    assert_eq!(progress.steps.len(), 2);
    assert_eq!(
        progress.steps[0],
        SyncStep::RollForward {
            header: b"hdr-1".to_vec(),
            tip: b"tip-1".to_vec(),
            blocks: vec![b"block-1".to_vec()],
        }
    );
    assert_eq!(
        progress.steps[1],
        SyncStep::RollBackward {
            point: b"tip-0".to_vec(),
            tip: b"tip-2".to_vec(),
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
        body_size: 1024,
        body_hash: [0x55; 32],
        opcert: sample_opcert(0x60),
        protocol_version: (2, 0),
    }
}

fn sample_block_bytes() -> Vec<u8> {
    let block = ShelleyBlock {
        header: ShelleyHeader {
            body: sample_header_body(),
            signature: vec![0xDD; 448],
        },
        transaction_bodies: vec![],
        witness_sets: vec![],
        transaction_metadata: std::collections::HashMap::new(),
    };
    block.to_cbor_bytes()
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
                header: b"hdr-decode".to_vec(),
                tip: b"tip-decode".to_vec(),
            }
            .to_cbor(),
        )
        .await
        .expect("send cs rollforward");

        let bf_req = bf.recv().await.expect("bf recv");
        let bf_msg = BlockFetchMessage::from_cbor(&bf_req).expect("decode bf request");
        match bf_msg {
            BlockFetchMessage::MsgRequestRange(range) => {
                assert_eq!(range.lower, b"origin".to_vec());
                assert_eq!(range.upper, b"tip-decode".to_vec());
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
                assert_eq!(range.lower, Point::Origin.to_cbor_bytes());
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
                assert_eq!(range.lower, Point::Origin.to_cbor_bytes());
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
    };

    let mut session = bootstrap(&config).await.expect("bootstrap");

    let step = sync_step_decoded(
        &mut session.chain_sync,
        &mut session.block_fetch,
        b"origin".to_vec(),
    )
    .await
    .expect("decoded sync step");

    match step {
        DecodedSyncStep::RollForward {
            header,
            tip,
            blocks,
        } => {
            assert_eq!(header, b"hdr-decode".to_vec());
            assert_eq!(tip, b"tip-decode".to_vec());
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
    };

    let mut session = bootstrap(&config).await.expect("bootstrap");

    let err = sync_step_decoded(
        &mut session.chain_sync,
        &mut session.block_fetch,
        b"origin".to_vec(),
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
        tip.to_cbor_bytes(),
        sample_block_bytes(),
    )
    .await;

    let config = NodeConfig {
        peer_addr: addr,
        network_magic: magic,
        protocol_versions: vec![HandshakeVersion(15)],
    };

    let mut session = bootstrap(&config).await.expect("bootstrap");

    let step = sync_step_typed(
        &mut session.chain_sync,
        &mut session.block_fetch,
        Point::Origin.to_cbor_bytes(),
    )
    .await
    .expect("typed sync step");

    match step {
        TypedSyncStep::RollForward {
            header: decoded_header,
            tip: decoded_tip,
            blocks,
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

    let addr =
        spawn_typed_rollback_responder(magic, point.to_cbor_bytes(), tip.to_cbor_bytes()).await;

    let config = NodeConfig {
        peer_addr: addr,
        network_magic: magic,
        protocol_versions: vec![HandshakeVersion(15)],
    };

    let mut session = bootstrap(&config).await.expect("bootstrap");

    let step = sync_step_typed(
        &mut session.chain_sync,
        &mut session.block_fetch,
        Point::Origin.to_cbor_bytes(),
    )
    .await
    .expect("typed rollback step");

    assert_eq!(
        step,
        TypedSyncStep::RollBackward {
            point,
            tip,
        }
    );

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
        first_tip.to_cbor_bytes(),
        sample_block_bytes(),
        rollback_point.to_cbor_bytes(),
        rollback_tip.to_cbor_bytes(),
    )
    .await;

    let config = NodeConfig {
        peer_addr: addr,
        network_magic: magic,
        protocol_versions: vec![HandshakeVersion(15)],
    };

    let mut session = bootstrap(&config).await.expect("bootstrap");

    let progress = sync_steps_typed(
        &mut session.chain_sync,
        &mut session.block_fetch,
        Point::Origin,
        2,
    )
    .await
    .expect("typed progress");

    assert_eq!(progress.fetched_blocks, 1);
    assert_eq!(progress.rollback_count, 1);
    assert_eq!(progress.current_point, rollback_point);
    assert_eq!(progress.steps.len(), 2);
    assert!(matches!(progress.steps[0], TypedSyncStep::RollForward { .. }));
    assert!(matches!(progress.steps[1], TypedSyncStep::RollBackward { .. }));

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
        first_tip.to_cbor_bytes(),
        sample_block_bytes(),
        rollback_point.to_cbor_bytes(),
        rollback_tip.to_cbor_bytes(),
    )
    .await;

    let config = NodeConfig {
        peer_addr: addr,
        network_magic: magic,
        protocol_versions: vec![HandshakeVersion(15)],
    };

    let mut session = bootstrap(&config).await.expect("bootstrap");

    let progress = sync_until_typed(
        &mut session.chain_sync,
        &mut session.block_fetch,
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
        first_tip.to_cbor_bytes(),
        sample_block_bytes(),
        rollback_point.to_cbor_bytes(),
        rollback_tip.to_cbor_bytes(),
    )
    .await;

    let config = NodeConfig {
        peer_addr: addr,
        network_magic: magic,
        protocol_versions: vec![HandshakeVersion(15)],
    };

    let mut session = bootstrap(&config).await.expect("bootstrap");
    let progress = sync_steps_typed(
        &mut session.chain_sync,
        &mut session.block_fetch,
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
        Point::BlockPoint(SlotNo(500), HeaderHash([0x55; 32]))
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
        tip.to_cbor_bytes(),
    )
    .await;

    let config = NodeConfig {
        peer_addr: addr,
        network_magic: magic,
        protocol_versions: vec![HandshakeVersion(15)],
    };

    let mut session = bootstrap(&config).await.expect("bootstrap");

    let result = typed_find_intersect(
        &mut session.chain_sync,
        &[intersect, Point::Origin],
    )
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

    let addr = spawn_intersect_not_found_responder(magic, tip.to_cbor_bytes()).await;

    let config = NodeConfig {
        peer_addr: addr,
        network_magic: magic,
        protocol_versions: vec![HandshakeVersion(15)],
    };

    let mut session = bootstrap(&config).await.expect("bootstrap");

    let result = typed_find_intersect(
        &mut session.chain_sync,
        &[Point::BlockPoint(SlotNo(1), HeaderHash([0x01; 32]))],
    )
    .await
    .expect("find intersect");

    assert_eq!(
        result,
        TypedIntersectResult::NotFound { tip }
    );

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
        first_tip.to_cbor_bytes(),
        sample_block_bytes(),
        rollback_point.to_cbor_bytes(),
        rollback_tip.to_cbor_bytes(),
    )
    .await;

    let config = NodeConfig {
        peer_addr: addr,
        network_magic: magic,
        protocol_versions: vec![HandshakeVersion(15)],
    };

    let mut session = bootstrap(&config).await.expect("bootstrap");
    let mut store = InMemoryVolatile::default();

    let progress = sync_batch_apply(
        &mut session.chain_sync,
        &mut session.block_fetch,
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
        Point::BlockPoint(SlotNo(500), HeaderHash([0x55; 32]))
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
