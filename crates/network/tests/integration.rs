#![allow(clippy::unwrap_used)]
use yggdrasil_ledger::{
    AlonzoCompatibleSubmittedTx, AlonzoTxBody, AlonzoTxOut, BlockNo, CborDecode, CborEncode,
    HeaderHash, MultiEraSubmittedTx, Point, ShelleyBlock, ShelleyCompatibleSubmittedTx,
    ShelleyHeader, ShelleyHeaderBody, ShelleyOpCert, ShelleyTxBody, ShelleyTxIn, ShelleyTxOut,
    ShelleyVrfCert, ShelleyWitnessSet, SlotNo, Tip, TxId, Value,
};
use yggdrasil_network::{
    BatchResponse, Bearer, BearerError, BlockFetchClient, BlockFetchMessage, BlockFetchState,
    ChainRange, ChainSyncClient, ChainSyncMessage, ChainSyncState, DecodedHeaderNextResponse,
    HandshakeMessage, HandshakeRequest, HandshakeState, HandshakeVersion, IntersectResponse,
    KeepAliveClient, KeepAliveMessage, KeepAliveState, LocalRootConfig, LocalRootTargets,
    MAX_SEGMENT_SIZE, MessageChannel, MiniProtocolDir, MiniProtocolNum, MuxChannel, NextResponse,
    NodeToNodeVersionData, PeerAccessPoint, PeerAttemptState, PeerBootstrapTargets,
    PeerDiffusionMode, PeerError, PeerRegistry, PeerRegistryEntry, PeerSource, PeerStatus,
    RefuseReason, SDU_HEADER_SIZE, Sdu, SduDecodeError, SduHeader, TcpBearer, TxIdAndSize,
    TxServerRequest, TxSubmissionClient, TxSubmissionMessage, TxSubmissionState,
    TypedIntersectResponse, TypedNextResponse, peer_accept, peer_connect, start_mux,
};

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

fn sample_shelley_header() -> ShelleyHeader {
    ShelleyHeader {
        body: ShelleyHeaderBody {
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
        },
        signature: vec![0xDD; 448],
    }
}

fn sample_shelley_block_bytes() -> Vec<u8> {
    ShelleyBlock {
        header: sample_shelley_header(),
        transaction_bodies: vec![],
        transaction_witness_sets: vec![],
        transaction_metadata_set: std::collections::HashMap::new(),
    }
    .to_cbor_bytes()
}

fn sample_tx_id(seed: u8) -> TxId {
    TxId([seed; 32])
}

fn sample_shelley_submitted_tx(seed: u8) -> MultiEraSubmittedTx {
    MultiEraSubmittedTx::Shelley(ShelleyCompatibleSubmittedTx::new(
        ShelleyTxBody {
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
        Some(vec![0x81, seed]),
    ))
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
        true,
        Some(vec![0x81, seed.wrapping_add(1)]),
    ))
}

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
    let propose = HandshakeMessage::ProposeVersions(vec![(HandshakeVersion::V14, vdata.clone())]);
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

#[test]
fn handshake_propose_with_legacy_v7_two_element_version_data() {
    // V7 version data: [networkMagic, initiatorOnlyDiffusionMode] — 2 elements
    use yggdrasil_ledger::cbor::Encoder;
    let mut enc = Encoder::new();
    enc.array(2).unsigned(0); // ProposeVersions tag
    enc.map(1); // 1 version entry
    enc.unsigned(7); // version 7
    enc.array(2).unsigned(764824073).bool(false); // 2-element vdata
    let bytes = enc.into_bytes();
    let msg = HandshakeMessage::from_cbor(&bytes).expect("should decode V7 2-element vdata");
    match msg {
        HandshakeMessage::ProposeVersions(versions) => {
            assert_eq!(versions.len(), 1);
            assert_eq!(versions[0].0, HandshakeVersion(7));
            assert_eq!(versions[0].1.network_magic, 764824073);
            assert!(!versions[0].1.initiator_only_diffusion_mode);
            assert_eq!(versions[0].1.peer_sharing, 0); // default
            assert!(!versions[0].1.query); // default
        }
        _ => panic!("expected ProposeVersions"),
    }
}

#[test]
fn handshake_propose_with_v11_three_element_version_data() {
    // V11 version data: [networkMagic, initiatorOnlyDiffusionMode, peerSharing] — 3 elements
    use yggdrasil_ledger::cbor::Encoder;
    let mut enc = Encoder::new();
    enc.array(2).unsigned(0); // ProposeVersions tag
    enc.map(1); // 1 version entry
    enc.unsigned(11); // version 11
    enc.array(3).unsigned(764824073).bool(true).unsigned(1); // 3-element vdata
    let bytes = enc.into_bytes();
    let msg = HandshakeMessage::from_cbor(&bytes).expect("should decode V11 3-element vdata");
    match msg {
        HandshakeMessage::ProposeVersions(versions) => {
            assert_eq!(versions.len(), 1);
            assert_eq!(versions[0].0, HandshakeVersion(11));
            assert_eq!(versions[0].1.network_magic, 764824073);
            assert!(versions[0].1.initiator_only_diffusion_mode);
            assert_eq!(versions[0].1.peer_sharing, 1);
            assert!(!versions[0].1.query); // default
        }
        _ => panic!("expected ProposeVersions"),
    }
}

#[test]
fn handshake_propose_with_mixed_legacy_and_modern_versions() {
    // A peer proposes V10 (2-element) and V14 (4-element) simultaneously
    use yggdrasil_ledger::cbor::Encoder;
    let mut enc = Encoder::new();
    enc.array(2).unsigned(0); // ProposeVersions tag
    enc.map(2); // 2 version entries
    enc.unsigned(10); // version 10
    enc.array(2).unsigned(764824073).bool(false); // 2-element
    enc.unsigned(14); // version 14
    enc.array(4)
        .unsigned(764824073)
        .bool(false)
        .unsigned(1)
        .bool(false); // 4-element
    let bytes = enc.into_bytes();
    let msg = HandshakeMessage::from_cbor(&bytes).expect("should decode mixed version proposal");
    match msg {
        HandshakeMessage::ProposeVersions(versions) => {
            assert_eq!(versions.len(), 2);
            assert_eq!(versions[0].0, HandshakeVersion(10));
            assert_eq!(versions[0].1.peer_sharing, 0); // defaulted
            assert_eq!(versions[1].0, HandshakeVersion(14));
            assert_eq!(versions[1].1.peer_sharing, 1);
        }
        _ => panic!("expected ProposeVersions"),
    }
}

// ===========================================================================
// ChainSync state transitions
// ===========================================================================

#[test]
fn chainsync_happy_path_request_next_roll_forward() {
    let s = ChainSyncState::StIdle;
    let s = s
        .transition(&ChainSyncMessage::MsgRequestNext)
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
    let s = s
        .transition(&ChainSyncMessage::MsgRequestNext)
        .expect("MsgRequestNext from StIdle");
    let s = s
        .transition(&ChainSyncMessage::MsgAwaitReply)
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
    let s = s
        .transition(&ChainSyncMessage::MsgDone)
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

    let s = s
        .transition(&BlockFetchMessage::MsgStartBatch)
        .expect("MsgStartBatch from StBusy");
    assert_eq!(s, BlockFetchState::StStreaming);

    let s = s
        .transition(&BlockFetchMessage::MsgBlock { block: vec![0xAB] })
        .expect("MsgBlock from StStreaming");
    assert_eq!(s, BlockFetchState::StStreaming);

    let s = s
        .transition(&BlockFetchMessage::MsgBatchDone)
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
    let s = s
        .transition(&BlockFetchMessage::MsgNoBlocks)
        .expect("MsgNoBlocks from StBusy");
    assert_eq!(s, BlockFetchState::StIdle);
}

