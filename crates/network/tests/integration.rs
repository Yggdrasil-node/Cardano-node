use yggdrasil_network::{
    BatchResponse, Bearer, BearerError, BlockFetchClient, BlockFetchMessage, BlockFetchState,
    ChainRange, ChainSyncMessage,
    ChainSyncState, ChainSyncClient, IntersectResponse, NextResponse,
    TypedIntersectResponse, TypedNextResponse,
    HandshakeMessage, HandshakeRequest, HandshakeState,
    HandshakeVersion, KeepAliveClient, KeepAliveMessage, KeepAliveState,
    MessageChannel, MiniProtocolDir, MiniProtocolNum, MuxChannel,
    NodeToNodeVersionData, RefuseReason, Sdu, SduDecodeError, SduHeader,
    TcpBearer, TxIdAndSize, TxServerRequest, TxSubmissionClient, TxSubmissionMessage,
    TxSubmissionState, SDU_HEADER_SIZE, MAX_SEGMENT_SIZE,
    start_mux, peer_connect, peer_accept, PeerError,
};
use yggdrasil_ledger::{CborEncode, HeaderHash, Point, SlotNo};

// ===========================================================================
// Legacy scaffold test (preserved)
// ===========================================================================

#[test]
fn handshake_request_keeps_version() {
    let request = HandshakeRequest {
        network_magic: 1,
        version: HandshakeVersion(12),
    };
    assert_eq!(request.version, HandshakeVersion(12));
    assert_eq!(MuxChannel(3), MuxChannel(3));
    assert_eq!(ChainSyncState::StIdle, ChainSyncState::StIdle);
}

// ===========================================================================
// SDU header encode / decode round-trip
// ===========================================================================

#[test]
fn sdu_header_encode_decode_roundtrip_initiator() {
    let hdr = SduHeader {
        timestamp: 0x0001_0203,
        protocol_num: MiniProtocolNum::CHAIN_SYNC,
        direction: MiniProtocolDir::Initiator,
        payload_length: 256,
    };
    let buf = hdr.encode();
    assert_eq!(buf.len(), SDU_HEADER_SIZE);
    let decoded = SduHeader::decode(&buf).expect("initiator header should decode");
    assert_eq!(hdr, decoded);
}

#[test]
fn sdu_header_encode_decode_roundtrip_responder() {
    let hdr = SduHeader {
        timestamp: 0xDEAD_BEEF,
        protocol_num: MiniProtocolNum::BLOCK_FETCH,
        direction: MiniProtocolDir::Responder,
        payload_length: 65535,
    };
    let buf = hdr.encode();
    let decoded = SduHeader::decode(&buf).expect("responder header should decode");
    assert_eq!(hdr, decoded);
}

#[test]
fn sdu_header_direction_bit_encoding() {
    // Initiator: bit 15 = 0
    let init_hdr = SduHeader {
        timestamp: 0,
        protocol_num: MiniProtocolNum(2),
        direction: MiniProtocolDir::Initiator,
        payload_length: 0,
    };
    let buf = init_hdr.encode();
    assert_eq!(buf[4], 0x00);
    assert_eq!(buf[5], 0x02);

    // Responder: bit 15 = 1
    let resp_hdr = SduHeader {
        timestamp: 0,
        protocol_num: MiniProtocolNum(2),
        direction: MiniProtocolDir::Responder,
        payload_length: 0,
    };
    let buf = resp_hdr.encode();
    assert_eq!(buf[4], 0x80); // 0x8000 | 2 = 0x8002 → high byte 0x80
    assert_eq!(buf[5], 0x02);
}

#[test]
fn sdu_header_decode_buffer_too_short() {
    let err = SduHeader::decode(&[0u8; 7]).expect_err("short buffer should fail");
    assert_eq!(err, SduDecodeError::BufferTooShort(7));
}

#[test]
fn sdu_header_decode_empty() {
    let err = SduHeader::decode(&[]).expect_err("empty buffer should fail");
    assert_eq!(err, SduDecodeError::BufferTooShort(0));
}

// ===========================================================================
// Mini-protocol number constants
// ===========================================================================

#[test]
fn well_known_protocol_numbers() {
    assert_eq!(MiniProtocolNum::HANDSHAKE.0, 0);
    assert_eq!(MiniProtocolNum::CHAIN_SYNC.0, 2);
    assert_eq!(MiniProtocolNum::BLOCK_FETCH.0, 3);
    assert_eq!(MiniProtocolNum::TX_SUBMISSION.0, 4);
    assert_eq!(MiniProtocolNum::KEEP_ALIVE.0, 8);
    assert_eq!(MiniProtocolNum::PEER_SHARING.0, 10);
}

// ===========================================================================
// Handshake types
// ===========================================================================

#[test]
fn handshake_version_constants() {
    assert_eq!(HandshakeVersion::V14.0, 14);
    assert_eq!(HandshakeVersion::V15.0, 15);
}

#[test]
fn handshake_message_propose_and_accept() {
    let vdata = NodeToNodeVersionData {
        network_magic: 764824073,
        initiator_only_diffusion_mode: false,
        peer_sharing: 1,
        query: false,
    };
    let propose = HandshakeMessage::ProposeVersions(vec![
        (HandshakeVersion::V14, vdata.clone()),
    ]);
    let accept = HandshakeMessage::AcceptVersion(HandshakeVersion::V14, vdata.clone());
    // Just verify construction doesn't panic and Debug works
    let _ = format!("{propose:?}");
    let _ = format!("{accept:?}");
}

#[test]
fn handshake_refuse_variants() {
    let r1 = RefuseReason::VersionMismatch(vec![HandshakeVersion::V14]);
    let r2 = RefuseReason::HandshakeDecodeError(HandshakeVersion::V14, "bad data".into());
    let r3 = RefuseReason::Refused(HandshakeVersion::V15, "policy".into());
    let _ = format!("{r1:?} {r2:?} {r3:?}");
}

#[test]
fn handshake_state_machine_happy_path() {
    assert_eq!(HandshakeState::StPropose, HandshakeState::StPropose);
    assert_eq!(HandshakeState::StConfirm, HandshakeState::StConfirm);
    assert_eq!(HandshakeState::StDone, HandshakeState::StDone);
}

// ===========================================================================
// ChainSync state transitions
// ===========================================================================

#[test]
fn chainsync_happy_path_request_next_roll_forward() {
    let s = ChainSyncState::StIdle;
    let s = s.transition(&ChainSyncMessage::MsgRequestNext)
        .expect("MsgRequestNext should be legal from StIdle");
    assert_eq!(s, ChainSyncState::StCanAwait);
    let s = s
        .transition(&ChainSyncMessage::MsgRollForward {
            header: vec![1],
            tip: vec![2],
        })
        .expect("MsgRollForward should be legal from StCanAwait");
    assert_eq!(s, ChainSyncState::StIdle);
}

#[test]
fn chainsync_await_then_roll_backward() {
    let s = ChainSyncState::StIdle;
    let s = s.transition(&ChainSyncMessage::MsgRequestNext)
        .expect("MsgRequestNext from StIdle");
    let s = s.transition(&ChainSyncMessage::MsgAwaitReply)
        .expect("MsgAwaitReply from StCanAwait");
    assert_eq!(s, ChainSyncState::StMustReply);
    let s = s
        .transition(&ChainSyncMessage::MsgRollBackward {
            point: vec![3],
            tip: vec![4],
        })
        .expect("MsgRollBackward from StMustReply");
    assert_eq!(s, ChainSyncState::StIdle);
}

#[test]
fn chainsync_find_intersect_found() {
    let s = ChainSyncState::StIdle;
    let s = s
        .transition(&ChainSyncMessage::MsgFindIntersect {
            points: vec![vec![10], vec![20]],
        })
        .expect("MsgFindIntersect from StIdle");
    assert_eq!(s, ChainSyncState::StIntersect);
    let s = s
        .transition(&ChainSyncMessage::MsgIntersectFound {
            point: vec![10],
            tip: vec![99],
        })
        .expect("MsgIntersectFound from StIntersect");
    assert_eq!(s, ChainSyncState::StIdle);
}

#[test]
fn chainsync_find_intersect_not_found() {
    let s = ChainSyncState::StIdle;
    let s = s
        .transition(&ChainSyncMessage::MsgFindIntersect {
            points: vec![vec![10]],
        })
        .expect("MsgFindIntersect from StIdle");
    let s = s
        .transition(&ChainSyncMessage::MsgIntersectNotFound { tip: vec![99] })
        .expect("MsgIntersectNotFound from StIntersect");
    assert_eq!(s, ChainSyncState::StIdle);
}

#[test]
fn chainsync_done_from_idle() {
    let s = ChainSyncState::StIdle;
    let s = s.transition(&ChainSyncMessage::MsgDone)
        .expect("MsgDone from StIdle");
    assert_eq!(s, ChainSyncState::StDone);
}

#[test]
fn chainsync_illegal_transition_rejected() {
    // Cannot send MsgAwaitReply from StIdle (server message in client state)
    let err = ChainSyncState::StIdle
        .transition(&ChainSyncMessage::MsgAwaitReply)
        .expect_err("MsgAwaitReply should be illegal from StIdle");
    assert_eq!(err.state, ChainSyncState::StIdle);
    assert_eq!(err.message, "MsgAwaitReply");

    // Cannot send MsgRequestNext from StCanAwait
    let err = ChainSyncState::StCanAwait
        .transition(&ChainSyncMessage::MsgRequestNext)
        .expect_err("MsgRequestNext should be illegal from StCanAwait");
    assert_eq!(err.state, ChainSyncState::StCanAwait);

    // Cannot do anything from StDone
    let err = ChainSyncState::StDone
        .transition(&ChainSyncMessage::MsgRequestNext)
        .expect_err("MsgRequestNext should be illegal from StDone");
    assert_eq!(err.state, ChainSyncState::StDone);
}

#[test]
fn chainsync_wire_tags() {
    assert_eq!(ChainSyncMessage::MsgRequestNext.wire_tag(), 0);
    assert_eq!(ChainSyncMessage::MsgAwaitReply.wire_tag(), 1);
    assert_eq!(
        ChainSyncMessage::MsgRollForward {
            header: vec![],
            tip: vec![]
        }
        .wire_tag(),
        2
    );
    assert_eq!(
        ChainSyncMessage::MsgRollBackward {
            point: vec![],
            tip: vec![]
        }
        .wire_tag(),
        3
    );
    assert_eq!(
        ChainSyncMessage::MsgFindIntersect { points: vec![] }.wire_tag(),
        4
    );
    assert_eq!(
        ChainSyncMessage::MsgIntersectFound {
            point: vec![],
            tip: vec![]
        }
        .wire_tag(),
        5
    );
    assert_eq!(
        ChainSyncMessage::MsgIntersectNotFound { tip: vec![] }.wire_tag(),
        6
    );
    assert_eq!(ChainSyncMessage::MsgDone.wire_tag(), 7);
}

// ===========================================================================
// BlockFetch state transitions
// ===========================================================================

#[test]
fn blockfetch_happy_path_stream_blocks() {
    let s = BlockFetchState::StIdle;
    let range = ChainRange {
        lower: vec![1],
        upper: vec![2],
    };
    let s = s
        .transition(&BlockFetchMessage::MsgRequestRange(range))
        .expect("MsgRequestRange from StIdle");
    assert_eq!(s, BlockFetchState::StBusy);

    let s = s.transition(&BlockFetchMessage::MsgStartBatch)
        .expect("MsgStartBatch from StBusy");
    assert_eq!(s, BlockFetchState::StStreaming);

    let s = s
        .transition(&BlockFetchMessage::MsgBlock {
            block: vec![0xAB],
        })
        .expect("MsgBlock from StStreaming");
    assert_eq!(s, BlockFetchState::StStreaming);

    let s = s.transition(&BlockFetchMessage::MsgBatchDone)
        .expect("MsgBatchDone from StStreaming");
    assert_eq!(s, BlockFetchState::StIdle);
}

