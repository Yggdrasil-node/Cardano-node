use yggdrasil_network::{
    BlockFetchMessage, BlockFetchState, ChainRange, ChainSyncMessage,
    ChainSyncState, HandshakeMessage, HandshakeRequest, HandshakeState,
    HandshakeVersion, KeepAliveMessage, KeepAliveState,
    MiniProtocolDir, MiniProtocolNum, MuxChannel,
    NodeToNodeVersionData, RefuseReason, SduDecodeError, SduHeader,
    SDU_HEADER_SIZE,
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