#[test]
fn blockfetch_client_done() {
    let s = BlockFetchState::StIdle;
    let s = s
        .transition(&BlockFetchMessage::MsgClientDone)
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
    assert_eq!(BlockFetchMessage::MsgBlock { block: vec![] }.wire_tag(), 4);
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
    assert_eq!(
        err.to_string(),
        "illegal keep-alive transition from StServer via MsgDone"
    );

    let err = KeepAliveState::StDone
        .transition(&KeepAliveMessage::MsgKeepAlive { cookie: 1 })
        .expect_err("MsgKeepAlive should be illegal from StDone");
    assert_eq!(
        err.to_string(),
        "illegal keep-alive transition from StDone via MsgKeepAlive"
    );
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

/// Wire-format parity with upstream
/// `Ouroboros.Network.Protocol.KeepAlive.Codec`:
/// `MsgKeepAlive=[0,c]`, `MsgKeepAliveResponse=[1,c]`, `MsgDone=[2]`.
///
/// Locks the on-wire tag mapping so a future codec refactor cannot
/// silently swap `MsgDone` and `MsgKeepAliveResponse` again.
#[test]
fn keepalive_cbor_wire_tags_match_upstream() {
    // [0, 0x2A] — MsgKeepAlive cookie=42
    assert_eq!(
        KeepAliveMessage::MsgKeepAlive { cookie: 42 }.to_cbor(),
        vec![0x82, 0x00, 0x18, 0x2A],
    );
    // [1, 0x2A] — MsgKeepAliveResponse cookie=42
    assert_eq!(
        KeepAliveMessage::MsgKeepAliveResponse { cookie: 42 }.to_cbor(),
        vec![0x82, 0x01, 0x18, 0x2A],
    );
    // [2] — MsgDone
    assert_eq!(KeepAliveMessage::MsgDone.to_cbor(), vec![0x81, 0x02]);

    // Round-trip decode of canonical upstream-shape bytes.
    assert_eq!(
        KeepAliveMessage::from_cbor(&[0x82, 0x00, 0x18, 0x2A]).unwrap(),
        KeepAliveMessage::MsgKeepAlive { cookie: 42 },
    );
    assert_eq!(
        KeepAliveMessage::from_cbor(&[0x82, 0x01, 0x18, 0x2A]).unwrap(),
        KeepAliveMessage::MsgKeepAliveResponse { cookie: 42 },
    );
    assert_eq!(
        KeepAliveMessage::from_cbor(&[0x81, 0x02]).unwrap(),
        KeepAliveMessage::MsgDone,
    );
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
    // header is inline CBOR (must be a valid CBOR item); tip is also inline CBOR
    let msg = ChainSyncMessage::MsgRollForward {
        header: vec![0x84, 0x01, 0x02, 0x03, 0x04],
        tip: vec![
            0x83, 0x18, 0x2A, 0x58, 0x20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x01,
        ], // [42, h'00..00', 1]
    };
    let decoded = ChainSyncMessage::from_cbor(&msg.to_cbor()).expect("decode");
    assert_eq!(msg, decoded);
}

#[test]
fn chainsync_cbor_roll_backward_round_trip() {
    // point and tip are inline CBOR
    let msg = ChainSyncMessage::MsgRollBackward {
        point: vec![0x80],           // [] (Origin)
        tip: vec![0x82, 0x01, 0x02], // [1, 2]
    };
    let decoded = ChainSyncMessage::from_cbor(&msg.to_cbor()).expect("decode");
    assert_eq!(msg, decoded);
}

#[test]
fn chainsync_cbor_find_intersect_round_trip() {
    // points are inline CBOR
    let msg = ChainSyncMessage::MsgFindIntersect {
        points: vec![vec![0x80], vec![0x82, 0x01, 0x02], vec![0x01]],
    };
    let decoded = ChainSyncMessage::from_cbor(&msg.to_cbor()).expect("decode");
    assert_eq!(msg, decoded);
}

#[test]
fn chainsync_cbor_intersect_found_round_trip() {
    // point and tip are inline CBOR
    let msg = ChainSyncMessage::MsgIntersectFound {
        point: vec![0x82, 0x01, 0x02], // [1, 2]
        tip: vec![0x80],               // []
    };
    let decoded = ChainSyncMessage::from_cbor(&msg.to_cbor()).expect("decode");
    assert_eq!(msg, decoded);
}

#[test]
fn chainsync_cbor_intersect_not_found_round_trip() {
    // tip is inline CBOR
    let msg = ChainSyncMessage::MsgIntersectNotFound {
        tip: vec![0x82, 0x03, 0x04], // [3, 4]
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
    // lower/upper are opaque point bytes (wrapped via TAG 24 on the wire)
    let msg = BlockFetchMessage::MsgRequestRange(ChainRange {
        lower: vec![0x82, 0x01, 0x02], // [1, 2]
        upper: vec![0x82, 0x03, 0x04], // [3, 4]
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
        (
            HandshakeVersion::V15,
            NodeToNodeVersionData {
                network_magic: 764824073,
                initiator_only_diffusion_mode: true,
                peer_sharing: 0,
                query: true,
            },
        ),
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
    let decoded =
        HandshakeMessage::from_cbor(&msg.to_cbor()).expect("decode Refuse VersionMismatch");
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
    let msg = HandshakeMessage::QueryReply(vec![(HandshakeVersion::V14, mainnet_version_data())]);
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
        .transition(&HandshakeMessage::ProposeVersions(vec![(
            HandshakeVersion::V14,
            mainnet_version_data(),
        )]))
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
        .transition(&HandshakeMessage::Refuse(RefuseReason::VersionMismatch(
            vec![],
        )))
        .expect("Refuse from StConfirm");
    assert_eq!(state, HandshakeState::StDone);
}

#[test]
fn handshake_transition_illegal_from_done() {
    let err = HandshakeState::StDone
        .transition(&HandshakeMessage::ProposeVersions(vec![]))
        .expect_err("ProposeVersions should be illegal from StDone");
    assert_eq!(
        err.to_string(),
        "illegal handshake transition from StDone via ProposeVersions"
    );
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
                txid: sample_tx_id(1),
                size: 100,
            }],
        })
        .expect("MsgReplyTxIds from StTxIds");
    assert_eq!(state, TxSubmissionState::StIdle);

    state = state
        .transition(&TxSubmissionMessage::MsgRequestTxs {
            txids: vec![sample_tx_id(1)],
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
                txid: sample_tx_id(0xDE),
                size: 256,
            },
            TxIdAndSize {
                txid: sample_tx_id(0xBE),
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
        txids: vec![sample_tx_id(1), sample_tx_id(2)],
    };
    let bytes = msg.to_cbor();
    let decoded = TxSubmissionMessage::from_cbor(&bytes).expect("decode MsgRequestTxs");
    assert_eq!(msg, decoded);
}

#[test]
fn tx_submission_cbor_request_txs_rejects_invalid_txid_length() {
    let bytes = vec![0x82, 0x02, 0x81, 0x42, 0xAA, 0xBB];
    let err = TxSubmissionMessage::from_cbor(&bytes).expect_err("short txid should fail");
    assert!(matches!(
        err,
        yggdrasil_ledger::LedgerError::CborInvalidLength {
            expected: 32,
            actual: 2,
        }
    ));
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

    let err = server
        .recv()
        .await
        .expect_err("should get connection closed");
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
        let ch = handles
            .get_mut(&MiniProtocolNum::CHAIN_SYNC)
            .expect("handle");
        ch.send(vec![0xCA, 0xFE]).await.expect("send");
        // Wait briefly for the server to process, then abort.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        mux.abort();
    });

    let (stream, _) = listener.accept().await.expect("accept");
    let (mut handles, mux) = start_mux(stream, MiniProtocolDir::Responder, &protos, 8);
    let ch = handles
        .get_mut(&MiniProtocolNum::CHAIN_SYNC)
        .expect("handle");
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
        let ch = handles
            .get_mut(&MiniProtocolNum::KEEP_ALIVE)
            .expect("handle");

        // Client sends ping.
        ch.send(vec![0xAA]).await.expect("send ping");

        // Client receives pong.
        let pong = ch.recv().await.expect("recv pong");
        assert_eq!(pong, vec![0xBB]);

        mux.abort();
    });

    let (stream, _) = listener.accept().await.expect("accept");
    let (mut handles, mux) = start_mux(stream, MiniProtocolDir::Responder, &protos, 8);
    let ch = handles
        .get_mut(&MiniProtocolNum::KEEP_ALIVE)
        .expect("handle");

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
    let h = handles
        .get_mut(&MiniProtocolNum::HANDSHAKE)
        .expect("handle");
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
    let bf = handles
        .get_mut(&MiniProtocolNum::BLOCK_FETCH)
        .expect("handle");
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
        assert!(
            writer_result.is_ok(),
            "writer should exit cleanly on handle drop"
        );
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
        let handle = handles
            .remove(&MiniProtocolNum::BLOCK_FETCH)
            .expect("handle");
        let ch = MessageChannel::new(handle);
        ch.send(send_payload).await.expect("send large payload");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        mux.abort();
    });

    let (stream, _) = listener.accept().await.expect("accept");
    let (mut handles, mux) = start_mux(stream, MiniProtocolDir::Responder, &protos, 8);
    let handle = handles
        .remove(&MiniProtocolNum::BLOCK_FETCH)
        .expect("handle");
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
        let handle = handles
            .remove(&MiniProtocolNum::CHAIN_SYNC)
            .expect("handle");
        let ch = MessageChannel::new(handle);
        ch.send(send_payload)
            .await
            .expect("send exact-multiple payload");
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        mux.abort();
    });

    let (stream, _) = listener.accept().await.expect("accept");
    let (mut handles, mux) = start_mux(stream, MiniProtocolDir::Responder, &protos, 8);
    let handle = handles
        .remove(&MiniProtocolNum::CHAIN_SYNC)
        .expect("handle");
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
        let handle = handles
            .remove(&MiniProtocolNum::BLOCK_FETCH)
            .expect("handle");
        let ch = MessageChannel::new(handle);
        for msg in &send_messages {
            ch.send(msg.clone()).await.expect("send");
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        mux.abort();
    });

    let (stream, _) = listener.accept().await.expect("accept");
    let (mut handles, mux) = start_mux(stream, MiniProtocolDir::Responder, &protos, 8);
    let handle = handles
        .remove(&MiniProtocolNum::BLOCK_FETCH)
        .expect("handle");
    let mut ch = MessageChannel::new(handle);
    for (i, expected) in messages.iter().enumerate() {
        let received = ch
            .recv()
            .await
            .unwrap_or_else(|| panic!("recv message {i}"));
        assert_eq!(
            received.len(),
            expected.len(),
            "message {i} length mismatch"
        );
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
    assert_eq!(cbor_item_length(&[0x00]), Some(1)); // uint 0
    assert_eq!(cbor_item_length(&[0x17]), Some(1)); // uint 23
    assert_eq!(cbor_item_length(&[0x18, 0x18]), Some(2)); // uint 24
    assert_eq!(cbor_item_length(&[0x19, 0x01, 0x00]), Some(3)); // uint 256

    // Byte strings.
    assert_eq!(cbor_item_length(&[0x43, 0xAA, 0xBB, 0xCC]), Some(4)); // bstr(3)
    assert_eq!(cbor_item_length(&[0x43, 0xAA, 0xBB]), None); // incomplete bstr

    // Empty array.
    assert_eq!(cbor_item_length(&[0x80]), Some(1)); // array(0)

    // Array with elements.
    assert_eq!(cbor_item_length(&[0x82, 0x01, 0x02]), Some(3)); // [1, 2]

    // Nested arrays.
    // [1, [2, 3]]
    assert_eq!(cbor_item_length(&[0x82, 0x01, 0x82, 0x02, 0x03]), Some(5));

    // Map.
    assert_eq!(cbor_item_length(&[0xA1, 0x01, 0x02]), Some(3)); // {1: 2}

    // Tag.
    assert_eq!(cbor_item_length(&[0xC0, 0x01]), Some(2)); // tag(0, uint 1)

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
// Mux — ingress queue overrun detection
// ===========================================================================

/// Verify that the demuxer terminates the connection when a protocol's
/// ingress bytes exceed the configured limit.
#[tokio::test]
async fn mux_ingress_queue_overrun() {
    use yggdrasil_network::mux::{ProtocolConfig, start_configured};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");

    // Server limits ChainSync ingress to 32 bytes.
    let server_handle = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let configs = vec![ProtocolConfig {
            num: MiniProtocolNum::CHAIN_SYNC,
            ingress_limit: 32,
            weight: 1,
        }];
        let (mut handles, mux) = start_configured(stream, MiniProtocolDir::Responder, &configs, 8);
        let ch = handles
            .get_mut(&MiniProtocolNum::CHAIN_SYNC)
            .expect("handle");
        // First small recv should succeed.
        let _first = ch.recv().await;
        // The second large payload should trigger IngressQueueOverRun in
        // the demuxer, which closes the connection. The recv may return
        // None or the reader task will fail.
        let _second = ch.recv().await;
        // The demuxer should have errored.
        let reader_result = mux.reader.await.expect("join reader");
        assert!(
            matches!(
                reader_result,
                Err(yggdrasil_network::MuxError::IngressQueueOverRun { .. })
            ),
            "expected IngressQueueOverRun, got {:?}",
            reader_result
        );
        mux.writer.abort();
    });

    let stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    let protos = [MiniProtocolNum::CHAIN_SYNC];
    let (mut handles, mux) = start_mux(stream, MiniProtocolDir::Initiator, &protos, 8);
    let ch = handles
        .get_mut(&MiniProtocolNum::CHAIN_SYNC)
        .expect("handle");
    // Send a small payload (within limit).
    ch.send(vec![0x01; 16]).await.expect("send small");
    // Send a large payload that will push over the 32-byte limit.
    let _ = ch.send(vec![0x02; 64]).await;
    // Allow server to process.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    mux.abort();

    server_handle.await.expect("server task");
}

