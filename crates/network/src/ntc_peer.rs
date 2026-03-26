//! Node-to-client (NtC) connection lifecycle.
//!
//! Establishes a connection over a Unix domain socket, runs the NtC handshake
//! mini-protocol (version negotiation), and returns a [`NtcPeerConnection`]
//! with handles for all registered NtC mini-protocols.
//!
//! ## NtC Mini-Protocol IDs
//!
//! | Protocol ID | Name                 |
//! |-------------|----------------------|
//! | 0           | Handshake            |
//! | 5           | LocalChainSync       |
//! | 6           | LocalTxSubmission    |
//! | 7           | LocalStateQuery      |
//! | 9           | LocalTxMonitor       |
//!
//! ## NtC Version Numbers
//!
//! Supported versions (from upstream `Ouroboros.Network.NodeToClient.Version`):
//! - V9 = 9, V10 = 10, V11 = 11, V12 = 12, V13 = 13, V14 = 14, V15 = 15, V16 = 16
//!
//! Version data carries `{network_magic: u32, query: bool}`.
//!
//! Reference: <https://github.com/IntersectMBO/ouroboros-network/tree/main/ouroboros-network/src/Ouroboros/Network/NodeToClient>

use std::collections::HashMap;
use std::path::Path;

use tokio::net::UnixStream;

use crate::mux::{MessageChannel, MuxHandle, ProtocolHandle, start as start_mux};
use crate::multiplexer::MiniProtocolNum;
use crate::handshake::HandshakeVersion;

// ---------------------------------------------------------------------------
// NtC version data
// ---------------------------------------------------------------------------

/// Version negotiation payload for node-to-client connections.
///
/// Simpler than the N2N counterpart — carries only `network_magic` and a
/// `query` flag (whether the client intends to run queries only, without sync).
///
/// Reference: `Ouroboros.Network.NodeToClient.NodeToClientVersionData`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodeToClientVersionData {
    /// Network magic (1 = preprod, 2 = preview, 764824073 = mainnet).
    pub network_magic: u32,
    /// When `true` the client only queries ledger state and does not sync.
    pub query: bool,
}

/// NtC protocol version numbers.
///
/// Reference: `Ouroboros.Network.NodeToClient.Version`.
impl HandshakeVersion {
    /// NtC v9 (Alonzo era onwards).
    pub const NTC_V9: Self  = Self(9);
    /// NtC v10.
    pub const NTC_V10: Self = Self(10);
    /// NtC v11.
    pub const NTC_V11: Self = Self(11);
    /// NtC v12.
    pub const NTC_V12: Self = Self(12);
    /// NtC v13.
    pub const NTC_V13: Self = Self(13);
    /// NtC v14.
    pub const NTC_V14: Self = Self(14);
    /// NtC v15.
    pub const NTC_V15: Self = Self(15);
    /// NtC v16 (Conway era, current).
    pub const NTC_V16: Self = Self(16);
}

/// NtC protocol handles registered per connection.
const NTC_PROTOCOLS: [MiniProtocolNum; 4] = [
    MiniProtocolNum::LOCAL_TX_SUBMISSION,
    MiniProtocolNum::LOCAL_STATE_QUERY,
    MiniProtocolNum::LOCAL_TX_MONITOR,
    MiniProtocolNum::LOCAL_CHAIN_SYNC,
];

// ---------------------------------------------------------------------------
// NtcPeerConnection
// ---------------------------------------------------------------------------

/// An established node-to-client connection with negotiated version data and
/// per-protocol message channel handles.
pub struct NtcPeerConnection {
    /// Negotiated NtC protocol version.
    pub version: HandshakeVersion,
    /// Version data from the server.
    pub version_data: NodeToClientVersionData,
    /// Per-protocol handles keyed by `MiniProtocolNum`.
    pub protocols: HashMap<MiniProtocolNum, ProtocolHandle>,
    /// Mux handle for lifecycle management.
    pub mux: MuxHandle,
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors produced during NtC connection setup.
#[derive(Debug, thiserror::Error)]
pub enum NtcPeerError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("handshake version mismatch — no common NtC version found")]
    VersionMismatch,
    #[error("handshake refused: {0}")]
    HandshakeRefused(String),
    #[error("CBOR codec error: {0}")]
    Cbor(String),
    #[error("mux error: {0}")]
    Mux(String),
}

// ---------------------------------------------------------------------------
// NtC handshake helpers
// ---------------------------------------------------------------------------

/// Encode NtC version data as CBOR `[network_magic, query]`.
fn encode_ntc_version_data(magic: u32, query: bool) -> Vec<u8> {
    let mut buf = Vec::new();
    // Simple 2-element array matching upstream encoding
    minicbor::encode(&(magic as u64, query), &mut buf).expect("infallible");
    buf
}

/// Decode NtC version data from CBOR `[network_magic, query]`.
fn decode_ntc_version_data(data: &[u8]) -> Result<NodeToClientVersionData, NtcPeerError> {
    let mut dec = minicbor::Decoder::new(data);
    dec.array().map_err(|e| NtcPeerError::Cbor(e.to_string()))?;
    let magic: u64 = dec.decode().map_err(|e| NtcPeerError::Cbor(e.to_string()))?;
    let query: bool = dec.decode().map_err(|e| NtcPeerError::Cbor(e.to_string()))?;
    Ok(NodeToClientVersionData {
        network_magic: magic as u32,
        query,
    })
}

