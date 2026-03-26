//! Node-to-client (NtC) connection lifecycle.
//!
//! Defines the node-to-client (NtC) protocol surface and version-data helpers.
//!
//! Full Unix-socket NtC connection setup is not yet wired because the current
//! mux implementation only supports `TcpStream`. The public `ntc_connect()` and
//! `ntc_accept()` functions therefore return an explicit unsupported error until
//! Unix-socket bearer support lands.
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
use yggdrasil_ledger::cbor::{Decoder, Encoder};
use yggdrasil_ledger::LedgerError;

use crate::mux::{MuxHandle, ProtocolHandle};
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
#[allow(dead_code)]
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
    #[error("unsupported: {0}")]
    Unsupported(&'static str),
}

// ---------------------------------------------------------------------------
// NtC handshake helpers
// ---------------------------------------------------------------------------

/// Encode NtC version data as CBOR `[network_magic, query]`.
fn encode_ntc_version_data(magic: u32, query: bool) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(2).unsigned(magic as u64).bool(query);
    enc.into_bytes()
}

/// Decode NtC version data from CBOR `[network_magic, query]`.
fn decode_ntc_version_data(data: &[u8]) -> Result<NodeToClientVersionData, NtcPeerError> {
    let cbor_err = |e: LedgerError| NtcPeerError::Cbor(e.to_string());
    let mut dec = Decoder::new(data);
    let len = dec.array().map_err(cbor_err)?;
    if len != 2 {
        return Err(NtcPeerError::Cbor(format!(
            "unexpected NtC version-data length {len}; expected 2"
        )));
    }
    let magic = dec.unsigned().map_err(cbor_err)?;
    let query = dec.bool().map_err(cbor_err)?;
    Ok(NodeToClientVersionData {
        network_magic: magic as u32,
        query,
    })
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
    let _ = (socket_path.as_ref(), network_magic, query_only);
    Err(NtcPeerError::Unsupported(
        "NtC Unix-socket transport is not yet supported by the current mux implementation",
    ))
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
    let _ = (stream, network_magic);
    Err(NtcPeerError::Unsupported(
        "NtC Unix-socket transport is not yet supported by the current mux implementation",
    ))
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