#[test]
fn blockfetch_no_blocks() {
    let s = BlockFetchState::StIdle;
    let range = ChainRange {
        lower: vec![1],
        upper: vec![2],
    };
    let s = s
        .transition(&BlockFetchMessage::MsgRequestRange(range))
        .expect("MsgRequestRange from StIdle");
    let s = s.transition(&BlockFetchMessage::MsgNoBlocks)
        .expect("MsgNoBlocks from StBusy");
    assert_eq!(s, BlockFetchState::StIdle);
}

#[test]
fn blockfetch_client_done() {
    let s = BlockFetchState::StIdle;
    let s = s.transition(&BlockFetchMessage::MsgClientDone)
        .expect("MsgClientDone from StIdle");
    assert_eq!(s, BlockFetchState::StDone);
}

#[test]
fn blockfetch_illegal_transition_rejected() {
    // Cannot send MsgStartBatch from StIdle
    let err = BlockFetchState::StIdle
        .transition(&BlockFetchMessage::MsgStartBatch)
        .expect_err("MsgStartBatch should be illegal from StIdle");
    assert_eq!(err.state, BlockFetchState::StIdle);
    assert_eq!(err.message, "MsgStartBatch");

    // Cannot send MsgClientDone from StBusy
    let range = ChainRange {
        lower: vec![],
        upper: vec![],
    };
    let busy = BlockFetchState::StIdle
        .transition(&BlockFetchMessage::MsgRequestRange(range))
        .expect("MsgRequestRange from StIdle");
    let err = busy
        .transition(&BlockFetchMessage::MsgClientDone)
        .expect_err("MsgClientDone should be illegal from StBusy");
    assert_eq!(err.state, BlockFetchState::StBusy);

    // Nothing from StDone
    let err = BlockFetchState::StDone
        .transition(&BlockFetchMessage::MsgNoBlocks)
        .expect_err("MsgNoBlocks should be illegal from StDone");
    assert_eq!(err.state, BlockFetchState::StDone);
}

#[test]
fn blockfetch_wire_tags() {
    let range = ChainRange {
        lower: vec![],
        upper: vec![],
    };
    assert_eq!(BlockFetchMessage::MsgRequestRange(range).wire_tag(), 0);
    assert_eq!(BlockFetchMessage::MsgClientDone.wire_tag(), 1);
    assert_eq!(BlockFetchMessage::MsgStartBatch.wire_tag(), 2);
    assert_eq!(BlockFetchMessage::MsgNoBlocks.wire_tag(), 3);
    assert_eq!(
        BlockFetchMessage::MsgBlock { block: vec![] }.wire_tag(),
        4
    );
    assert_eq!(BlockFetchMessage::MsgBatchDone.wire_tag(), 5);
}

// ===========================================================================
// KeepAlive state machine
// ===========================================================================

#[test]
fn keepalive_happy_path_round_trip() {
    let state = KeepAliveState::StClient;
    let state = state
        .transition(&KeepAliveMessage::MsgKeepAlive { cookie: 42 })
        .expect("MsgKeepAlive from StClient");
    assert_eq!(state, KeepAliveState::StServer);
    let state = state
        .transition(&KeepAliveMessage::MsgKeepAliveResponse { cookie: 42 })
        .expect("MsgKeepAliveResponse from StServer");
    assert_eq!(state, KeepAliveState::StClient);
}

#[test]
fn keepalive_done_from_client() {
    let state = KeepAliveState::StClient;
    let state = state
        .transition(&KeepAliveMessage::MsgDone)
        .expect("MsgDone from StClient");
    assert_eq!(state, KeepAliveState::StDone);
}

#[test]
fn keepalive_illegal_transition_rejected() {
    let err = KeepAliveState::StServer
        .transition(&KeepAliveMessage::MsgDone)
        .expect_err("MsgDone should be illegal from StServer");
    assert_eq!(err.to_string(), "illegal keep-alive transition from StServer via MsgDone");

    let err = KeepAliveState::StDone
        .transition(&KeepAliveMessage::MsgKeepAlive { cookie: 1 })
        .expect_err("MsgKeepAlive should be illegal from StDone");
    assert_eq!(err.to_string(), "illegal keep-alive transition from StDone via MsgKeepAlive");
}

// ===========================================================================
// KeepAlive CBOR round-trip
// ===========================================================================

#[test]
fn keepalive_cbor_msg_keepalive_round_trip() {
    let msg = KeepAliveMessage::MsgKeepAlive { cookie: 12345 };
    let bytes = msg.to_cbor();
    let decoded = KeepAliveMessage::from_cbor(&bytes).expect("decode MsgKeepAlive");
    assert_eq!(msg, decoded);
}

#[test]
fn keepalive_cbor_msg_done_round_trip() {
    let msg = KeepAliveMessage::MsgDone;
    let bytes = msg.to_cbor();
    let decoded = KeepAliveMessage::from_cbor(&bytes).expect("decode MsgDone");
    assert_eq!(msg, decoded);
}

#[test]
fn keepalive_cbor_msg_response_round_trip() {
    let msg = KeepAliveMessage::MsgKeepAliveResponse { cookie: 0xFFFF };
    let bytes = msg.to_cbor();
    let decoded = KeepAliveMessage::from_cbor(&bytes).expect("decode MsgKeepAliveResponse");
    assert_eq!(msg, decoded);
}

// ===========================================================================
// ChainSync CBOR round-trip
// ===========================================================================

#[test]
fn chainsync_cbor_request_next_round_trip() {
    let msg = ChainSyncMessage::MsgRequestNext;
    let decoded = ChainSyncMessage::from_cbor(&msg.to_cbor()).expect("decode");
    assert_eq!(msg, decoded);
}

#[test]
fn chainsync_cbor_await_reply_round_trip() {
    let msg = ChainSyncMessage::MsgAwaitReply;
    let decoded = ChainSyncMessage::from_cbor(&msg.to_cbor()).expect("decode");
    assert_eq!(msg, decoded);
}

#[test]
fn chainsync_cbor_roll_forward_round_trip() {
    let msg = ChainSyncMessage::MsgRollForward {
        header: vec![1, 2, 3, 4],
        tip: vec![5, 6, 7],
    };
    let decoded = ChainSyncMessage::from_cbor(&msg.to_cbor()).expect("decode");
    assert_eq!(msg, decoded);
}

#[test]
fn chainsync_cbor_roll_backward_round_trip() {
    let msg = ChainSyncMessage::MsgRollBackward {
        point: vec![10, 20],
        tip: vec![30, 40, 50],
    };
    let decoded = ChainSyncMessage::from_cbor(&msg.to_cbor()).expect("decode");
    assert_eq!(msg, decoded);
}

#[test]
fn chainsync_cbor_find_intersect_round_trip() {
    let msg = ChainSyncMessage::MsgFindIntersect {
        points: vec![vec![1, 2], vec![3, 4, 5], vec![]],
    };
    let decoded = ChainSyncMessage::from_cbor(&msg.to_cbor()).expect("decode");
    assert_eq!(msg, decoded);
}

#[test]
fn chainsync_cbor_intersect_found_round_trip() {
    let msg = ChainSyncMessage::MsgIntersectFound {
        point: vec![0xAA, 0xBB],
        tip: vec![0xCC],
    };
    let decoded = ChainSyncMessage::from_cbor(&msg.to_cbor()).expect("decode");
    assert_eq!(msg, decoded);
}

#[test]
fn chainsync_cbor_intersect_not_found_round_trip() {
    let msg = ChainSyncMessage::MsgIntersectNotFound {
        tip: vec![0xDD, 0xEE, 0xFF],
    };
    let decoded = ChainSyncMessage::from_cbor(&msg.to_cbor()).expect("decode");
    assert_eq!(msg, decoded);
}

#[test]
fn chainsync_cbor_done_round_trip() {
    let msg = ChainSyncMessage::MsgDone;
    let decoded = ChainSyncMessage::from_cbor(&msg.to_cbor()).expect("decode");
    assert_eq!(msg, decoded);
}

// ===========================================================================
// BlockFetch CBOR round-trip
// ===========================================================================

#[test]
fn blockfetch_cbor_request_range_round_trip() {
    let msg = BlockFetchMessage::MsgRequestRange(ChainRange {
        lower: vec![1, 2, 3],
        upper: vec![4, 5, 6],
    });
    let decoded = BlockFetchMessage::from_cbor(&msg.to_cbor()).expect("decode");
    assert_eq!(msg, decoded);
}

#[test]
fn blockfetch_cbor_client_done_round_trip() {
    let msg = BlockFetchMessage::MsgClientDone;
    let decoded = BlockFetchMessage::from_cbor(&msg.to_cbor()).expect("decode");
    assert_eq!(msg, decoded);
}

#[test]
fn blockfetch_cbor_start_batch_round_trip() {
    let msg = BlockFetchMessage::MsgStartBatch;
    let decoded = BlockFetchMessage::from_cbor(&msg.to_cbor()).expect("decode");
    assert_eq!(msg, decoded);
}

#[test]
fn blockfetch_cbor_no_blocks_round_trip() {
    let msg = BlockFetchMessage::MsgNoBlocks;
    let decoded = BlockFetchMessage::from_cbor(&msg.to_cbor()).expect("decode");
    assert_eq!(msg, decoded);
}

#[test]
fn blockfetch_cbor_block_round_trip() {
    let msg = BlockFetchMessage::MsgBlock {
        block: vec![0xDE, 0xAD, 0xBE, 0xEF],
    };
    let decoded = BlockFetchMessage::from_cbor(&msg.to_cbor()).expect("decode");
    assert_eq!(msg, decoded);
}

#[test]
fn blockfetch_cbor_batch_done_round_trip() {
    let msg = BlockFetchMessage::MsgBatchDone;
    let decoded = BlockFetchMessage::from_cbor(&msg.to_cbor()).expect("decode");
    assert_eq!(msg, decoded);
}

// ===========================================================================
// Handshake CBOR round-trip
// ===========================================================================

fn mainnet_version_data() -> NodeToNodeVersionData {
    NodeToNodeVersionData {
        network_magic: 764824073,
        initiator_only_diffusion_mode: false,
        peer_sharing: 1,
        query: false,
    }
}

#[test]
fn handshake_cbor_propose_versions_round_trip() {
    let msg = HandshakeMessage::ProposeVersions(vec![
        (HandshakeVersion::V14, mainnet_version_data()),
        (HandshakeVersion::V15, NodeToNodeVersionData {
            network_magic: 764824073,
            initiator_only_diffusion_mode: true,
            peer_sharing: 0,
            query: true,
        }),
    ]);
    let decoded = HandshakeMessage::from_cbor(&msg.to_cbor()).expect("decode ProposeVersions");
    assert_eq!(msg, decoded);
}

#[test]
fn handshake_cbor_accept_version_round_trip() {
    let msg = HandshakeMessage::AcceptVersion(HandshakeVersion::V14, mainnet_version_data());
    let decoded = HandshakeMessage::from_cbor(&msg.to_cbor()).expect("decode AcceptVersion");
    assert_eq!(msg, decoded);
}

#[test]
fn handshake_cbor_refuse_version_mismatch_round_trip() {
    let msg = HandshakeMessage::Refuse(RefuseReason::VersionMismatch(vec![
        HandshakeVersion(10),
        HandshakeVersion(11),
    ]));
    let decoded = HandshakeMessage::from_cbor(&msg.to_cbor()).expect("decode Refuse VersionMismatch");
    assert_eq!(msg, decoded);
}

#[test]
fn handshake_cbor_refuse_decode_error_round_trip() {
    let msg = HandshakeMessage::Refuse(RefuseReason::HandshakeDecodeError(
        HandshakeVersion::V14,
        "bad version data".to_owned(),
    ));
    let decoded = HandshakeMessage::from_cbor(&msg.to_cbor()).expect("decode Refuse DecodeError");
    assert_eq!(msg, decoded);
}

