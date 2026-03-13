use std::net::SocketAddr;

use yggdrasil_network::{
    BlockFetchMessage, ChainSyncMessage, HandshakeVersion, MiniProtocolNum, peer_accept,
};
use yggdrasil_ledger::{
    CborEncode, ShelleyBlock, ShelleyHeader, ShelleyHeaderBody, ShelleyOpCert, ShelleyVrfCert,
};
use yggdrasil_node::{
    DecodedSyncStep, NodeConfig, SyncStep, bootstrap, sync_step, sync_step_decoded, sync_steps,
};

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