// ===========================================================================
// Mux — egress buffer overflow detection
// ===========================================================================

/// Verify that send() returns EgressBufferOverflow when the pending
/// egress bytes would exceed the soft limit.
#[tokio::test]
async fn mux_egress_buffer_overflow() {
    use yggdrasil_network::mux::{ProtocolConfig, start_configured};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");

    // Spawn a server that never reads — egress will back up.
    let server_handle = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        let (_handles, _mux) = start_mux(
            stream,
            MiniProtocolDir::Responder,
            &[MiniProtocolNum::CHAIN_SYNC],
            2,
        );
        // Hold the stream open but don't read.
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    });

    let stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    // Use a very small egress limit by configuring with default then
    // relying on the per-protocol egress_limit field (EGRESS_SOFT_LIMIT).
    // We'll craft a payload larger than EGRESS_SOFT_LIMIT.
    let configs = vec![ProtocolConfig {
        num: MiniProtocolNum::CHAIN_SYNC,
        ingress_limit: 2_000_000,
        weight: 1,
    }];
    let (mut handles, mux) = start_configured(stream, MiniProtocolDir::Initiator, &configs, 2);
    let ch = handles
        .get_mut(&MiniProtocolNum::CHAIN_SYNC)
        .expect("handle");

    // Send a payload larger than EGRESS_SOFT_LIMIT (262143).
    // First send something to accumulate bytes.
    let large = vec![0xAA; yggdrasil_network::EGRESS_SOFT_LIMIT + 1];
    let result = ch.send(large).await;
    assert!(
        matches!(
            result,
            Err(yggdrasil_network::MuxError::EgressBufferOverflow { .. })
        ),
        "expected EgressBufferOverflow, got {:?}",
        result
    );

    mux.abort();
    server_handle.abort();
}

