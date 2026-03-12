use yggdrasil_network::{
    Bearer, BearerError, BlockFetchMessage, BlockFetchState, ChainRange, ChainSyncMessage,
    ChainSyncState, HandshakeMessage, HandshakeRequest, HandshakeState,
    HandshakeVersion, KeepAliveMessage, KeepAliveState,
    MiniProtocolDir, MiniProtocolNum, MuxChannel,
    NodeToNodeVersionData, RefuseReason, Sdu, SduDecodeError, SduHeader,
    TcpBearer, TxIdAndSize, TxSubmissionMessage, TxSubmissionState,
    SDU_HEADER_SIZE,
    start_mux,
};

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