#[test]
fn handshake_cbor_refuse_refused_round_trip() {
    let msg = HandshakeMessage::Refuse(RefuseReason::Refused(
        HandshakeVersion::V15,
        "incompatible peer".to_owned(),
    ));
    let decoded = HandshakeMessage::from_cbor(&msg.to_cbor()).expect("decode Refuse Refused");
    assert_eq!(msg, decoded);
}

#[test]
fn handshake_cbor_query_reply_round_trip() {
    let msg = HandshakeMessage::QueryReply(vec![
        (HandshakeVersion::V14, mainnet_version_data()),
    ]);
    let decoded = HandshakeMessage::from_cbor(&msg.to_cbor()).expect("decode QueryReply");
    assert_eq!(msg, decoded);
}

// ===========================================================================
// Handshake state transitions
// ===========================================================================

#[test]
fn handshake_transition_propose_to_confirm() {
    let state = HandshakeState::StPropose;
    let state = state
        .transition(&HandshakeMessage::ProposeVersions(vec![
            (HandshakeVersion::V14, mainnet_version_data()),
        ]))
        .expect("ProposeVersions from StPropose");
    assert_eq!(state, HandshakeState::StConfirm);
}

#[test]
fn handshake_transition_confirm_to_done_accept() {
    let state = HandshakeState::StConfirm;
    let state = state
        .transition(&HandshakeMessage::AcceptVersion(
            HandshakeVersion::V14,
            mainnet_version_data(),
        ))
        .expect("AcceptVersion from StConfirm");
    assert_eq!(state, HandshakeState::StDone);
}

#[test]
fn handshake_transition_confirm_to_done_refuse() {
    let state = HandshakeState::StConfirm;
    let state = state
        .transition(&HandshakeMessage::Refuse(RefuseReason::VersionMismatch(vec![])))
        .expect("Refuse from StConfirm");
    assert_eq!(state, HandshakeState::StDone);
}

#[test]
fn handshake_transition_illegal_from_done() {
    let err = HandshakeState::StDone
        .transition(&HandshakeMessage::ProposeVersions(vec![]))
        .expect_err("ProposeVersions should be illegal from StDone");
    assert_eq!(err.to_string(), "illegal handshake transition from StDone via ProposeVersions");
}

// ===========================================================================
// TxSubmission2 — state machine transitions
// ===========================================================================

#[test]
fn tx_submission_happy_path() {
    let mut state = TxSubmissionState::StInit;
    state = state
        .transition(&TxSubmissionMessage::MsgInit)
        .expect("MsgInit from StInit");
    assert_eq!(state, TxSubmissionState::StIdle);

    state = state
        .transition(&TxSubmissionMessage::MsgRequestTxIds {
            blocking: false,
            ack: 0,
            req: 3,
        })
        .expect("MsgRequestTxIds from StIdle");
    assert_eq!(state, TxSubmissionState::StTxIds { blocking: false });

    state = state
        .transition(&TxSubmissionMessage::MsgReplyTxIds {
            txids: vec![TxIdAndSize {
                txid: vec![1, 2, 3],
                size: 100,
            }],
        })
        .expect("MsgReplyTxIds from StTxIds");
    assert_eq!(state, TxSubmissionState::StIdle);

    state = state
        .transition(&TxSubmissionMessage::MsgRequestTxs {
            txids: vec![vec![1, 2, 3]],
        })
        .expect("MsgRequestTxs from StIdle");
    assert_eq!(state, TxSubmissionState::StTxs);

    state = state
        .transition(&TxSubmissionMessage::MsgReplyTxs {
            txs: vec![vec![0xAA, 0xBB]],
        })
        .expect("MsgReplyTxs from StTxs");
    assert_eq!(state, TxSubmissionState::StIdle);
}

#[test]
fn tx_submission_blocking_done() {
    let state = TxSubmissionState::StTxIds { blocking: true };
    let next = state
        .transition(&TxSubmissionMessage::MsgDone)
        .expect("MsgDone from blocking StTxIds");
    assert_eq!(next, TxSubmissionState::StDone);
}

#[test]
fn tx_submission_nonblocking_cannot_done() {
    let state = TxSubmissionState::StTxIds { blocking: false };
    state
        .transition(&TxSubmissionMessage::MsgDone)
        .expect_err("MsgDone should be illegal from non-blocking StTxIds");
}

#[test]
fn tx_submission_illegal_from_init() {
    TxSubmissionState::StInit
        .transition(&TxSubmissionMessage::MsgRequestTxIds {
            blocking: false,
            ack: 0,
            req: 1,
        })
        .expect_err("MsgRequestTxIds should be illegal from StInit");
}

#[test]
fn tx_submission_illegal_from_done() {
    TxSubmissionState::StDone
        .transition(&TxSubmissionMessage::MsgInit)
        .expect_err("MsgInit should be illegal from StDone");
}

// ===========================================================================
// TxSubmission2 — CBOR round-trip
// ===========================================================================

#[test]
fn tx_submission_cbor_init() {
    let msg = TxSubmissionMessage::MsgInit;
    let bytes = msg.to_cbor();
    let decoded = TxSubmissionMessage::from_cbor(&bytes).expect("decode MsgInit");
    assert_eq!(msg, decoded);
}

#[test]
fn tx_submission_cbor_request_txids() {
    let msg = TxSubmissionMessage::MsgRequestTxIds {
        blocking: true,
        ack: 5,
        req: 10,
    };
    let bytes = msg.to_cbor();
    let decoded = TxSubmissionMessage::from_cbor(&bytes).expect("decode MsgRequestTxIds");
    assert_eq!(msg, decoded);
}

#[test]
fn tx_submission_cbor_reply_txids() {
    let msg = TxSubmissionMessage::MsgReplyTxIds {
        txids: vec![
            TxIdAndSize {
                txid: vec![0xDE, 0xAD],
                size: 256,
            },
            TxIdAndSize {
                txid: vec![0xBE, 0xEF],
                size: 512,
            },
        ],
    };
    let bytes = msg.to_cbor();
    let decoded = TxSubmissionMessage::from_cbor(&bytes).expect("decode MsgReplyTxIds");
    assert_eq!(msg, decoded);
}

#[test]
fn tx_submission_cbor_reply_txids_empty() {
    let msg = TxSubmissionMessage::MsgReplyTxIds { txids: vec![] };
    let bytes = msg.to_cbor();
    let decoded = TxSubmissionMessage::from_cbor(&bytes).expect("decode empty MsgReplyTxIds");
    assert_eq!(msg, decoded);
}

#[test]
fn tx_submission_cbor_request_txs() {
    let msg = TxSubmissionMessage::MsgRequestTxs {
        txids: vec![vec![1, 2, 3], vec![4, 5, 6]],
    };
    let bytes = msg.to_cbor();
    let decoded = TxSubmissionMessage::from_cbor(&bytes).expect("decode MsgRequestTxs");
    assert_eq!(msg, decoded);
}

#[test]
fn tx_submission_cbor_reply_txs() {
    let msg = TxSubmissionMessage::MsgReplyTxs {
        txs: vec![vec![0xAA, 0xBB], vec![0xCC, 0xDD, 0xEE]],
    };
    let bytes = msg.to_cbor();
    let decoded = TxSubmissionMessage::from_cbor(&bytes).expect("decode MsgReplyTxs");
    assert_eq!(msg, decoded);
}

#[test]
fn tx_submission_cbor_reply_txs_empty() {
    let msg = TxSubmissionMessage::MsgReplyTxs { txs: vec![] };
    let bytes = msg.to_cbor();
    let decoded = TxSubmissionMessage::from_cbor(&bytes).expect("decode empty MsgReplyTxs");
    assert_eq!(msg, decoded);
}

#[test]
fn tx_submission_cbor_done() {
    let msg = TxSubmissionMessage::MsgDone;
    let bytes = msg.to_cbor();
    let decoded = TxSubmissionMessage::from_cbor(&bytes).expect("decode MsgDone");
    assert_eq!(msg, decoded);
}

// ===========================================================================
// Bearer — SDU round-trip over TCP loopback
// ===========================================================================

#[tokio::test]
async fn tcp_bearer_sdu_round_trip() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("local addr");

    let payload = b"hello mux".to_vec();
    let sdu = Sdu::new(
        MiniProtocolNum::CHAIN_SYNC,
        MiniProtocolDir::Initiator,
        payload.clone(),
    );

    let send_handle = tokio::spawn(async move {
        let mut client = TcpBearer::connect(addr).await.expect("connect");
        client.send(&sdu).await.expect("send");
    });

    let (stream, _) = listener.accept().await.expect("accept");
    let mut server = TcpBearer::new(stream);
    let received = server.recv().await.expect("recv");

    send_handle.await.expect("join send task");

    assert_eq!(received.header.protocol_num, MiniProtocolNum::CHAIN_SYNC);
    assert_eq!(received.header.direction, MiniProtocolDir::Initiator);
    assert_eq!(received.payload, payload);
}

#[tokio::test]
async fn tcp_bearer_multiple_sdus() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("local addr");

    let messages: Vec<(MiniProtocolNum, Vec<u8>)> = vec![
        (MiniProtocolNum::HANDSHAKE, vec![0x01, 0x02]),
        (MiniProtocolNum::CHAIN_SYNC, vec![0xAA, 0xBB, 0xCC]),
        (MiniProtocolNum::BLOCK_FETCH, vec![]),
        (MiniProtocolNum::KEEP_ALIVE, vec![0xFF; 100]),
    ];

    let send_msgs = messages.clone();
    let send_handle = tokio::spawn(async move {
        let mut client = TcpBearer::connect(addr).await.expect("connect");
        for (proto, payload) in &send_msgs {
            let sdu = Sdu::new(*proto, MiniProtocolDir::Initiator, payload.clone());
            client.send(&sdu).await.expect("send");
        }
    });

    let (stream, _) = listener.accept().await.expect("accept");
    let mut server = TcpBearer::new(stream);

    for (expected_proto, expected_payload) in &messages {
        let received = server.recv().await.expect("recv");
        assert_eq!(received.header.protocol_num, *expected_proto);
        assert_eq!(received.payload, *expected_payload);
    }

    send_handle.await.expect("join send task");
}

#[tokio::test]
async fn tcp_bearer_connection_closed_on_eof() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("local addr");

    // Client connects and immediately drops.
    let send_handle = tokio::spawn(async move {
        let _client = TcpBearer::connect(addr).await.expect("connect");
        // Drop immediately.
    });

    let (stream, _) = listener.accept().await.expect("accept");
    let mut server = TcpBearer::new(stream);

    send_handle.await.expect("join send task");

    let err = server.recv().await.expect_err("should get connection closed");
    assert!(
        matches!(err, BearerError::ConnectionClosed),
        "expected ConnectionClosed, got {err:?}"
    );
}

#[tokio::test]
async fn tcp_bearer_responder_direction() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("local addr");

    let sdu = Sdu::new(
        MiniProtocolNum::BLOCK_FETCH,
        MiniProtocolDir::Responder,
        vec![0xDE, 0xAD],
    );

    let send_handle = tokio::spawn({
        let sdu = sdu.clone();
        async move {
            let mut client = TcpBearer::connect(addr).await.expect("connect");
            client.send(&sdu).await.expect("send");
        }
    });

    let (stream, _) = listener.accept().await.expect("accept");
    let mut server = TcpBearer::new(stream);
    let received = server.recv().await.expect("recv");

    send_handle.await.expect("join send task");

    assert_eq!(received.header.direction, MiniProtocolDir::Responder);
    assert_eq!(received.header.protocol_num, MiniProtocolNum::BLOCK_FETCH);
    assert_eq!(received.payload, vec![0xDE, 0xAD]);
}

#[test]
fn sdu_new_sets_payload_length() {
    let payload = vec![1, 2, 3, 4, 5];
    let sdu = Sdu::new(
        MiniProtocolNum::TX_SUBMISSION,
        MiniProtocolDir::Initiator,
        payload.clone(),
    );
    assert_eq!(sdu.header.payload_length, 5);
    assert_eq!(sdu.header.protocol_num, MiniProtocolNum::TX_SUBMISSION);
    assert_eq!(sdu.header.direction, MiniProtocolDir::Initiator);
    assert_eq!(sdu.header.timestamp, 0);
    assert_eq!(sdu.payload, payload);
}