// ===========================================================================
// Mux — weighted round-robin fairness
// ===========================================================================

/// Verify that the mux writer interleaves SDUs from multiple protocols
/// rather than letting one protocol starve others.
#[tokio::test]
async fn mux_fair_scheduling_interleaves_protocols() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");

    let protos = [MiniProtocolNum::CHAIN_SYNC, MiniProtocolNum::BLOCK_FETCH];

    let client_handle = tokio::spawn(async move {
        let stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
        let (mut handles, mux) = start_mux(stream, MiniProtocolDir::Initiator, &protos, 16);

        let cs = handles.remove(&MiniProtocolNum::CHAIN_SYNC).expect("cs");
        let bf = handles.remove(&MiniProtocolNum::BLOCK_FETCH).expect("bf");

        // Send multiple messages on both protocols rapidly.
        for i in 0u8..5 {
            cs.send(vec![0x10 + i]).await.expect("send cs");
            bf.send(vec![0x20 + i]).await.expect("send bf");
        }

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        mux.abort();
    });

    let (stream, _) = listener.accept().await.expect("accept");
    let (mut handles, mux) = start_mux(stream, MiniProtocolDir::Responder, &protos, 16);
    let mut cs = handles.remove(&MiniProtocolNum::CHAIN_SYNC).expect("cs");
    let mut bf = handles.remove(&MiniProtocolNum::BLOCK_FETCH).expect("bf");

    // Collect messages from both protocols.
    let mut cs_msgs = Vec::new();
    let mut bf_msgs = Vec::new();

    for _ in 0..10 {
        tokio::select! {
            msg = cs.recv() => {
                if let Some(m) = msg { cs_msgs.push(m); }
            }
            msg = bf.recv() => {
                if let Some(m) = msg { bf_msgs.push(m); }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => {
                break;
            }
        }
    }

    // Both protocols should have received messages (not starved).
    assert!(
        !cs_msgs.is_empty(),
        "ChainSync should have received messages"
    );
    assert!(
        !bf_msgs.is_empty(),
        "BlockFetch should have received messages"
    );

    mux.abort();
    client_handle.await.expect("client task");
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
            header: vec![0x82, 0x00, 0x01],
            tip: vec![0x81, 0x01],
        };
        sh.send(reply.to_cbor()).await.expect("send reply");
    });

    let resp = client.request_next().await.expect("request_next");
    assert_eq!(
        resp,
        NextResponse::RollForward {
            header: vec![0x82, 0x00, 0x01],
            tip: vec![0x81, 0x01],
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
            point: vec![0x82, 0x00, 0x00],
            tip: vec![0x81, 0x00],
        };
        sh.send(reply.to_cbor()).await.expect("send reply");
    });

    let resp = client.request_next().await.expect("request_next");
    assert_eq!(
        resp,
        NextResponse::RollBackward {
            point: vec![0x82, 0x00, 0x00],
            tip: vec![0x81, 0x00],
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
            header: vec![0x82, 0x00, 0x03],
            tip: vec![0x82, 0x0A, 0x14],
        };
        sh.send(reply.to_cbor()).await.expect("send reply");
    });

    let resp = client
        .request_next()
        .await
        .expect("request_next with await");
    assert_eq!(
        resp,
        NextResponse::AwaitRollForward {
            header: vec![0x82, 0x00, 0x03],
            tip: vec![0x82, 0x0A, 0x14],
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
    let tip_obj = Tip::Tip(tip, BlockNo(15));

    let server = tokio::spawn(async move {
        let mut sh = s_handle;
        let _raw = sh.recv().await.expect("recv request");
        let reply = ChainSyncMessage::MsgRollBackward {
            point: point.to_cbor_bytes(),
            tip: tip_obj.to_cbor_bytes(),
        };
        sh.send(reply.to_cbor()).await.expect("send reply");
    });

    let resp = client
        .request_next_typed()
        .await
        .expect("request_next_typed");
    assert_eq!(resp, TypedNextResponse::RollBackward { point, tip });

    server.await.expect("server task");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    c_mux.abort();
    s_mux.abort();
}

#[tokio::test]
async fn chainsync_client_request_next_decoded_header_decodes_shelley_header() {
    let (c_handle, s_handle, c_mux, s_mux) = chainsync_mux_pair().await;
    let mut client = ChainSyncClient::new(c_handle);
    let header = sample_shelley_header();
    let tip = Point::BlockPoint(SlotNo(500), HeaderHash([0xCC; 32]));
    let tip_obj = Tip::Tip(tip, BlockNo(500));
    let expected_header = header.clone();

    let server = tokio::spawn(async move {
        let mut sh = s_handle;
        let _raw = sh.recv().await.expect("recv request");
        let reply = ChainSyncMessage::MsgRollForward {
            header: header.to_cbor_bytes(),
            tip: tip_obj.to_cbor_bytes(),
        };
        sh.send(reply.to_cbor()).await.expect("send reply");
    });

    let resp = client
        .request_next_decoded_header::<ShelleyHeader>()
        .await
        .expect("request_next_decoded_header");
    assert_eq!(
        resp,
        DecodedHeaderNextResponse::RollForward {
            header: expected_header,
            tip
        }
    );

    server.await.expect("server task");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    c_mux.abort();
    s_mux.abort();
}

#[tokio::test]
async fn chainsync_client_request_next_decoded_header_rejects_invalid_header() {
    let (c_handle, s_handle, c_mux, s_mux) = chainsync_mux_pair().await;
    let mut client = ChainSyncClient::new(c_handle);
    let tip = Point::Origin;

    let server = tokio::spawn(async move {
        let mut sh = s_handle;
        let _raw = sh.recv().await.expect("recv request");
        let reply = ChainSyncMessage::MsgRollForward {
            header: vec![0x80],
            tip: tip.to_cbor_bytes(),
        };
        sh.send(reply.to_cbor()).await.expect("send reply");
    });

    let err = client
        .request_next_decoded_header::<ShelleyHeader>()
        .await
        .expect_err("invalid header should fail");
    assert!(matches!(
        err,
        yggdrasil_network::ChainSyncClientError::HeaderDecode(_)
    ));

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
            point: vec![0x82, 0x03, 0x05],
            tip: vec![0x82, 0x03, 0x04],
        };
        sh.send(reply.to_cbor()).await.expect("send reply");
    });

    let resp = client
        .find_intersect(vec![vec![0x82, 0x0A, 0x01], vec![0x82, 0x0B, 0x01]])
        .await
        .expect("find_intersect");
    assert_eq!(
        resp,
        IntersectResponse::Found {
            point: vec![0x82, 0x03, 0x05],
            tip: vec![0x82, 0x03, 0x04],
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
            tip: vec![0x82, 0x04, 0x04],
        };
        sh.send(reply.to_cbor()).await.expect("send reply");
    });

    let resp = client
        .find_intersect(vec![vec![0x82, 0x18, 0xFF, 0x01]])
        .await
        .expect("find_intersect");
    assert_eq!(
        resp,
        IntersectResponse::NotFound {
            tip: vec![0x82, 0x04, 0x04],
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
    let tip_obj = Tip::Tip(tip, BlockNo(120));

    let server = tokio::spawn(async move {
        let mut sh = s_handle;
        let raw = sh.recv().await.expect("recv find_intersect");
        let msg = ChainSyncMessage::from_cbor(&raw).expect("decode");
        match msg {
            ChainSyncMessage::MsgFindIntersect { points } => {
                assert_eq!(
                    points,
                    vec![wanted.to_cbor_bytes(), Point::Origin.to_cbor_bytes()]
                );
            }
            _ => panic!("expected MsgFindIntersect"),
        }

        let reply = ChainSyncMessage::MsgIntersectFound {
            point: wanted.to_cbor_bytes(),
            tip: tip_obj.to_cbor_bytes(),
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
            point: vec![0x80],
            tip: vec![0x81, 0x03],
        };
        sh.send(reply.to_cbor()).await.expect("send intersect");

        // 2. RequestNext -> RollForward
        let _raw = sh.recv().await.expect("recv request1");
        let reply = ChainSyncMessage::MsgRollForward {
            header: vec![0x82, 0x00, 0x01],
            tip: vec![0x81, 0x03],
        };
        sh.send(reply.to_cbor()).await.expect("send rf1");

        // 3. RequestNext -> RollForward
        let _raw = sh.recv().await.expect("recv request2");
        let reply = ChainSyncMessage::MsgRollForward {
            header: vec![0x82, 0x00, 0x02],
            tip: vec![0x81, 0x03],
        };
        sh.send(reply.to_cbor()).await.expect("send rf2");

        // 4. RequestNext -> AwaitReply -> RollForward
        let _raw = sh.recv().await.expect("recv request3");
        sh.send(ChainSyncMessage::MsgAwaitReply.to_cbor())
            .await
            .expect("send await");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let reply = ChainSyncMessage::MsgRollForward {
            header: vec![0x82, 0x00, 0x03],
            tip: vec![0x81, 0x03],
        };
        sh.send(reply.to_cbor()).await.expect("send rf3");

        // 5. Done
        let _raw = sh.recv().await.expect("recv done");
    });

    // Client side: full sync sequence.
    let intersect = client
        .find_intersect(vec![vec![0x80]])
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
            lower: vec![0x82, 0x0A, 0x01],
            upper: vec![0x82, 0x0B, 0x02],
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
            lower: vec![0x82, 0x0A, 0x01],
            upper: vec![0x82, 0x0B, 0x02],
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
async fn blockfetch_client_recv_block_decoded_decodes_shelley_block() {
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
                block: sample_shelley_block_bytes(),
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
            lower: vec![0x82, 0x0A, 0x01],
            upper: vec![0x82, 0x0B, 0x02],
        })
        .await
        .expect("request_range");
    assert_eq!(resp, BatchResponse::StartedBatch);

    let blk = client
        .recv_block_decoded::<ShelleyBlock>()
        .await
        .expect("recv_block_decoded")
        .expect("expected block");
    assert_eq!(blk.header.body.block_number, 1);
    assert_eq!(blk.header.body.slot, 500);

    let done = client
        .recv_block_decoded::<ShelleyBlock>()
        .await
        .expect("recv_block_decoded batch_done");
    assert_eq!(done, None);

    server.await.expect("server task");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    c_mux.abort();
    s_mux.abort();
}

