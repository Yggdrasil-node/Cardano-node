use yggdrasil_network::{ChainSyncState, HandshakeRequest, HandshakeVersion, MuxChannel};

#[test]
fn handshake_request_keeps_version() {
    let request = HandshakeRequest {
        network_magic: 1,
        version: HandshakeVersion(12),
    };

    assert_eq!(request.version, HandshakeVersion(12));
    assert_eq!(MuxChannel(3), MuxChannel(3));
    assert_eq!(ChainSyncState::Idle, ChainSyncState::Idle);
}