// ===========================================================================
// Mux — single-protocol round-trip
// ===========================================================================

#[tokio::test]
async fn mux_single_protocol_round_trip() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");

    let protos = [MiniProtocolNum::CHAIN_SYNC];
    let payload = vec![0xCA, 0xFE];

    let client_handle = tokio::spawn(async move {
        let stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
        let (mut handles, mux) = start_mux(stream, MiniProtocolDir::Initiator, &protos, 8);
        let ch = handles.get_mut(&MiniProtocolNum::CHAIN_SYNC).expect("handle");
        ch.send(vec![0xCA, 0xFE]).await.expect("send");
        // Wait briefly for the server to process, then abort.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        mux.abort();
    });

    let (stream, _) = listener.accept().await.expect("accept");
    let (mut handles, mux) = start_mux(stream, MiniProtocolDir::Responder, &protos, 8);
    let ch = handles.get_mut(&MiniProtocolNum::CHAIN_SYNC).expect("handle");
    let received = ch.recv().await.expect("recv payload");
    assert_eq!(received, payload);
    mux.abort();

    client_handle.await.expect("client task");
}

// ===========================================================================
// Mux — multi-protocol routing
// ===========================================================================

#[tokio::test]
async fn mux_multi_protocol_routing() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");

    let protos = [
        MiniProtocolNum::CHAIN_SYNC,
        MiniProtocolNum::BLOCK_FETCH,
        MiniProtocolNum::KEEP_ALIVE,
    ];

    let client_handle = tokio::spawn(async move {
        let stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
        let (handles, mux) = start_mux(stream, MiniProtocolDir::Initiator, &protos, 8);

        // Send on each protocol in a specific order.
        let cs = handles.get(&MiniProtocolNum::CHAIN_SYNC).expect("cs");
        let bf = handles.get(&MiniProtocolNum::BLOCK_FETCH).expect("bf");
        let ka = handles.get(&MiniProtocolNum::KEEP_ALIVE).expect("ka");

        cs.send(vec![0x01]).await.expect("send cs");
        bf.send(vec![0x02]).await.expect("send bf");
        ka.send(vec![0x03]).await.expect("send ka");

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        mux.abort();
    });

    let (stream, _) = listener.accept().await.expect("accept");
    let (mut handles, mux) = start_mux(stream, MiniProtocolDir::Responder, &protos, 8);

    let cs = handles.get_mut(&MiniProtocolNum::CHAIN_SYNC).expect("cs");
    assert_eq!(cs.recv().await.expect("cs payload"), vec![0x01]);

    let bf = handles.get_mut(&MiniProtocolNum::BLOCK_FETCH).expect("bf");
    assert_eq!(bf.recv().await.expect("bf payload"), vec![0x02]);

    let ka = handles.get_mut(&MiniProtocolNum::KEEP_ALIVE).expect("ka");
    assert_eq!(ka.recv().await.expect("ka payload"), vec![0x03]);

    mux.abort();
    client_handle.await.expect("client task");
}

// ===========================================================================
// Mux — bidirectional exchange
// ===========================================================================

#[tokio::test]
async fn mux_bidirectional_exchange() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");

    let protos = [MiniProtocolNum::KEEP_ALIVE];

    let client_handle = tokio::spawn(async move {
        let stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
        let (mut handles, mux) = start_mux(stream, MiniProtocolDir::Initiator, &protos, 8);
        let ch = handles.get_mut(&MiniProtocolNum::KEEP_ALIVE).expect("handle");

        // Client sends ping.
        ch.send(vec![0xAA]).await.expect("send ping");

        // Client receives pong.
        let pong = ch.recv().await.expect("recv pong");
        assert_eq!(pong, vec![0xBB]);

        mux.abort();
    });

    let (stream, _) = listener.accept().await.expect("accept");
    let (mut handles, mux) = start_mux(stream, MiniProtocolDir::Responder, &protos, 8);
    let ch = handles.get_mut(&MiniProtocolNum::KEEP_ALIVE).expect("handle");

    // Server receives ping.
    let ping = ch.recv().await.expect("recv ping");
    assert_eq!(ping, vec![0xAA]);

    // Server sends pong.
    ch.send(vec![0xBB]).await.expect("send pong");

    // Wait for client to receive and finish.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    mux.abort();

    client_handle.await.expect("client task");
}

// ===========================================================================
// Mux — connection close detected
// ===========================================================================

#[tokio::test]
async fn mux_connection_close_detected() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");

    let protos = [MiniProtocolNum::CHAIN_SYNC];

    // Client connects and immediately drops.
    let client_handle = tokio::spawn(async move {
        let _stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
        // Drop without starting mux.
    });

    let (stream, _) = listener.accept().await.expect("accept");
    let (_handles, mux) = start_mux(stream, MiniProtocolDir::Responder, &protos, 8);

    client_handle.await.expect("client task");

    // The reader task should detect connection close.
    let result = mux.reader.await.expect("reader task should not panic");
    assert!(result.is_err(), "reader should detect connection close");
}

// ===========================================================================
// Mux — empty payload round-trip
// ===========================================================================

#[tokio::test]
async fn mux_empty_payload_round_trip() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");

    let protos = [MiniProtocolNum::HANDSHAKE];

    let client_handle = tokio::spawn(async move {
        let stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
        let (handles, mux) = start_mux(stream, MiniProtocolDir::Initiator, &protos, 8);
        let h = handles.get(&MiniProtocolNum::HANDSHAKE).expect("handle");
        h.send(vec![]).await.expect("send empty");
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        mux.abort();
    });

    let (stream, _) = listener.accept().await.expect("accept");
    let (mut handles, mux) = start_mux(stream, MiniProtocolDir::Responder, &protos, 8);
    let h = handles.get_mut(&MiniProtocolNum::HANDSHAKE).expect("handle");
    let received = h.recv().await.expect("recv empty payload");
    assert!(received.is_empty());
    mux.abort();

    client_handle.await.expect("client task");
}

// ===========================================================================
// Mux — multiple messages on one protocol
// ===========================================================================

#[tokio::test]
async fn mux_multiple_messages_same_protocol() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");

    let protos = [MiniProtocolNum::BLOCK_FETCH];
    let messages: Vec<Vec<u8>> = vec![
        vec![0x01, 0x02],
        vec![0x03, 0x04, 0x05],
        vec![0x06],
        vec![0x07, 0x08, 0x09, 0x0A],
    ];

    let send_msgs = messages.clone();
    let client_handle = tokio::spawn(async move {
        let stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
        let (handles, mux) = start_mux(stream, MiniProtocolDir::Initiator, &protos, 8);
        let bf = handles.get(&MiniProtocolNum::BLOCK_FETCH).expect("handle");
        for msg in &send_msgs {
            bf.send(msg.clone()).await.expect("send");
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        mux.abort();
    });

    let (stream, _) = listener.accept().await.expect("accept");
    let (mut handles, mux) = start_mux(stream, MiniProtocolDir::Responder, &protos, 8);
    let bf = handles.get_mut(&MiniProtocolNum::BLOCK_FETCH).expect("handle");
    for expected in &messages {
        let received = bf.recv().await.expect("recv");
        assert_eq!(&received, expected);
    }
    mux.abort();

    client_handle.await.expect("client task");
}

// ===========================================================================
// Mux — clean shutdown when all handles dropped
// ===========================================================================

#[tokio::test]
async fn mux_clean_shutdown_on_handle_drop() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");

    let protos = [MiniProtocolNum::CHAIN_SYNC];

    let client_handle = tokio::spawn(async move {
        let stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
        let (handles, mux) = start_mux(stream, MiniProtocolDir::Initiator, &protos, 8);
        // Drop all protocol handles — writer should exit cleanly.
        drop(handles);
        let writer_result = mux.writer.await.expect("writer should not panic");
        assert!(writer_result.is_ok(), "writer should exit cleanly on handle drop");
        mux.reader.abort();
    });

    let (stream, _) = listener.accept().await.expect("accept");
    // Just keep the connection alive until client finishes.
    let (_handles, mux) = start_mux(stream, MiniProtocolDir::Responder, &protos, 8);
    client_handle.await.expect("client task");
    mux.abort();
}

// ===========================================================================
// SDU segmentation — large message round-trip via MessageChannel
// ===========================================================================

#[tokio::test]
async fn sdu_segmentation_large_payload_round_trip() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");

    let protos = [MiniProtocolNum::BLOCK_FETCH];

    // Build a CBOR byte-string payload larger than MAX_SEGMENT_SIZE.
    // CBOR: major 2 (bstr), 2-byte length (0x59 = additional 25), then N bytes.
    let body_len: usize = MAX_SEGMENT_SIZE * 3 + 42; // spans 4 SDU segments
    let mut payload = Vec::with_capacity(3 + body_len);
    payload.push(0x59); // major 2, additional 25 → 2-byte length
    payload.extend_from_slice(&(body_len as u16).to_be_bytes());
    for i in 0..body_len {
        payload.push((i & 0xFF) as u8);
    }

    let send_payload = payload.clone();
    let client_handle = tokio::spawn(async move {
        let stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
        let (mut handles, mux) = start_mux(stream, MiniProtocolDir::Initiator, &protos, 8);
        let handle = handles.remove(&MiniProtocolNum::BLOCK_FETCH).expect("handle");
        let ch = MessageChannel::new(handle);
        ch.send(send_payload).await.expect("send large payload");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        mux.abort();
    });

    let (stream, _) = listener.accept().await.expect("accept");
    let (mut handles, mux) = start_mux(stream, MiniProtocolDir::Responder, &protos, 8);
    let handle = handles.remove(&MiniProtocolNum::BLOCK_FETCH).expect("handle");
    let mut ch = MessageChannel::new(handle);
    let received = ch.recv().await.expect("recv reassembled message");
    assert_eq!(received.len(), payload.len());
    assert_eq!(received, payload);
    mux.abort();

    client_handle.await.expect("client task");
}

// ===========================================================================
// SDU segmentation — exact multiple of segment size
// ===========================================================================

#[tokio::test]
async fn sdu_segmentation_exact_multiple() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");

    let protos = [MiniProtocolNum::CHAIN_SYNC];

    // Build a payload exactly 2 * MAX_SEGMENT_SIZE bytes (CBOR bstr).
    let body_len: usize = 2 * MAX_SEGMENT_SIZE - 3; // minus CBOR header (3 bytes)
    let mut payload = Vec::with_capacity(3 + body_len);
    payload.push(0x59); // major 2, additional 25 → 2-byte length
    payload.extend_from_slice(&(body_len as u16).to_be_bytes());
    payload.resize(3 + body_len, 0xAB);
    assert_eq!(payload.len(), 2 * MAX_SEGMENT_SIZE);

    let send_payload = payload.clone();
    let client_handle = tokio::spawn(async move {
        let stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
        let (mut handles, mux) = start_mux(stream, MiniProtocolDir::Initiator, &protos, 8);
        let handle = handles.remove(&MiniProtocolNum::CHAIN_SYNC).expect("handle");
        let ch = MessageChannel::new(handle);
        ch.send(send_payload).await.expect("send exact-multiple payload");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        mux.abort();
    });

    let (stream, _) = listener.accept().await.expect("accept");
    let (mut handles, mux) = start_mux(stream, MiniProtocolDir::Responder, &protos, 8);
    let handle = handles.remove(&MiniProtocolNum::CHAIN_SYNC).expect("handle");
    let mut ch = MessageChannel::new(handle);
    let received = ch.recv().await.expect("recv");
    assert_eq!(received, payload);
    mux.abort();

    client_handle.await.expect("client task");
}

// ===========================================================================
// SDU segmentation — multiple large messages in sequence
// ===========================================================================