#[tokio::test]
async fn blockfetch_client_recv_block_decoded_rejects_invalid_block() {
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
                block: b"not-a-cbor-block".to_vec(),
            }
            .to_cbor(),
        )
        .await
        .expect("send invalid block");
    });

    let resp = client
        .request_range(ChainRange {
            lower: vec![0x82, 0x0A, 0x01],
            upper: vec![0x82, 0x0B, 0x02],
        })
        .await
        .expect("request_range");
    assert_eq!(resp, BatchResponse::StartedBatch);

    let err = client
        .recv_block_decoded::<ShelleyBlock>()
        .await
        .expect_err("invalid block should fail");
    assert!(matches!(
        err,
        yggdrasil_network::BlockFetchClientError::BlockDecode(_)
    ));

    server.await.expect("server task");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    c_mux.abort();
    s_mux.abort();
}

#[tokio::test]
async fn blockfetch_client_recv_block_raw_with_returns_raw_and_decoded() {
    let (c_handle, s_handle, c_mux, s_mux) = blockfetch_mux_pair().await;
    let mut client = BlockFetchClient::new(c_handle);
    let raw_block = sample_shelley_block_bytes();

    let server = tokio::spawn(async move {
        let mut sh = s_handle;
        let _raw = sh.recv().await.expect("recv");

        sh.send(BlockFetchMessage::MsgStartBatch.to_cbor())
            .await
            .expect("send start_batch");
        sh.send(BlockFetchMessage::MsgBlock { block: raw_block }.to_cbor())
            .await
            .expect("send block");
        sh.send(BlockFetchMessage::MsgBatchDone.to_cbor())
            .await
            .expect("send batch_done");
    });

    let resp = client
        .request_range(ChainRange {
            lower: vec![0x82, 0x0A, 0x01],
            upper: vec![0x82, 0x0B, 0x02],
        })
        .await
        .expect("request_range");
    assert_eq!(resp, BatchResponse::StartedBatch);

    let (raw, blk) = client
        .recv_block_raw_decoded::<ShelleyBlock>()
        .await
        .expect("recv_block_raw_decoded")
        .expect("expected block");
    assert_eq!(raw, sample_shelley_block_bytes());
    assert_eq!(blk.header.body.block_number, 1);

    assert_eq!(
        client
            .recv_block_raw_decoded::<ShelleyBlock>()
            .await
            .expect("done"),
        None
    );

    server.await.expect("server task");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    c_mux.abort();
    s_mux.abort();
}

#[tokio::test]
async fn blockfetch_client_request_range_collect_decoded_collects_full_batch() {
    let (c_handle, s_handle, c_mux, s_mux) = blockfetch_mux_pair().await;
    let mut client = BlockFetchClient::new(c_handle);

    let server = tokio::spawn(async move {
        let mut sh = s_handle;
        let _raw = sh.recv().await.expect("recv");

        sh.send(BlockFetchMessage::MsgStartBatch.to_cbor())
            .await
            .expect("send start_batch");
        for _ in 0..2 {
            sh.send(
                BlockFetchMessage::MsgBlock {
                    block: sample_shelley_block_bytes(),
                }
                .to_cbor(),
            )
            .await
            .expect("send block");
        }
        sh.send(BlockFetchMessage::MsgBatchDone.to_cbor())
            .await
            .expect("send batch_done");
    });

    let blocks = client
        .request_range_collect_decoded::<ShelleyBlock>(ChainRange {
            lower: vec![0x82, 0x0A, 0x01],
            upper: vec![0x82, 0x0B, 0x02],
        })
        .await
        .expect("request_range_collect_decoded");
    assert_eq!(blocks.len(), 2);
    assert!(
        blocks
            .iter()
            .all(|block| block.header.body.block_number == 1)
    );
    assert_eq!(client.state(), BlockFetchState::StIdle);

    server.await.expect("server task");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    c_mux.abort();
    s_mux.abort();
}

