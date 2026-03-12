use std::net::SocketAddr;
use yggdrasil_network::{
    ChainSyncMessage,
    HandshakeVersion,
    peer_accept,
};
use yggdrasil_node::{NodeConfig, bootstrap};

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