#[tokio::test]
async fn sdu_segmentation_multiple_large_messages() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");

    let protos = [MiniProtocolNum::BLOCK_FETCH];

    // Create several CBOR bstr payloads of different sizes.
    fn make_bstr(body_len: usize) -> Vec<u8> {
        let mut payload = Vec::with_capacity(3 + body_len);
        payload.push(0x59);
        payload.extend_from_slice(&(body_len as u16).to_be_bytes());
        for i in 0..body_len {
            payload.push((i % 251) as u8);
        }
        payload
    }

    let messages = vec![
        make_bstr(MAX_SEGMENT_SIZE + 500),     // spans 2 segments
        make_bstr(MAX_SEGMENT_SIZE * 2 + 100), // spans 3 segments
        make_bstr(100),                        // fits in 1 segment
        make_bstr(MAX_SEGMENT_SIZE * 4),       // spans 5 segments
    ];

    let send_messages = messages.clone();
    let client_handle = tokio::spawn(async move {
        let stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
        let (mut handles, mux) = start_mux(stream, MiniProtocolDir::Initiator, &protos, 8);
        let handle = handles.remove(&MiniProtocolNum::BLOCK_FETCH).expect("handle");
        let ch = MessageChannel::new(handle);
        for msg in &send_messages {
            ch.send(msg.clone()).await.expect("send");
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        mux.abort();
    });

    let (stream, _) = listener.accept().await.expect("accept");
    let (mut handles, mux) = start_mux(stream, MiniProtocolDir::Responder, &protos, 8);
    let handle = handles.remove(&MiniProtocolNum::BLOCK_FETCH).expect("handle");
    let mut ch = MessageChannel::new(handle);
    for (i, expected) in messages.iter().enumerate() {
        let received = ch.recv().await.unwrap_or_else(|| panic!("recv message {i}"));
        assert_eq!(received.len(), expected.len(), "message {i} length mismatch");
        assert_eq!(&received, expected, "message {i} content mismatch");
    }
    mux.abort();

    client_handle.await.expect("client task");
}

// ===========================================================================
// SDU segmentation — interleaved protocols
// ===========================================================================

#[tokio::test]
async fn sdu_segmentation_interleaved_protocols() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");

    let protos = [MiniProtocolNum::CHAIN_SYNC, MiniProtocolNum::BLOCK_FETCH];

    // Two large bstr payloads, one per protocol.
    fn make_bstr(body_len: usize, fill: u8) -> Vec<u8> {
        let mut payload = Vec::with_capacity(3 + body_len);
        payload.push(0x59);
        payload.extend_from_slice(&(body_len as u16).to_be_bytes());
        payload.resize(3 + body_len, fill);
        payload
    }

    let cs_payload = make_bstr(MAX_SEGMENT_SIZE + 1000, 0x11);
    let bf_payload = make_bstr(MAX_SEGMENT_SIZE + 2000, 0x22);

    let cs_send = cs_payload.clone();
    let bf_send = bf_payload.clone();

    let client_handle = tokio::spawn(async move {
        let stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
        let (mut handles, mux) = start_mux(stream, MiniProtocolDir::Initiator, &protos, 8);
        let cs_handle = handles.remove(&MiniProtocolNum::CHAIN_SYNC).expect("cs");
        let bf_handle = handles.remove(&MiniProtocolNum::BLOCK_FETCH).expect("bf");
        let cs_ch = MessageChannel::new(cs_handle);
        let bf_ch = MessageChannel::new(bf_handle);

        cs_ch.send(cs_send).await.expect("send cs");
        bf_ch.send(bf_send).await.expect("send bf");

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        mux.abort();
    });

    let (stream, _) = listener.accept().await.expect("accept");
    let (mut handles, mux) = start_mux(stream, MiniProtocolDir::Responder, &protos, 8);
    let cs_handle = handles.remove(&MiniProtocolNum::CHAIN_SYNC).expect("cs");
    let bf_handle = handles.remove(&MiniProtocolNum::BLOCK_FETCH).expect("bf");
    let mut cs_ch = MessageChannel::new(cs_handle);
    let mut bf_ch = MessageChannel::new(bf_handle);

    let cs_received = cs_ch.recv().await.expect("recv cs");
    let bf_received = bf_ch.recv().await.expect("recv bf");

    assert_eq!(cs_received, cs_payload);
    assert_eq!(bf_received, bf_payload);

    mux.abort();
    client_handle.await.expect("client task");
}

// ===========================================================================
// CBOR item length — unit tests
// ===========================================================================

#[test]
fn cbor_item_length_unit_tests() {
    use yggdrasil_network::mux::cbor_item_length;

    // Unsigned integers.
    assert_eq!(cbor_item_length(&[0x00]), Some(1));          // uint 0
    assert_eq!(cbor_item_length(&[0x17]), Some(1));          // uint 23
    assert_eq!(cbor_item_length(&[0x18, 0x18]), Some(2));    // uint 24
    assert_eq!(cbor_item_length(&[0x19, 0x01, 0x00]), Some(3)); // uint 256

    // Byte strings.
    assert_eq!(cbor_item_length(&[0x43, 0xAA, 0xBB, 0xCC]), Some(4)); // bstr(3)
    assert_eq!(cbor_item_length(&[0x43, 0xAA, 0xBB]), None);          // incomplete bstr

    // Empty array.
    assert_eq!(cbor_item_length(&[0x80]), Some(1));          // array(0)

    // Array with elements.
    assert_eq!(cbor_item_length(&[0x82, 0x01, 0x02]), Some(3)); // [1, 2]

    // Nested arrays.
    // [1, [2, 3]]
    assert_eq!(cbor_item_length(&[0x82, 0x01, 0x82, 0x02, 0x03]), Some(5));

    // Map.
    assert_eq!(cbor_item_length(&[0xA1, 0x01, 0x02]), Some(3)); // {1: 2}

    // Tag.
    assert_eq!(cbor_item_length(&[0xC0, 0x01]), Some(2));       // tag(0, uint 1)

    // Incomplete data.
    assert_eq!(cbor_item_length(&[]), None);
    assert_eq!(cbor_item_length(&[0x82, 0x01]), None); // array(2) with only 1 element

    // Simple values.
    assert_eq!(cbor_item_length(&[0xF4]), Some(1)); // false
    assert_eq!(cbor_item_length(&[0xF5]), Some(1)); // true
    assert_eq!(cbor_item_length(&[0xF6]), Some(1)); // null

    // Extra bytes after complete value are NOT consumed.
    assert_eq!(cbor_item_length(&[0x01, 0x99]), Some(1)); // uint 1 + trailing byte
}

// ===========================================================================
// Peer connection — successful handshake
// ===========================================================================

fn mainnet_magic() -> u32 {
    764824073
}

fn mainnet_proposals() -> Vec<(HandshakeVersion, NodeToNodeVersionData)> {
    vec![(
        HandshakeVersion::V14,
        NodeToNodeVersionData {
            network_magic: mainnet_magic(),
            initiator_only_diffusion_mode: false,
            peer_sharing: 0,
            query: false,
        },
    )]
}

#[tokio::test]
async fn peer_connect_accept_happy_path() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");

    let server_handle = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let conn = peer_accept(stream, mainnet_magic(), &[HandshakeVersion::V14])
            .await
            .expect("accept handshake");
        assert_eq!(conn.version, HandshakeVersion::V14);
        assert_eq!(conn.version_data.network_magic, mainnet_magic());
        assert!(conn.protocols.contains_key(&MiniProtocolNum::CHAIN_SYNC));
        assert!(conn.protocols.contains_key(&MiniProtocolNum::BLOCK_FETCH));
        assert!(conn.protocols.contains_key(&MiniProtocolNum::TX_SUBMISSION));
        assert!(conn.protocols.contains_key(&MiniProtocolNum::KEEP_ALIVE));
        // Allow the mux writer to flush the AcceptVersion SDU before shutdown.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        conn.mux.abort();
    });

    let conn = peer_connect(addr, mainnet_proposals())
        .await
        .expect("connect handshake");
    assert_eq!(conn.version, HandshakeVersion::V14);
    assert_eq!(conn.version_data.network_magic, mainnet_magic());
    assert!(conn.protocols.contains_key(&MiniProtocolNum::CHAIN_SYNC));
    conn.mux.abort();

    server_handle.await.expect("server task");
}

// ===========================================================================
// Peer connection — data exchange after handshake
// ===========================================================================

#[tokio::test]
async fn peer_data_exchange_after_handshake() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");

    let server_handle = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let mut conn = peer_accept(stream, mainnet_magic(), &[HandshakeVersion::V14])
            .await
            .expect("accept handshake");

        // Server receives a KeepAlive ping from client.
        let ka = conn
            .protocols
            .get_mut(&MiniProtocolNum::KEEP_ALIVE)
            .expect("ka");
        let ping = ka.recv().await.expect("recv ping");
        assert_eq!(ping, vec![0xAA, 0xBB]);

        // Server sends a response.
        ka.send(vec![0xCC, 0xDD]).await.expect("send pong");

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        conn.mux.abort();
    });

    let mut conn = peer_connect(addr, mainnet_proposals())
        .await
        .expect("connect handshake");

    let ka = conn
        .protocols
        .get_mut(&MiniProtocolNum::KEEP_ALIVE)
        .expect("ka");

    // Client sends a KeepAlive ping.
    ka.send(vec![0xAA, 0xBB]).await.expect("send ping");

    // Client receives the server's response.
    let pong = ka.recv().await.expect("recv pong");
    assert_eq!(pong, vec![0xCC, 0xDD]);

    conn.mux.abort();
    server_handle.await.expect("server task");
}

// ===========================================================================
// Peer connection — handshake refused (wrong magic)
// ===========================================================================

#[tokio::test]
async fn peer_handshake_refused_wrong_magic() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");

    // Server expects mainnet magic.
    let server_handle = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let result = peer_accept(stream, mainnet_magic(), &[HandshakeVersion::V14]).await;
        assert!(result.is_err(), "accept should fail with wrong magic");
    });

    // Client proposes with wrong magic (testnet).
    let wrong_proposals = vec![(
        HandshakeVersion::V14,
        NodeToNodeVersionData {
            network_magic: 1097911063, // preprod magic, not mainnet
            initiator_only_diffusion_mode: false,
            peer_sharing: 0,
            query: false,
        },
    )];

    let result = peer_connect(addr, wrong_proposals).await;
    assert!(
        matches!(&result, Err(PeerError::Refused { .. })),
        "connect should get Refused"
    );

    server_handle.await.expect("server task");
}

// ===========================================================================
// Peer connection — version negotiation picks highest
// ===========================================================================

#[tokio::test]
async fn peer_version_negotiation_picks_highest() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");

    let vdata = NodeToNodeVersionData {
        network_magic: mainnet_magic(),
        initiator_only_diffusion_mode: false,
        peer_sharing: 0,
        query: false,
    };

    // Server supports V14 and V15.
    let server_handle = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let conn = peer_accept(
            stream,
            mainnet_magic(),
            &[HandshakeVersion::V14, HandshakeVersion::V15],
        )
        .await
        .expect("accept handshake");
        // Should have picked V15 (highest common version).
        assert_eq!(conn.version, HandshakeVersion::V15);
        // Allow the mux writer to flush the AcceptVersion SDU before shutdown.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        conn.mux.abort();
    });

    // Client proposes V14 and V15.
    let proposals = vec![
        (HandshakeVersion::V14, vdata.clone()),
        (HandshakeVersion::V15, vdata),
    ];

    let conn = peer_connect(addr, proposals)
        .await
        .expect("connect handshake");
    assert_eq!(conn.version, HandshakeVersion::V15);
    conn.mux.abort();

    server_handle.await.expect("server task");
}

// ===========================================================================
// ChainSync client driver tests
// ===========================================================================