/// Build a `ProposeVersions` CBOR payload for NtC handshake.
///
/// Proposes V9 through V16 in descending order. The server picks the highest
/// version it supports.
fn build_ntc_propose_versions(network_magic: u32, query: bool) -> Vec<u8> {
    let vd = encode_ntc_version_data(network_magic, query);
    let versions: Vec<(u64, &[u8])> = [9u64, 10, 11, 12, 13, 14, 15, 16]
        .iter()
        .map(|v| (*v, vd.as_slice()))
        .collect();

    // Encode as CBOR: [0, {version_num: version_data, ...}]
    // Tag 0 = ProposeVersions
    let mut map_buf = Vec::new();
    let mut enc = minicbor::Encoder::new(&mut map_buf);
    enc.map(versions.len() as u64).expect("infallible");
    for (v, data) in &versions {
        enc.u64(*v).expect("infallible");
        enc.bytes(data).expect("infallible");
    }
    drop(enc);

    let mut buf = Vec::new();
    let mut outer = minicbor::Encoder::new(&mut buf);
    outer.array(2).expect("infallible");
    outer.u64(0).expect("infallible"); // ProposeVersions tag
    outer.writer().extend_from_slice(&map_buf);
    buf
}

/// Parse the server's `AcceptVersion` response and extract version + data.
fn parse_ntc_accept_version(
    data: &[u8],
) -> Result<(HandshakeVersion, NodeToClientVersionData), NtcPeerError> {
    let mut dec = minicbor::Decoder::new(data);
    dec.array().map_err(|e| NtcPeerError::Cbor(e.to_string()))?;
    let tag: u64 = dec.decode().map_err(|e| NtcPeerError::Cbor(e.to_string()))?;
    match tag {
        1 => {
            // AcceptVersion
            let ver: u64 = dec.decode().map_err(|e| NtcPeerError::Cbor(e.to_string()))?;
            let vd_bytes: minicbor::bytes::ByteVec =
                dec.decode().map_err(|e| NtcPeerError::Cbor(e.to_string()))?;
            let vd = decode_ntc_version_data(&vd_bytes)?;
            Ok((HandshakeVersion(ver as u16), vd))
        }
        2 => {
            // Refuse
            Err(NtcPeerError::HandshakeRefused(
                "server refused NtC connection".to_owned(),
            ))
        }
        _ => Err(NtcPeerError::Cbor(format!("unexpected handshake tag {tag}"))),
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Connect to a Cardano node over a Unix socket and complete the NtC handshake.
///
/// Returns a [`NtcPeerConnection`] with handles for LocalTxSubmission,
/// LocalStateQuery, LocalTxMonitor, and LocalChainSync protocols.
///
/// # Arguments
/// - `socket_path` — path to the node's Unix socket (e.g. `/run/cardano-node/node.socket`)
/// - `network_magic` — network discriminant (mainnet = 764824073)
/// - `query_only` — if `true`, advertise as a query-only client (no sync)
pub async fn ntc_connect(
    socket_path: impl AsRef<Path>,
    network_magic: u32,
    query_only: bool,
) -> Result<NtcPeerConnection, NtcPeerError> {
    let stream = UnixStream::connect(socket_path.as_ref()).await?;
    let (read_half, write_half) = stream.into_split();

    // Start the mux over the Unix stream
    let mut protocols_init: Vec<MiniProtocolNum> =
        vec![MiniProtocolNum::HANDSHAKE];
    protocols_init.extend_from_slice(&NTC_PROTOCOLS);

    let mux = start_mux(read_half, write_half, &protocols_init)
        .map_err(|e| NtcPeerError::Mux(e.to_string()))?;

    let mut handles = mux.handles.clone();

    // Run handshake
    let hs_handle = handles
        .remove(&MiniProtocolNum::HANDSHAKE)
        .ok_or_else(|| NtcPeerError::Mux("no handshake handle".to_owned()))?;
    let mut hs_channel = MessageChannel::new(hs_handle);

    let propose = build_ntc_propose_versions(network_magic, query_only);
    hs_channel
        .send(propose)
        .await
        .map_err(|e| NtcPeerError::Mux(e.to_string()))?;

    let response = hs_channel
        .recv()
        .await
        .ok_or(NtcPeerError::Mux("handshake channel closed".to_owned()))?
        .map_err(|e| NtcPeerError::Mux(e.to_string()))?;

    let (version, version_data) = parse_ntc_accept_version(&response)?;

    // Collect data protocol handles
    let protocols: HashMap<MiniProtocolNum, ProtocolHandle> = NTC_PROTOCOLS
        .iter()
        .filter_map(|num| handles.remove(num).map(|h| (*num, h)))
        .collect();

    Ok(NtcPeerConnection {
        version,
        version_data,
        protocols,
        mux: mux.mux_handle,
    })
}

/// Accept an inbound NtC connection from a Unix socket stream.
///
/// Runs the server side of the NtC handshake, selecting the highest common
/// protocol version, then returns a [`NtcPeerConnection`].
///
/// # Arguments
/// - `stream` — accepted Unix socket stream
/// - `network_magic` — expected network magic (connections with different magic are refused)
pub async fn ntc_accept(
    stream: UnixStream,
    network_magic: u32,
) -> Result<NtcPeerConnection, NtcPeerError> {
    let (read_half, write_half) = stream.into_split();

    let mut protocols_init: Vec<MiniProtocolNum> =
        vec![MiniProtocolNum::HANDSHAKE];
    protocols_init.extend_from_slice(&NTC_PROTOCOLS);

    let mux = start_mux(read_half, write_half, &protocols_init)
        .map_err(|e| NtcPeerError::Mux(e.to_string()))?;

    let mut handles = mux.handles.clone();

    let hs_handle = handles
        .remove(&MiniProtocolNum::HANDSHAKE)
        .ok_or_else(|| NtcPeerError::Mux("no handshake handle".to_owned()))?;
    let mut hs_channel = MessageChannel::new(hs_handle);

    // Receive ProposeVersions
    let proposal = hs_channel
        .recv()
        .await
        .ok_or(NtcPeerError::Mux("handshake channel closed".to_owned()))?
        .map_err(|e| NtcPeerError::Mux(e.to_string()))?;

    // Parse proposal and select highest supported version
    let (selected_version, client_vd) = parse_ntc_propose_versions(&proposal, network_magic)?;

    // Send AcceptVersion
    let accept = build_ntc_accept_version(selected_version, network_magic, client_vd.query);
    hs_channel
        .send(accept)
        .await
        .map_err(|e| NtcPeerError::Mux(e.to_string()))?;

    let version_data = NodeToClientVersionData {
        network_magic,
        query: client_vd.query,
    };

    let protocols: HashMap<MiniProtocolNum, ProtocolHandle> = NTC_PROTOCOLS
        .iter()
        .filter_map(|num| handles.remove(num).map(|h| (*num, h)))
        .collect();

    Ok(NtcPeerConnection {
        version: HandshakeVersion(selected_version),
        version_data,
        protocols,
        mux: mux.mux_handle,
    })
}

/// Parse a client's ProposeVersions message and select the highest supported version.
fn parse_ntc_propose_versions(
    data: &[u8],
    expected_magic: u32,
) -> Result<(u16, NodeToClientVersionData), NtcPeerError> {
    let mut dec = minicbor::Decoder::new(data);
    dec.array().map_err(|e| NtcPeerError::Cbor(e.to_string()))?;
    let tag: u64 = dec.decode().map_err(|e| NtcPeerError::Cbor(e.to_string()))?;
    if tag != 0 {
        return Err(NtcPeerError::Cbor(format!("expected ProposeVersions tag 0, got {tag}")));
    }
    let n = dec.map().map_err(|e| NtcPeerError::Cbor(e.to_string()))?;
    let count = n.unwrap_or(0) as usize;

    let supported = [9u16, 10, 11, 12, 13, 14, 15, 16];
    let mut best: Option<(u16, NodeToClientVersionData)> = None;

    for _ in 0..count {
        let ver: u64 = dec.decode().map_err(|e| NtcPeerError::Cbor(e.to_string()))?;
        let vd_bytes: minicbor::bytes::ByteVec =
            dec.decode().map_err(|e| NtcPeerError::Cbor(e.to_string()))?;

        let v = ver as u16;
        if supported.contains(&v) {
            if let Ok(vd) = decode_ntc_version_data(&vd_bytes) {
                if vd.network_magic == expected_magic {
                    if best.as_ref().map_or(true, |(bv, _)| v > *bv) {
                        best = Some((v, vd));
                    }
                }
            }
        }
    }

    best.ok_or(NtcPeerError::VersionMismatch)
}

/// Build an `AcceptVersion` CBOR payload.
fn build_ntc_accept_version(version: u16, magic: u32, query: bool) -> Vec<u8> {
    let vd = encode_ntc_version_data(magic, query);
    let mut buf = Vec::new();
    let mut enc = minicbor::Encoder::new(&mut buf);
    enc.array(3).expect("infallible");
    enc.u64(1).expect("infallible"); // AcceptVersion tag
    enc.u64(u64::from(version)).expect("infallible");
    enc.bytes(&vd).expect("infallible");
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_ntc_version_data() {
        let vd = NodeToClientVersionData {
            network_magic: 764_824_073,
            query: false,
        };
        let encoded = encode_ntc_version_data(vd.network_magic, vd.query);
        let decoded = decode_ntc_version_data(&encoded).unwrap();
        assert_eq!(decoded, vd);
    }

    #[test]
    fn encode_decode_ntc_version_data_query_mode() {
        let vd = NodeToClientVersionData {
            network_magic: 1,
            query: true,
        };
        let encoded = encode_ntc_version_data(vd.network_magic, vd.query);
        let decoded = decode_ntc_version_data(&encoded).unwrap();
        assert_eq!(decoded, vd);
    }
}