#[tokio::test]
async fn blockfetch_client_request_range_collect_points_raw_with_collects_pairs() {
    let (c_handle, s_handle, c_mux, s_mux) = blockfetch_mux_pair().await;
    let mut client = BlockFetchClient::new(c_handle);
    let lower = Point::Origin;
    let upper = Point::BlockPoint(SlotNo(50), HeaderHash([0x50; 32]));

    let server = tokio::spawn(async move {
        let mut sh = s_handle;
        let _raw = sh.recv().await.expect("recv");

        sh.send(BlockFetchMessage::MsgStartBatch.to_cbor())
            .await
            .expect("send start_batch");
        sh.send(
            BlockFetchMessage::MsgBlock {
                block: sample_shelley_block_bytes(),
            }
            .to_cbor(),
        )
        .await
        .expect("send block");
        sh.send(BlockFetchMessage::MsgBatchDone.to_cbor())
            .await
            .expect("send batch_done");
    });

    let blocks = client
        .request_range_collect_points_raw_with(lower, upper, ShelleyBlock::from_cbor_bytes)
        .await
        .expect("request_range_collect_points_raw_with");
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].0, sample_shelley_block_bytes());
    assert_eq!(blocks[0].1.header.body.slot, 500);
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
            sh.send(BlockFetchMessage::MsgBlock { block: vec![i; 16] }.to_cbor())
                .await
                .expect("send block");
        }
        sh.send(BlockFetchMessage::MsgBatchDone.to_cbor())
            .await
            .expect("batch_done");
    });

    let resp = client
        .request_range(ChainRange {
            lower: vec![0x82, 0x0A, 0x01],
            upper: vec![0x82, 0x0B, 0x02],
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
            lower: vec![0x82, 0x0A, 0x01],
            upper: vec![0x82, 0x0B, 0x02],
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
            lower: vec![0x82, 0x0C, 0x03],
            upper: vec![0x82, 0x0D, 0x04],
        })
        .await
        .expect("range 2");
    assert_eq!(r2, BatchResponse::NoBlocks);

    // Range 3: 1 block.
    let r3 = client
        .request_range(ChainRange {
            lower: vec![0x82, 0x0E, 0x05],
            upper: vec![0x82, 0x0F, 0x06],
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
            blocking: true,
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
                assert_eq!(txids[0].txid, sample_tx_id(0x11));
                assert_eq!(txids[0].size, 100);
                assert_eq!(txids[1].txid, sample_tx_id(0x22));
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
            blocking: true,
            ack: 0,
            req: 3,
        }
    );

    client
        .reply_tx_ids(vec![
            TxIdAndSize {
                txid: sample_tx_id(0x11),
                size: 100,
            },
            TxIdAndSize {
                txid: sample_tx_id(0x22),
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

        sh.send(
            TxSubmissionMessage::MsgRequestTxIds {
                blocking: true,
                ack: 0,
                req: 2,
            }
            .to_cbor(),
        )
        .await
        .expect("send request_tx_ids");

        let raw = sh.recv().await.expect("recv reply_txids");
        let msg = TxSubmissionMessage::from_cbor(&raw).expect("decode");
        match msg {
            TxSubmissionMessage::MsgReplyTxIds { txids } => {
                assert_eq!(txids.len(), 2);
                assert_eq!(txids[0].txid, sample_tx_id(0x0A));
                assert_eq!(txids[1].txid, sample_tx_id(0x0B));
            }
            _ => panic!("expected MsgReplyTxIds"),
        }

        // MsgRequestTxs.
        let req = TxSubmissionMessage::MsgRequestTxs {
            txids: vec![sample_tx_id(0x0A), sample_tx_id(0x0B)],
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

    let req = client.recv_request().await.expect("recv txid request");
    assert_eq!(
        req,
        TxServerRequest::RequestTxIds {
            blocking: true,
            ack: 0,
            req: 2,
        }
    );
    client
        .reply_tx_ids(vec![
            TxIdAndSize {
                txid: sample_tx_id(0x0A),
                size: 6,
            },
            TxIdAndSize {
                txid: sample_tx_id(0x0B),
                size: 6,
            },
        ])
        .await
        .expect("reply_tx_ids");

    let req = client.recv_request().await.expect("recv_request");
    assert_eq!(
        req,
        TxServerRequest::RequestTxs {
            txids: vec![sample_tx_id(0x0A), sample_tx_id(0x0B)],
        }
    );

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
        TxServerRequest::RequestTxIds { blocking: true, .. }
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
    let submitted = sample_shelley_submitted_tx(0x44);
    let submitted_bytes = submitted.raw_cbor();
    let submitted_size = submitted_bytes.len() as u32;
    let submitted_txid = submitted.tx_id();

    let server = tokio::spawn(async move {
        let mut sh = s_handle;

        // 1. MsgInit.
        let _raw = sh.recv().await.expect("recv init");

        // 2. Non-blocking MsgRequestTxIds.
        sh.send(
            TxSubmissionMessage::MsgRequestTxIds {
                blocking: true,
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
                txids: vec![txids[0].txid],
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
                assert_eq!(txs[0], submitted_bytes);
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
            txid: submitted_txid,
            size: submitted_size,
        }])
        .await
        .expect("reply_tx_ids");

    // Fetch request: reply with the tx body.
    let req = client.recv_request().await.expect("2");
    assert_eq!(
        req,
        TxServerRequest::RequestTxs {
            txids: vec![submitted_txid],
        }
    );
    client
        .reply_txs_multi_era(vec![submitted])
        .await
        .expect("reply_txs");

    // Blocking: we have nothing, send Done.
    let req = client.recv_request().await.expect("3");
    assert!(matches!(
        req,
        TxServerRequest::RequestTxIds { blocking: true, .. }
    ));
    client.done().await.expect("done");

    server.await.expect("server task");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    c_mux.abort();
    s_mux.abort();
}

#[tokio::test]
async fn txsubmission_client_full_session_alonzo_multi_era() {
    let (c_handle, s_handle, c_mux, s_mux) = txsubmission_mux_pair().await;
    let mut client = TxSubmissionClient::new(c_handle);
    let submitted = sample_alonzo_submitted_tx(0x52);
    let submitted_bytes = submitted.raw_cbor();
    let submitted_size = submitted_bytes.len() as u32;
    let submitted_txid = submitted.tx_id();

    let server = tokio::spawn(async move {
        let mut sh = s_handle;

        let _raw = sh.recv().await.expect("recv init");

        sh.send(
            TxSubmissionMessage::MsgRequestTxIds {
                blocking: true,
                ack: 0,
                req: 1,
            }
            .to_cbor(),
        )
        .await
        .expect("send request txids");

        let raw = sh.recv().await.expect("recv reply_txids");
        let msg = TxSubmissionMessage::from_cbor(&raw).expect("decode");
        let txids = match msg {
            TxSubmissionMessage::MsgReplyTxIds { txids } => txids,
            _ => panic!("expected MsgReplyTxIds"),
        };
        assert_eq!(txids.len(), 1);
        assert_eq!(txids[0].txid, submitted_txid);

        sh.send(
            TxSubmissionMessage::MsgRequestTxs {
                txids: vec![submitted_txid],
            }
            .to_cbor(),
        )
        .await
        .expect("send request_txs");

        let raw = sh.recv().await.expect("recv reply_txs");
        let msg = TxSubmissionMessage::from_cbor(&raw).expect("decode");
        match msg {
            TxSubmissionMessage::MsgReplyTxs { txs } => {
                assert_eq!(txs, vec![submitted_bytes]);
            }
            _ => panic!("expected MsgReplyTxs"),
        }

        sh.send(
            TxSubmissionMessage::MsgRequestTxIds {
                blocking: true,
                ack: 1,
                req: 1,
            }
            .to_cbor(),
        )
        .await
        .expect("send blocking request");

        let raw = sh.recv().await.expect("recv done");
        let msg = TxSubmissionMessage::from_cbor(&raw).expect("decode");
        assert_eq!(msg, TxSubmissionMessage::MsgDone);
    });

    client.init().await.expect("init");

    let req = client.recv_request().await.expect("recv txid request");
    assert_eq!(
        req,
        TxServerRequest::RequestTxIds {
            blocking: true,
            ack: 0,
            req: 1,
        }
    );
    client
        .reply_tx_ids(vec![TxIdAndSize {
            txid: submitted_txid,
            size: submitted_size,
        }])
        .await
        .expect("reply_tx_ids");

    let req = client.recv_request().await.expect("recv tx request");
    assert_eq!(
        req,
        TxServerRequest::RequestTxs {
            txids: vec![submitted_txid],
        }
    );
    client
        .reply_txs_multi_era(vec![submitted])
        .await
        .expect("reply_txs");

    let req = client.recv_request().await.expect("recv blocking request");
    assert!(matches!(
        req,
        TxServerRequest::RequestTxIds {
            blocking: true,
            ack: 1,
            req: 1,
        }
    ));
    client.send_done().await.expect("done");

    server.await.expect("server task");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    c_mux.abort();
    s_mux.abort();
}

#[tokio::test]
async fn txsubmission_client_rejects_acknowledging_too_many_txids() {
    let (c_handle, s_handle, c_mux, s_mux) = txsubmission_mux_pair().await;
    let mut client = TxSubmissionClient::new(c_handle);

    let server = tokio::spawn(async move {
        let mut sh = s_handle;

        let _ = sh.recv().await.expect("recv init");
        sh.send(
            TxSubmissionMessage::MsgRequestTxIds {
                blocking: true,
                ack: 0,
                req: 1,
            }
            .to_cbor(),
        )
        .await
        .expect("send first request");

        let _ = sh.recv().await.expect("recv reply_txids");

        sh.send(
            TxSubmissionMessage::MsgRequestTxIds {
                blocking: true,
                ack: 2,
                req: 1,
            }
            .to_cbor(),
        )
        .await
        .expect("send invalid ack request");
    });

    client.init().await.expect("init");
    let _ = client.recv_request().await.expect("recv first request");
    client
        .reply_tx_ids(vec![TxIdAndSize {
            txid: sample_tx_id(0x31),
            size: 10,
        }])
        .await
        .expect("reply txids");

    let err = client.recv_request().await.expect_err("ack should fail");
    assert!(matches!(
        err,
        yggdrasil_network::TxSubmissionClientError::AckedTooManyTxIds {
            ack: 2,
            outstanding: 1,
        }
    ));

    server.await.expect("server task");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    c_mux.abort();
    s_mux.abort();
}

#[tokio::test]
async fn txsubmission_client_rejects_blocking_request_with_outstanding_txids() {
    let (c_handle, s_handle, c_mux, s_mux) = txsubmission_mux_pair().await;
    let mut client = TxSubmissionClient::new(c_handle);

    let server = tokio::spawn(async move {
        let mut sh = s_handle;

        let _ = sh.recv().await.expect("recv init");
        sh.send(
            TxSubmissionMessage::MsgRequestTxIds {
                blocking: true,
                ack: 0,
                req: 1,
            }
            .to_cbor(),
        )
        .await
        .expect("send first request");

        let _ = sh.recv().await.expect("recv reply_txids");

        sh.send(
            TxSubmissionMessage::MsgRequestTxIds {
                blocking: true,
                ack: 0,
                req: 1,
            }
            .to_cbor(),
        )
        .await
        .expect("send invalid blocking request");
    });

    client.init().await.expect("init");
    let _ = client.recv_request().await.expect("recv first request");
    client
        .reply_tx_ids(vec![TxIdAndSize {
            txid: sample_tx_id(0x32),
            size: 10,
        }])
        .await
        .expect("reply txids");

    let err = client
        .recv_request()
        .await
        .expect_err("blocking request should fail");
    assert!(matches!(
        err,
        yggdrasil_network::TxSubmissionClientError::BlockingRequestHasOutstandingTxIds {
            remaining: 1,
        }
    ));

    server.await.expect("server task");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    c_mux.abort();
    s_mux.abort();
}

#[tokio::test]
async fn txsubmission_client_rejects_unavailable_tx_requests() {
    let (c_handle, s_handle, c_mux, s_mux) = txsubmission_mux_pair().await;
    let mut client = TxSubmissionClient::new(c_handle);

    let server = tokio::spawn(async move {
        let mut sh = s_handle;

        let _ = sh.recv().await.expect("recv init");
        sh.send(
            TxSubmissionMessage::MsgRequestTxIds {
                blocking: true,
                ack: 0,
                req: 1,
            }
            .to_cbor(),
        )
        .await
        .expect("send first request");

        let _ = sh.recv().await.expect("recv reply_txids");

        sh.send(
            TxSubmissionMessage::MsgRequestTxs {
                txids: vec![sample_tx_id(0x99)],
            }
            .to_cbor(),
        )
        .await
        .expect("send invalid request_txs");
    });

    client.init().await.expect("init");
    let _ = client.recv_request().await.expect("recv first request");
    client
        .reply_tx_ids(vec![TxIdAndSize {
            txid: sample_tx_id(0x33),
            size: 10,
        }])
        .await
        .expect("reply txids");

    let err = client
        .recv_request()
        .await
        .expect_err("unknown txid request should fail");
    assert!(matches!(
        err,
        yggdrasil_network::TxSubmissionClientError::RequestedUnavailableTxId { txid }
        if txid == sample_tx_id(0x99)
    ));

    server.await.expect("server task");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    c_mux.abort();
    s_mux.abort();
}

#[tokio::test]
async fn txsubmission_client_rejects_unrequested_typed_tx_reply() {
    let (c_handle, s_handle, c_mux, s_mux) = txsubmission_mux_pair().await;
    let mut client = TxSubmissionClient::new(c_handle);

    let server = tokio::spawn(async move {
        let mut sh = s_handle;

        let _ = sh.recv().await.expect("recv init");
        sh.send(
            TxSubmissionMessage::MsgRequestTxIds {
                blocking: true,
                ack: 0,
                req: 1,
            }
            .to_cbor(),
        )
        .await
        .expect("send first request");

        let _ = sh.recv().await.expect("recv reply_txids");

        sh.send(
            TxSubmissionMessage::MsgRequestTxs {
                txids: vec![sample_tx_id(0x44)],
            }
            .to_cbor(),
        )
        .await
        .expect("send request_txs");
    });

    client.init().await.expect("init");
    let _ = client.recv_request().await.expect("recv first request");
    client
        .reply_tx_ids(vec![TxIdAndSize {
            txid: sample_tx_id(0x44),
            size: 50,
        }])
        .await
        .expect("reply txids");

    let _ = client.recv_request().await.expect("recv tx request");
    let err = client
        .reply_txs_multi_era(vec![sample_shelley_submitted_tx(0x55)])
        .await
        .expect_err("typed reply with wrong txid should fail");
    assert!(matches!(
        err,
        yggdrasil_network::TxSubmissionClientError::ReturnedUnrequestedTxId { txid }
        if txid == sample_shelley_submitted_tx(0x55).tx_id()
    ));

    server.await.expect("server task");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    c_mux.abort();
    s_mux.abort();
}

// ---------------------------------------------------------------------------
// PeerRegistry tests
// ---------------------------------------------------------------------------

#[test]
fn peer_registry_len_tracks_entries() {
    let mut reg = PeerRegistry::default();
    assert_eq!(reg.len(), 0);
    assert!(reg.is_empty());

    let addr_a: std::net::SocketAddr = "127.0.0.1:3001".parse().expect("parse addr_a");
    let addr_b: std::net::SocketAddr = "127.0.0.2:3001".parse().expect("parse addr_b");
    let addr_c: std::net::SocketAddr = "127.0.0.3:3001".parse().expect("parse addr_c");

    reg.insert_source(addr_a, PeerSource::PeerSourceLocalRoot);
    assert_eq!(reg.len(), 1);

    reg.insert_source(addr_b, PeerSource::PeerSourcePublicRoot);
    assert_eq!(reg.len(), 2);

    // Inserting a second source for the same peer should NOT increase len.
    reg.insert_source(addr_a, PeerSource::PeerSourceBootstrap);
    assert_eq!(reg.len(), 2);

    reg.insert_source(addr_c, PeerSource::PeerSourceLedger);
    assert_eq!(reg.len(), 3);
    assert!(!reg.is_empty());
}

#[test]
fn peer_registry_entry_is_root_peer_local() {
    let entry = PeerRegistryEntry {
        sources: std::collections::BTreeSet::from([PeerSource::PeerSourceLocalRoot]),
        status: PeerStatus::PeerCold,
        hot_tip_slot: None,
        tepid: false,
    };
    assert!(entry.is_root_peer());
}

#[test]
fn peer_registry_entry_is_root_peer_public() {
    let entry = PeerRegistryEntry {
        sources: std::collections::BTreeSet::from([PeerSource::PeerSourcePublicRoot]),
        status: PeerStatus::PeerCold,
        hot_tip_slot: None,
        tepid: false,
    };
    assert!(entry.is_root_peer());
}

#[test]
fn peer_registry_entry_is_root_peer_ledger() {
    // Ledger peers are considered root peers by the current implementation
    // (matches PeerSourceLedger in is_root_peer). Only PeerShare is excluded.
    let entry = PeerRegistryEntry {
        sources: std::collections::BTreeSet::from([PeerSource::PeerSourceLedger]),
        status: PeerStatus::PeerCold,
        hot_tip_slot: None,
        tepid: false,
    };
    assert!(entry.is_root_peer());

    // PeerShare is the only non-root source.
    let peer_share_only = PeerRegistryEntry {
        sources: std::collections::BTreeSet::from([PeerSource::PeerSourcePeerShare]),
        status: PeerStatus::PeerCold,
        hot_tip_slot: None,
        tepid: false,
    };
    assert!(!peer_share_only.is_root_peer());
}

#[test]
fn peer_registry_entry_is_root_peer_mixed_sources() {
    // An entry with Ledger + LocalRoot should be a root peer.
    let entry = PeerRegistryEntry {
        sources: std::collections::BTreeSet::from([
            PeerSource::PeerSourceLedger,
            PeerSource::PeerSourceLocalRoot,
        ]),
        status: PeerStatus::PeerWarm,
        hot_tip_slot: None,
        tepid: false,
    };
    assert!(entry.is_root_peer());

    // Even PeerShare mixed with LocalRoot should be a root peer.
    let mixed = PeerRegistryEntry {
        sources: std::collections::BTreeSet::from([
            PeerSource::PeerSourcePeerShare,
            PeerSource::PeerSourceLocalRoot,
        ]),
        status: PeerStatus::PeerCold,
        hot_tip_slot: None,
        tepid: false,
    };
    assert!(mixed.is_root_peer());
}

// ---------------------------------------------------------------------------
// Governor tests
// ---------------------------------------------------------------------------

#[test]
fn local_root_targets_from_config() {
    let config = LocalRootConfig {
        access_points: vec![
            PeerAccessPoint {
                address: "127.0.0.1".to_owned(),
                port: 3001,
            },
            PeerAccessPoint {
                address: "127.0.0.2".to_owned(),
                port: 3002,
            },
        ],
        advertise: false,
        trustable: true,
        hot_valency: 2,
        warm_valency: Some(4),
        diffusion_mode: PeerDiffusionMode::InitiatorAndResponderDiffusionMode,
    };

    let resolved: Vec<std::net::SocketAddr> = vec![
        "127.0.0.1:3001".parse().expect("parse resolved_a"),
        "127.0.0.2:3002".parse().expect("parse resolved_b"),
    ];

    let targets = LocalRootTargets::from_config(&config, resolved.clone());

    assert_eq!(targets.peers, resolved);
    assert_eq!(targets.hot_valency, 2);
    // warm_valency should come from effective_warm_valency() which returns
    // the explicit warm_valency (4) when present.
    assert_eq!(targets.warm_valency, 4);
}

#[test]
fn local_root_targets_from_config_defaults_warm_to_hot() {
    let config = LocalRootConfig {
        access_points: vec![PeerAccessPoint {
            address: "127.0.0.1".to_owned(),
            port: 3001,
        }],
        advertise: false,
        trustable: false,
        hot_valency: 3,
        warm_valency: None,
        diffusion_mode: PeerDiffusionMode::InitiatorOnlyDiffusionMode,
    };

    let resolved: Vec<std::net::SocketAddr> =
        vec!["127.0.0.1:3001".parse().expect("parse resolved")];

    let targets = LocalRootTargets::from_config(&config, resolved.clone());

    assert_eq!(targets.peers, resolved);
    assert_eq!(targets.hot_valency, 3);
    // When warm_valency is None, effective_warm_valency() falls back to
    // hot_valency.
    assert_eq!(targets.warm_valency, 3);
}

// ---------------------------------------------------------------------------
// PeerSelection tests
// ---------------------------------------------------------------------------

#[test]
fn peer_attempt_state_targets_returns_bootstrap_targets() {
    let primary: std::net::SocketAddr = "127.0.0.10:3001".parse().expect("parse primary");
    let fallback_a: std::net::SocketAddr = "127.0.0.11:3001".parse().expect("parse fallback_a");
    let fallback_b: std::net::SocketAddr = "127.0.0.12:3001".parse().expect("parse fallback_b");

    let bt = PeerBootstrapTargets::new(primary, &[fallback_a, fallback_b]);
    let state = PeerAttemptState::new(bt);

    let targets = state.targets();
    assert_eq!(targets.primary_peer(), primary);
    assert_eq!(targets.fallback_peers(), &[fallback_a, fallback_b]);
}