/// Helper: set up a mux pair over TCP loopback with a single ChainSync
/// protocol, returning (client_handle, server_handle, client_mux, server_mux).
async fn chainsync_mux_pair() -> (
    yggdrasil_network::ProtocolHandle,
    yggdrasil_network::ProtocolHandle,
    yggdrasil_network::MuxHandle,
    yggdrasil_network::MuxHandle,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local addr");

    let client_stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let (server_stream, _) = listener.accept().await.expect("accept");

    let protocols = [MiniProtocolNum::CHAIN_SYNC];

    let (mut c_handles, c_mux) =
        start_mux(client_stream, MiniProtocolDir::Initiator, &protocols, 16);
    let (mut s_handles, s_mux) =
        start_mux(server_stream, MiniProtocolDir::Responder, &protocols, 16);

    let c_handle = c_handles
        .remove(&MiniProtocolNum::CHAIN_SYNC)
        .expect("client chain_sync handle");
    let s_handle = s_handles
        .remove(&MiniProtocolNum::CHAIN_SYNC)
        .expect("server chain_sync handle");

    (c_handle, s_handle, c_mux, s_mux)
}

#[tokio::test]
async fn chainsync_client_request_next_roll_forward() {
    let (c_handle, s_handle, c_mux, s_mux) = chainsync_mux_pair().await;
    let mut client = ChainSyncClient::new(c_handle);

    let server = tokio::spawn(async move {
        let mut sh = s_handle;
        let raw = sh.recv().await.expect("recv request");
        let msg = ChainSyncMessage::from_cbor(&raw).expect("decode");
        assert_eq!(msg, ChainSyncMessage::MsgRequestNext);

        let reply = ChainSyncMessage::MsgRollForward {
            header: b"header-1".to_vec(),
            tip: b"tip-1".to_vec(),
        };
        sh.send(reply.to_cbor()).await.expect("send reply");
    });

    let resp = client.request_next().await.expect("request_next");
    assert_eq!(
        resp,
        NextResponse::RollForward {
            header: b"header-1".to_vec(),
            tip: b"tip-1".to_vec(),
        }
    );
    assert_eq!(client.state(), ChainSyncState::StIdle);

    server.await.expect("server task");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    c_mux.abort();
    s_mux.abort();
}

#[tokio::test]
async fn chainsync_client_request_next_roll_backward() {
    let (c_handle, s_handle, c_mux, s_mux) = chainsync_mux_pair().await;
    let mut client = ChainSyncClient::new(c_handle);

    let server = tokio::spawn(async move {
        let mut sh = s_handle;
        let _raw = sh.recv().await.expect("recv request");
        let reply = ChainSyncMessage::MsgRollBackward {
            point: b"point-0".to_vec(),
            tip: b"tip-0".to_vec(),
        };
        sh.send(reply.to_cbor()).await.expect("send reply");
    });

    let resp = client.request_next().await.expect("request_next");
    assert_eq!(
        resp,
        NextResponse::RollBackward {
            point: b"point-0".to_vec(),
            tip: b"tip-0".to_vec(),
        }
    );

    server.await.expect("server task");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    c_mux.abort();
    s_mux.abort();
}

#[tokio::test]
async fn chainsync_client_request_next_await_reply() {
    let (c_handle, s_handle, c_mux, s_mux) = chainsync_mux_pair().await;
    let mut client = ChainSyncClient::new(c_handle);

    let server = tokio::spawn(async move {
        let mut sh = s_handle;
        let _raw = sh.recv().await.expect("recv request");

        // Send MsgAwaitReply first, then MsgRollForward.
        let await_msg = ChainSyncMessage::MsgAwaitReply;
        sh.send(await_msg.to_cbor()).await.expect("send await");

        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        let reply = ChainSyncMessage::MsgRollForward {
            header: b"awaited-header".to_vec(),
            tip: b"awaited-tip".to_vec(),
        };
        sh.send(reply.to_cbor()).await.expect("send reply");
    });

    let resp = client.request_next().await.expect("request_next with await");
    assert_eq!(
        resp,
        NextResponse::AwaitRollForward {
            header: b"awaited-header".to_vec(),
            tip: b"awaited-tip".to_vec(),
        }
    );
    assert_eq!(client.state(), ChainSyncState::StIdle);

    server.await.expect("server task");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    c_mux.abort();
    s_mux.abort();
}

#[tokio::test]
async fn chainsync_client_request_next_typed_decodes_points() {
    let (c_handle, s_handle, c_mux, s_mux) = chainsync_mux_pair().await;
    let mut client = ChainSyncClient::new(c_handle);
    let point = Point::BlockPoint(SlotNo(12), HeaderHash([0x12; 32]));
    let tip = Point::BlockPoint(SlotNo(15), HeaderHash([0x15; 32]));

    let server = tokio::spawn(async move {
        let mut sh = s_handle;
        let _raw = sh.recv().await.expect("recv request");
        let reply = ChainSyncMessage::MsgRollBackward {
            point: point.to_cbor_bytes(),
            tip: tip.to_cbor_bytes(),
        };
        sh.send(reply.to_cbor()).await.expect("send reply");
    });

    let resp = client.request_next_typed().await.expect("request_next_typed");
    assert_eq!(
        resp,
        TypedNextResponse::RollBackward { point, tip }
    );

    server.await.expect("server task");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    c_mux.abort();
    s_mux.abort();
}

#[tokio::test]
async fn chainsync_client_find_intersect_found() {
    let (c_handle, s_handle, c_mux, s_mux) = chainsync_mux_pair().await;
    let mut client = ChainSyncClient::new(c_handle);

    let server = tokio::spawn(async move {
        let mut sh = s_handle;
        let raw = sh.recv().await.expect("recv find_intersect");
        let msg = ChainSyncMessage::from_cbor(&raw).expect("decode");
        match msg {
            ChainSyncMessage::MsgFindIntersect { points } => {
                assert_eq!(points.len(), 2);
            }
            _ => panic!("expected MsgFindIntersect"),
        }

        let reply = ChainSyncMessage::MsgIntersectFound {
            point: b"found-point".to_vec(),
            tip: b"found-tip".to_vec(),
        };
        sh.send(reply.to_cbor()).await.expect("send reply");
    });

    let resp = client
        .find_intersect(vec![b"pt-a".to_vec(), b"pt-b".to_vec()])
        .await
        .expect("find_intersect");
    assert_eq!(
        resp,
        IntersectResponse::Found {
            point: b"found-point".to_vec(),
            tip: b"found-tip".to_vec(),
        }
    );
    assert_eq!(client.state(), ChainSyncState::StIdle);

    server.await.expect("server task");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    c_mux.abort();
    s_mux.abort();
}

#[tokio::test]
async fn chainsync_client_find_intersect_not_found() {
    let (c_handle, s_handle, c_mux, s_mux) = chainsync_mux_pair().await;
    let mut client = ChainSyncClient::new(c_handle);

    let server = tokio::spawn(async move {
        let mut sh = s_handle;
        let _raw = sh.recv().await.expect("recv");
        let reply = ChainSyncMessage::MsgIntersectNotFound {
            tip: b"not-found-tip".to_vec(),
        };
        sh.send(reply.to_cbor()).await.expect("send reply");
    });

    let resp = client
        .find_intersect(vec![b"nonexistent".to_vec()])
        .await
        .expect("find_intersect");
    assert_eq!(
        resp,
        IntersectResponse::NotFound {
            tip: b"not-found-tip".to_vec(),
        }
    );

    server.await.expect("server task");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    c_mux.abort();
    s_mux.abort();
}

#[tokio::test]
async fn chainsync_client_find_intersect_points_decodes_points() {
    let (c_handle, s_handle, c_mux, s_mux) = chainsync_mux_pair().await;
    let mut client = ChainSyncClient::new(c_handle);
    let wanted = Point::BlockPoint(SlotNo(99), HeaderHash([0x99; 32]));
    let tip = Point::BlockPoint(SlotNo(120), HeaderHash([0xAB; 32]));

    let server = tokio::spawn(async move {
        let mut sh = s_handle;
        let raw = sh.recv().await.expect("recv find_intersect");
        let msg = ChainSyncMessage::from_cbor(&raw).expect("decode");
        match msg {
            ChainSyncMessage::MsgFindIntersect { points } => {
                assert_eq!(points, vec![wanted.to_cbor_bytes(), Point::Origin.to_cbor_bytes()]);
            }
            _ => panic!("expected MsgFindIntersect"),
        }

        let reply = ChainSyncMessage::MsgIntersectFound {
            point: wanted.to_cbor_bytes(),
            tip: tip.to_cbor_bytes(),
        };
        sh.send(reply.to_cbor()).await.expect("send reply");
    });

    let resp = client
        .find_intersect_points(vec![wanted, Point::Origin])
        .await
        .expect("find_intersect_points");
    assert_eq!(resp, TypedIntersectResponse::Found { point: wanted, tip });

    server.await.expect("server task");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    c_mux.abort();
    s_mux.abort();
}

#[tokio::test]
async fn chainsync_client_done() {
    let (c_handle, s_handle, c_mux, s_mux) = chainsync_mux_pair().await;
    let client = ChainSyncClient::new(c_handle);

    let server = tokio::spawn(async move {
        let mut sh = s_handle;
        let raw = sh.recv().await.expect("recv done");
        let msg = ChainSyncMessage::from_cbor(&raw).expect("decode");
        assert_eq!(msg, ChainSyncMessage::MsgDone);
    });

    client.done().await.expect("done");

    server.await.expect("server task");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    c_mux.abort();
    s_mux.abort();
}

#[tokio::test]
async fn chainsync_client_full_sync_sequence() {
    let (c_handle, s_handle, c_mux, s_mux) = chainsync_mux_pair().await;
    let mut client = ChainSyncClient::new(c_handle);

    let server = tokio::spawn(async move {
        let mut sh = s_handle;

        // 1. FindIntersect
        let _raw = sh.recv().await.expect("recv find_intersect");
        let reply = ChainSyncMessage::MsgIntersectFound {
            point: b"genesis".to_vec(),
            tip: b"tip-3".to_vec(),
        };
        sh.send(reply.to_cbor()).await.expect("send intersect");

        // 2. RequestNext -> RollForward
        let _raw = sh.recv().await.expect("recv request1");
        let reply = ChainSyncMessage::MsgRollForward {
            header: b"block-1".to_vec(),
            tip: b"tip-3".to_vec(),
        };
        sh.send(reply.to_cbor()).await.expect("send rf1");

        // 3. RequestNext -> RollForward
        let _raw = sh.recv().await.expect("recv request2");
        let reply = ChainSyncMessage::MsgRollForward {
            header: b"block-2".to_vec(),
            tip: b"tip-3".to_vec(),
        };
        sh.send(reply.to_cbor()).await.expect("send rf2");

        // 4. RequestNext -> AwaitReply -> RollForward
        let _raw = sh.recv().await.expect("recv request3");
        sh.send(ChainSyncMessage::MsgAwaitReply.to_cbor())
            .await
            .expect("send await");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let reply = ChainSyncMessage::MsgRollForward {
            header: b"block-3".to_vec(),
            tip: b"tip-3".to_vec(),
        };
        sh.send(reply.to_cbor()).await.expect("send rf3");

        // 5. Done
        let _raw = sh.recv().await.expect("recv done");
    });

    // Client side: full sync sequence.
    let intersect = client
        .find_intersect(vec![b"genesis".to_vec()])
        .await
        .expect("find_intersect");
    assert!(matches!(intersect, IntersectResponse::Found { .. }));

    let r1 = client.request_next().await.expect("request 1");
    assert!(matches!(r1, NextResponse::RollForward { .. }));

    let r2 = client.request_next().await.expect("request 2");
    assert!(matches!(r2, NextResponse::RollForward { .. }));

    let r3 = client.request_next().await.expect("request 3");
    assert!(matches!(r3, NextResponse::AwaitRollForward { .. }));

    client.done().await.expect("done");

    server.await.expect("server task");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    c_mux.abort();
    s_mux.abort();
}

// ===========================================================================
// BlockFetch client driver tests
// ===========================================================================

/// Helper: set up a mux pair over TCP loopback with a single BlockFetch
/// protocol, returning (client_handle, server_handle, client_mux, server_mux).
async fn blockfetch_mux_pair() -> (
    yggdrasil_network::ProtocolHandle,
    yggdrasil_network::ProtocolHandle,
    yggdrasil_network::MuxHandle,
    yggdrasil_network::MuxHandle,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local addr");

    let client_stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let (server_stream, _) = listener.accept().await.expect("accept");

    let protocols = [MiniProtocolNum::BLOCK_FETCH];

    let (mut c_handles, c_mux) =
        start_mux(client_stream, MiniProtocolDir::Initiator, &protocols, 16);
    let (mut s_handles, s_mux) =
        start_mux(server_stream, MiniProtocolDir::Responder, &protocols, 16);

    let c_handle = c_handles
        .remove(&MiniProtocolNum::BLOCK_FETCH)
        .expect("client block_fetch handle");
    let s_handle = s_handles
        .remove(&MiniProtocolNum::BLOCK_FETCH)
        .expect("server block_fetch handle");

    (c_handle, s_handle, c_mux, s_mux)
}

#[tokio::test]
async fn blockfetch_client_request_range_no_blocks() {
    let (c_handle, s_handle, c_mux, s_mux) = blockfetch_mux_pair().await;
    let mut client = BlockFetchClient::new(c_handle);

    let server = tokio::spawn(async move {
        let mut sh = s_handle;
        let raw = sh.recv().await.expect("recv request_range");
        let msg = BlockFetchMessage::from_cbor(&raw).expect("decode");
        assert!(matches!(msg, BlockFetchMessage::MsgRequestRange(_)));

        sh.send(BlockFetchMessage::MsgNoBlocks.to_cbor())
            .await
            .expect("send no_blocks");
    });

    let resp = client
        .request_range(ChainRange {
            lower: b"pt-a".to_vec(),
            upper: b"pt-b".to_vec(),
        })
        .await
        .expect("request_range");
    assert_eq!(resp, BatchResponse::NoBlocks);
    assert_eq!(client.state(), BlockFetchState::StIdle);

    server.await.expect("server task");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    c_mux.abort();
    s_mux.abort();
}

#[tokio::test]
async fn blockfetch_client_request_range_single_block() {
    let (c_handle, s_handle, c_mux, s_mux) = blockfetch_mux_pair().await;
    let mut client = BlockFetchClient::new(c_handle);

    let server = tokio::spawn(async move {
        let mut sh = s_handle;
        let _raw = sh.recv().await.expect("recv");

        sh.send(BlockFetchMessage::MsgStartBatch.to_cbor())
            .await
            .expect("send start_batch");
        sh.send(
            BlockFetchMessage::MsgBlock {
                block: b"block-data-1".to_vec(),
            }
            .to_cbor(),
        )
        .await
        .expect("send block");
        sh.send(BlockFetchMessage::MsgBatchDone.to_cbor())
            .await
            .expect("send batch_done");
    });

    let resp = client
        .request_range(ChainRange {
            lower: b"a".to_vec(),
            upper: b"b".to_vec(),
        })
        .await
        .expect("request_range");
    assert_eq!(resp, BatchResponse::StartedBatch);

    let blk = client.recv_block().await.expect("recv_block");
    assert_eq!(blk, Some(b"block-data-1".to_vec()));

    let done = client.recv_block().await.expect("recv_block batch_done");
    assert_eq!(done, None);
    assert_eq!(client.state(), BlockFetchState::StIdle);

    server.await.expect("server task");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    c_mux.abort();
    s_mux.abort();
}

#[tokio::test]
async fn blockfetch_client_request_range_points_encodes_points() {
    let (c_handle, s_handle, c_mux, s_mux) = blockfetch_mux_pair().await;
    let mut client = BlockFetchClient::new(c_handle);
    let lower = Point::Origin;
    let upper = Point::BlockPoint(SlotNo(50), HeaderHash([0x50; 32]));

    let server = tokio::spawn(async move {
        let mut sh = s_handle;
        let raw = sh.recv().await.expect("recv request_range");
        let msg = BlockFetchMessage::from_cbor(&raw).expect("decode");
        match msg {
            BlockFetchMessage::MsgRequestRange(range) => {
                assert_eq!(range.lower, lower.to_cbor_bytes());
                assert_eq!(range.upper, upper.to_cbor_bytes());
            }
            other => panic!("unexpected request: {other:?}"),
        }

        sh.send(BlockFetchMessage::MsgNoBlocks.to_cbor())
            .await
            .expect("send no_blocks");
    });

    let resp = client
        .request_range_points(lower, upper)
        .await
        .expect("request_range_points");
    assert_eq!(resp, BatchResponse::NoBlocks);

    server.await.expect("server task");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    c_mux.abort();
    s_mux.abort();
}

#[tokio::test]
async fn blockfetch_client_request_range_multiple_blocks() {
    let (c_handle, s_handle, c_mux, s_mux) = blockfetch_mux_pair().await;
    let mut client = BlockFetchClient::new(c_handle);

    let server = tokio::spawn(async move {
        let mut sh = s_handle;
        let _raw = sh.recv().await.expect("recv");

        sh.send(BlockFetchMessage::MsgStartBatch.to_cbor())
            .await
            .expect("start_batch");
        for i in 0u8..5 {
            sh.send(
                BlockFetchMessage::MsgBlock {
                    block: vec![i; 16],
                }
                .to_cbor(),
            )
            .await
            .expect("send block");
        }
        sh.send(BlockFetchMessage::MsgBatchDone.to_cbor())
            .await
            .expect("batch_done");
    });

    let resp = client
        .request_range(ChainRange {
            lower: b"lo".to_vec(),
            upper: b"hi".to_vec(),
        })
        .await
        .expect("request_range");
    assert_eq!(resp, BatchResponse::StartedBatch);

    let mut blocks = Vec::new();
    while let Some(b) = client.recv_block().await.expect("recv_block") {
        blocks.push(b);
    }
    assert_eq!(blocks.len(), 5);
    for (i, b) in blocks.iter().enumerate() {
        assert_eq!(b, &vec![i as u8; 16]);
    }
    assert_eq!(client.state(), BlockFetchState::StIdle);

    server.await.expect("server task");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    c_mux.abort();
    s_mux.abort();
}

#[tokio::test]
async fn blockfetch_client_driver_done() {
    let (c_handle, s_handle, c_mux, s_mux) = blockfetch_mux_pair().await;
    let client = BlockFetchClient::new(c_handle);

    let server = tokio::spawn(async move {
        let mut sh = s_handle;
        let raw = sh.recv().await.expect("recv done");
        let msg = BlockFetchMessage::from_cbor(&raw).expect("decode");
        assert_eq!(msg, BlockFetchMessage::MsgClientDone);
    });

    client.done().await.expect("done");

    server.await.expect("server task");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    c_mux.abort();
    s_mux.abort();
}

#[tokio::test]
async fn blockfetch_client_multi_range_session() {
    let (c_handle, s_handle, c_mux, s_mux) = blockfetch_mux_pair().await;
    let mut client = BlockFetchClient::new(c_handle);

    let server = tokio::spawn(async move {
        let mut sh = s_handle;

        // Range 1: start batch with 2 blocks.
        let _raw = sh.recv().await.expect("recv range1");
        sh.send(BlockFetchMessage::MsgStartBatch.to_cbor())
            .await
            .expect("start_batch 1");
        sh.send(
            BlockFetchMessage::MsgBlock {
                block: b"blk-1a".to_vec(),
            }
            .to_cbor(),
        )
        .await
        .expect("block 1a");
        sh.send(
            BlockFetchMessage::MsgBlock {
                block: b"blk-1b".to_vec(),
            }
            .to_cbor(),
        )
        .await
        .expect("block 1b");
        sh.send(BlockFetchMessage::MsgBatchDone.to_cbor())
            .await
            .expect("batch_done 1");

        // Range 2: no blocks.
        let _raw = sh.recv().await.expect("recv range2");
        sh.send(BlockFetchMessage::MsgNoBlocks.to_cbor())
            .await
            .expect("no_blocks 2");

        // Range 3: single block.
        let _raw = sh.recv().await.expect("recv range3");
        sh.send(BlockFetchMessage::MsgStartBatch.to_cbor())
            .await
            .expect("start_batch 3");
        sh.send(
            BlockFetchMessage::MsgBlock {
                block: b"blk-3".to_vec(),
            }
            .to_cbor(),
        )
        .await
        .expect("block 3");
        sh.send(BlockFetchMessage::MsgBatchDone.to_cbor())
            .await
            .expect("batch_done 3");

        // Done.
        let raw = sh.recv().await.expect("recv done");
        let msg = BlockFetchMessage::from_cbor(&raw).expect("decode");
        assert_eq!(msg, BlockFetchMessage::MsgClientDone);
    });

    // Range 1: 2 blocks.
    let r1 = client
        .request_range(ChainRange {
            lower: b"lo1".to_vec(),
            upper: b"hi1".to_vec(),
        })
        .await
        .expect("range 1");
    assert_eq!(r1, BatchResponse::StartedBatch);
    assert_eq!(
        client.recv_block().await.expect("blk"),
        Some(b"blk-1a".to_vec())
    );
    assert_eq!(
        client.recv_block().await.expect("blk"),
        Some(b"blk-1b".to_vec())
    );
    assert_eq!(client.recv_block().await.expect("done"), None);

    // Range 2: no blocks.
    let r2 = client
        .request_range(ChainRange {
            lower: b"lo2".to_vec(),
            upper: b"hi2".to_vec(),
        })
        .await
        .expect("range 2");
    assert_eq!(r2, BatchResponse::NoBlocks);

    // Range 3: 1 block.
    let r3 = client
        .request_range(ChainRange {
            lower: b"lo3".to_vec(),
            upper: b"hi3".to_vec(),
        })
        .await
        .expect("range 3");
    assert_eq!(r3, BatchResponse::StartedBatch);
    assert_eq!(
        client.recv_block().await.expect("blk"),
        Some(b"blk-3".to_vec())
    );
    assert_eq!(client.recv_block().await.expect("done"), None);

    client.done().await.expect("client done");

    server.await.expect("server task");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    c_mux.abort();
    s_mux.abort();
}

// ===========================================================================
// KeepAlive client driver tests
// ===========================================================================

/// Helper: set up a mux pair over TCP loopback with a single KeepAlive
/// protocol.
async fn keepalive_mux_pair() -> (
    yggdrasil_network::ProtocolHandle,
    yggdrasil_network::ProtocolHandle,
    yggdrasil_network::MuxHandle,
    yggdrasil_network::MuxHandle,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local addr");

    let client_stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let (server_stream, _) = listener.accept().await.expect("accept");

    let protocols = [MiniProtocolNum::KEEP_ALIVE];

    let (mut c_handles, c_mux) =
        start_mux(client_stream, MiniProtocolDir::Initiator, &protocols, 16);
    let (mut s_handles, s_mux) =
        start_mux(server_stream, MiniProtocolDir::Responder, &protocols, 16);

    let c_handle = c_handles
        .remove(&MiniProtocolNum::KEEP_ALIVE)
        .expect("client keep_alive handle");
    let s_handle = s_handles
        .remove(&MiniProtocolNum::KEEP_ALIVE)
        .expect("server keep_alive handle");

    (c_handle, s_handle, c_mux, s_mux)
}

#[tokio::test]
async fn keepalive_client_single_ping() {
    let (c_handle, s_handle, c_mux, s_mux) = keepalive_mux_pair().await;
    let mut client = KeepAliveClient::new(c_handle);

    let server = tokio::spawn(async move {
        let mut sh = s_handle;
        let raw = sh.recv().await.expect("recv keep_alive");
        let msg = KeepAliveMessage::from_cbor(&raw).expect("decode");
        match msg {
            KeepAliveMessage::MsgKeepAlive { cookie } => {
                let reply = KeepAliveMessage::MsgKeepAliveResponse { cookie };
                sh.send(reply.to_cbor()).await.expect("send response");
            }
            _ => panic!("expected MsgKeepAlive"),
        }
    });

    client.keep_alive(42).await.expect("keep_alive");
    assert_eq!(client.state(), KeepAliveState::StClient);

    server.await.expect("server task");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    c_mux.abort();
    s_mux.abort();
}

#[tokio::test]
async fn keepalive_client_multiple_pings() {
    let (c_handle, s_handle, c_mux, s_mux) = keepalive_mux_pair().await;
    let mut client = KeepAliveClient::new(c_handle);

    let server = tokio::spawn(async move {
        let mut sh = s_handle;
        for _ in 0..3 {
            let raw = sh.recv().await.expect("recv");
            let msg = KeepAliveMessage::from_cbor(&raw).expect("decode");
            if let KeepAliveMessage::MsgKeepAlive { cookie } = msg {
                let reply = KeepAliveMessage::MsgKeepAliveResponse { cookie };
                sh.send(reply.to_cbor()).await.expect("send response");
            } else {
                panic!("expected MsgKeepAlive");
            }
        }
        // Expect MsgDone.
        let raw = sh.recv().await.expect("recv done");
        let msg = KeepAliveMessage::from_cbor(&raw).expect("decode");
        assert_eq!(msg, KeepAliveMessage::MsgDone);
    });

    for i in 0..3 {
        client.keep_alive(100 + i).await.expect("keep_alive");
    }
    client.done().await.expect("done");

    server.await.expect("server task");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    c_mux.abort();
    s_mux.abort();
}

#[tokio::test]
async fn keepalive_client_cookie_mismatch() {
    let (c_handle, s_handle, c_mux, s_mux) = keepalive_mux_pair().await;
    let mut client = KeepAliveClient::new(c_handle);

    let server = tokio::spawn(async move {
        let mut sh = s_handle;
        let raw = sh.recv().await.expect("recv");
        let _msg = KeepAliveMessage::from_cbor(&raw).expect("decode");
        // Echo back a different cookie.
        let reply = KeepAliveMessage::MsgKeepAliveResponse { cookie: 9999 };
        sh.send(reply.to_cbor()).await.expect("send response");
    });

    let err = client.keep_alive(1234).await;
    assert!(err.is_err());
    let err_str = format!("{}", err.expect_err("should be cookie mismatch"));
    assert!(err_str.contains("cookie mismatch"), "got: {err_str}");

    server.await.expect("server task");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    c_mux.abort();
    s_mux.abort();
}

#[tokio::test]
async fn keepalive_client_driver_done() {
    let (c_handle, s_handle, c_mux, s_mux) = keepalive_mux_pair().await;
    let client = KeepAliveClient::new(c_handle);

    let server = tokio::spawn(async move {
        let mut sh = s_handle;
        let raw = sh.recv().await.expect("recv done");
        let msg = KeepAliveMessage::from_cbor(&raw).expect("decode");
        assert_eq!(msg, KeepAliveMessage::MsgDone);
    });

    client.done().await.expect("done");

    server.await.expect("server task");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    c_mux.abort();
    s_mux.abort();
}

// ===========================================================================
// TxSubmission client driver tests
// ===========================================================================

/// Helper: set up a mux pair over TCP loopback with a single TxSubmission
/// protocol.
async fn txsubmission_mux_pair() -> (
    yggdrasil_network::ProtocolHandle,
    yggdrasil_network::ProtocolHandle,
    yggdrasil_network::MuxHandle,
    yggdrasil_network::MuxHandle,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("local addr");

    let client_stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let (server_stream, _) = listener.accept().await.expect("accept");

    let protocols = [MiniProtocolNum::TX_SUBMISSION];

    let (mut c_handles, c_mux) =
        start_mux(client_stream, MiniProtocolDir::Initiator, &protocols, 16);
    let (mut s_handles, s_mux) =
        start_mux(server_stream, MiniProtocolDir::Responder, &protocols, 16);

    let c_handle = c_handles
        .remove(&MiniProtocolNum::TX_SUBMISSION)
        .expect("client tx_submission handle");
    let s_handle = s_handles
        .remove(&MiniProtocolNum::TX_SUBMISSION)
        .expect("server tx_submission handle");

    (c_handle, s_handle, c_mux, s_mux)
}

#[tokio::test]
async fn txsubmission_client_init_and_reply_txids() {
    let (c_handle, s_handle, c_mux, s_mux) = txsubmission_mux_pair().await;
    let mut client = TxSubmissionClient::new(c_handle);

    let server = tokio::spawn(async move {
        let mut sh = s_handle;

        // Receive MsgInit.
        let raw = sh.recv().await.expect("recv init");
        let msg = TxSubmissionMessage::from_cbor(&raw).expect("decode");
        assert_eq!(msg, TxSubmissionMessage::MsgInit);

        // Send MsgRequestTxIds (non-blocking).
        let req = TxSubmissionMessage::MsgRequestTxIds {
            blocking: false,
            ack: 0,
            req: 3,
        };
        sh.send(req.to_cbor()).await.expect("send request_tx_ids");

        // Receive MsgReplyTxIds.
        let raw = sh.recv().await.expect("recv reply");
        let msg = TxSubmissionMessage::from_cbor(&raw).expect("decode");
        match msg {
            TxSubmissionMessage::MsgReplyTxIds { txids } => {
                assert_eq!(txids.len(), 2);
                assert_eq!(txids[0].txid, b"tx-1");
                assert_eq!(txids[0].size, 100);
                assert_eq!(txids[1].txid, b"tx-2");
                assert_eq!(txids[1].size, 200);
            }
            _ => panic!("expected MsgReplyTxIds"),
        }
    });

    client.init().await.expect("init");
    assert_eq!(client.state(), TxSubmissionState::StIdle);

    let req = client.recv_request().await.expect("recv_request");
    assert_eq!(
        req,
        TxServerRequest::RequestTxIds {
            blocking: false,
            ack: 0,
            req: 3,
        }
    );

    client
        .reply_tx_ids(vec![
            TxIdAndSize {
                txid: b"tx-1".to_vec(),
                size: 100,
            },
            TxIdAndSize {
                txid: b"tx-2".to_vec(),
                size: 200,
            },
        ])
        .await
        .expect("reply_tx_ids");
    assert_eq!(client.state(), TxSubmissionState::StIdle);

    server.await.expect("server task");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    c_mux.abort();
    s_mux.abort();
}

#[tokio::test]
async fn txsubmission_client_reply_txs() {
    let (c_handle, s_handle, c_mux, s_mux) = txsubmission_mux_pair().await;
    let mut client = TxSubmissionClient::new(c_handle);

    let server = tokio::spawn(async move {
        let mut sh = s_handle;

        // MsgInit.
        let _raw = sh.recv().await.expect("recv init");

        // MsgRequestTxs.
        let req = TxSubmissionMessage::MsgRequestTxs {
            txids: vec![b"tx-a".to_vec(), b"tx-b".to_vec()],
        };
        sh.send(req.to_cbor()).await.expect("send request_txs");

        // MsgReplyTxs.
        let raw = sh.recv().await.expect("recv reply_txs");
        let msg = TxSubmissionMessage::from_cbor(&raw).expect("decode");
        match msg {
            TxSubmissionMessage::MsgReplyTxs { txs } => {
                assert_eq!(txs.len(), 2);
                assert_eq!(txs[0], b"body-a");
                assert_eq!(txs[1], b"body-b");
            }
            _ => panic!("expected MsgReplyTxs"),
        }
    });

    client.init().await.expect("init");

    let req = client.recv_request().await.expect("recv_request");
    assert!(matches!(req, TxServerRequest::RequestTxs { .. }));

    client
        .reply_txs(vec![b"body-a".to_vec(), b"body-b".to_vec()])
        .await
        .expect("reply_txs");

    server.await.expect("server task");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    c_mux.abort();
    s_mux.abort();
}

#[tokio::test]
async fn txsubmission_client_done_from_blocking() {
    let (c_handle, s_handle, c_mux, s_mux) = txsubmission_mux_pair().await;
    let mut client = TxSubmissionClient::new(c_handle);

    let server = tokio::spawn(async move {
        let mut sh = s_handle;

        // MsgInit.
        let _raw = sh.recv().await.expect("recv init");

        // Blocking MsgRequestTxIds.
        let req = TxSubmissionMessage::MsgRequestTxIds {
            blocking: true,
            ack: 0,
            req: 1,
        };
        sh.send(req.to_cbor()).await.expect("send");

        // Expect MsgDone.
        let raw = sh.recv().await.expect("recv done");
        let msg = TxSubmissionMessage::from_cbor(&raw).expect("decode");
        assert_eq!(msg, TxSubmissionMessage::MsgDone);
    });

    client.init().await.expect("init");

    let req = client.recv_request().await.expect("recv_request");
    assert!(matches!(
        req,
        TxServerRequest::RequestTxIds {
            blocking: true,
            ..
        }
    ));

    client.done().await.expect("done");

    server.await.expect("server task");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    c_mux.abort();
    s_mux.abort();
}

#[tokio::test]
async fn txsubmission_client_full_session() {
    let (c_handle, s_handle, c_mux, s_mux) = txsubmission_mux_pair().await;
    let mut client = TxSubmissionClient::new(c_handle);

    let server = tokio::spawn(async move {
        let mut sh = s_handle;

        // 1. MsgInit.
        let _raw = sh.recv().await.expect("recv init");

        // 2. Non-blocking MsgRequestTxIds.
        sh.send(
            TxSubmissionMessage::MsgRequestTxIds {
                blocking: false,
                ack: 0,
                req: 5,
            }
            .to_cbor(),
        )
        .await
        .expect("send");

        // 3. Receive MsgReplyTxIds.
        let raw = sh.recv().await.expect("recv reply_txids");
        let msg = TxSubmissionMessage::from_cbor(&raw).expect("decode");
        let txids = match msg {
            TxSubmissionMessage::MsgReplyTxIds { txids } => txids,
            _ => panic!("expected MsgReplyTxIds"),
        };
        assert_eq!(txids.len(), 1);

        // 4. MsgRequestTxs for that tx.
        sh.send(
            TxSubmissionMessage::MsgRequestTxs {
                txids: vec![txids[0].txid.clone()],
            }
            .to_cbor(),
        )
        .await
        .expect("send request_txs");

        // 5. Receive MsgReplyTxs.
        let raw = sh.recv().await.expect("recv reply_txs");
        let msg = TxSubmissionMessage::from_cbor(&raw).expect("decode");
        match msg {
            TxSubmissionMessage::MsgReplyTxs { txs } => {
                assert_eq!(txs.len(), 1);
                assert_eq!(txs[0], b"full-tx-body");
            }
            _ => panic!("expected MsgReplyTxs"),
        }

        // 6. Blocking MsgRequestTxIds (client will send MsgDone).
        sh.send(
            TxSubmissionMessage::MsgRequestTxIds {
                blocking: true,
                ack: 1,
                req: 1,
            }
            .to_cbor(),
        )
        .await
        .expect("send blocking");

        // 7. Expect MsgDone.
        let raw = sh.recv().await.expect("recv done");
        let msg = TxSubmissionMessage::from_cbor(&raw).expect("decode");
        assert_eq!(msg, TxSubmissionMessage::MsgDone);
    });

    // Client side.
    client.init().await.expect("init");

    // Non-blocking: reply with one tx id.
    let req = client.recv_request().await.expect("1");
    assert!(matches!(req, TxServerRequest::RequestTxIds { .. }));
    client
        .reply_tx_ids(vec![TxIdAndSize {
            txid: b"my-tx".to_vec(),
            size: 50,
        }])
        .await
        .expect("reply_tx_ids");

    // Fetch request: reply with the tx body.
    let req = client.recv_request().await.expect("2");
    assert!(matches!(req, TxServerRequest::RequestTxs { .. }));
    client
        .reply_txs(vec![b"full-tx-body".to_vec()])
        .await
        .expect("reply_txs");

    // Blocking: we have nothing, send Done.
    let req = client.recv_request().await.expect("3");
    assert!(matches!(
        req,
        TxServerRequest::RequestTxIds {
            blocking: true,
            ..
        }
    ));
    client.done().await.expect("done");

    server.await.expect("server task");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    c_mux.abort();
    s_mux.abort();
}
