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
use yggdrasil_ledger::LedgerError;
use yggdrasil_ledger::cbor::{Decoder, Encoder};

use crate::handshake::HandshakeVersion;
use crate::multiplexer::{MiniProtocolDir, MiniProtocolNum};
use crate::mux::{self, MuxHandle, ProtocolHandle};

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
    pub const NTC_V9: Self = Self(9);
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

/// NtC mini-protocol set including the handshake (protocol 0).
const NTC_PROTOCOLS: [MiniProtocolNum; 4] = [
    MiniProtocolNum::HANDSHAKE,
    MiniProtocolNum::NTC_LOCAL_TX_SUBMISSION,
    MiniProtocolNum::NTC_LOCAL_STATE_QUERY,
    MiniProtocolNum::NTC_LOCAL_TX_MONITOR,
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
#[cfg(test)]
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
// NtC handshake CBOR helpers
// ---------------------------------------------------------------------------

/// Supported NtC versions (sorted descending for selection).
const NTC_SUPPORTED_VERSIONS: [HandshakeVersion; 8] = [
    HandshakeVersion::NTC_V16,
    HandshakeVersion::NTC_V15,
    HandshakeVersion::NTC_V14,
    HandshakeVersion::NTC_V13,
    HandshakeVersion::NTC_V12,
    HandshakeVersion::NTC_V11,
    HandshakeVersion::NTC_V10,
    HandshakeVersion::NTC_V9,
];

/// Decode `ProposeVersions [0, versionTable]` from raw CBOR.
///
/// Returns a list of `(version, version_data)` pairs.  The version data
/// values are decoded with [`decode_ntc_version_data`].
fn decode_ntc_propose_versions(
    bytes: &[u8],
) -> Result<Vec<(HandshakeVersion, NodeToClientVersionData)>, NtcPeerError> {
    let cbor_err = |e: LedgerError| NtcPeerError::Cbor(e.to_string());
    let mut dec = Decoder::new(bytes);
    let msg_len = dec.array().map_err(cbor_err)?;
    if msg_len < 2 {
        return Err(NtcPeerError::Cbor(format!(
            "NtC handshake message too short: {msg_len}"
        )));
    }
    let tag = dec.unsigned().map_err(cbor_err)?;
    if tag != 0 {
        return Err(NtcPeerError::HandshakeRefused(format!(
            "expected ProposeVersions (tag 0), got tag {tag}"
        )));
    }
    // versionTable is a map { versionNumber => versionData }
    let map_len = dec.map().map_err(cbor_err)?;
    let mut proposals = Vec::with_capacity(map_len as usize);
    for _ in 0..map_len {
        let ver_num = dec.unsigned().map_err(cbor_err)? as u16;
        // Version data is encoded inline as a CBOR array.
        let vd_start = dec.position();
        // Skip one CBOR item to measure the version-data bytes.
        dec.skip().map_err(cbor_err)?;
        let vd_end = dec.position();
        let vd_bytes = &bytes[vd_start..vd_end];
        match decode_ntc_version_data(vd_bytes) {
            Ok(vd) => proposals.push((HandshakeVersion(ver_num), vd)),
            Err(_) => {
                // Skip undecodable version data — server just won't select
                // this version, matching upstream behavior.
                continue;
            }
        }
    }
    Ok(proposals)
}

/// Encode `AcceptVersion [1, versionNumber, versionData]` as CBOR.
fn encode_ntc_accept_version(version: HandshakeVersion, data: &NodeToClientVersionData) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(3).unsigned(1).unsigned(version.0 as u64);
    // Inline version data array.
    enc.array(2)
        .unsigned(data.network_magic as u64)
        .bool(data.query);
    enc.into_bytes()
}

/// Encode `Refuse [2, [0, [*versionNumber]]]` as CBOR (VersionMismatch).
fn encode_ntc_refuse_version_mismatch(proposed: &[HandshakeVersion]) -> Vec<u8> {
    let mut enc = Encoder::new();
    enc.array(2).unsigned(2);
    // refuseReason: [0, [*versionNumber]]
    enc.array(2).unsigned(0);
    enc.array(proposed.len() as u64);
    for v in proposed {
        enc.unsigned(v.0 as u64);
    }
    enc.into_bytes()
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
        "NtC client-side connect is not yet implemented",
    ))
}

/// Accept an inbound NtC connection from a Unix socket stream.
///
/// Runs the server side of the NtC handshake, selecting the highest common
/// protocol version whose `network_magic` matches, then returns a
/// [`NtcPeerConnection`] with handles for the NtC data mini-protocols.
///
/// Reference: `Ouroboros.Network.NodeToClient` — `NodeToClient.accept`.
///
/// # Arguments
/// - `stream` — accepted Unix socket stream
/// - `network_magic` — expected network magic (connections with different magic are refused)
pub async fn ntc_accept(
    stream: UnixStream,
    network_magic: u32,
) -> Result<NtcPeerConnection, NtcPeerError> {
    let (mut handles, mux_handle) =
        mux::start_unix(stream, MiniProtocolDir::Responder, &NTC_PROTOCOLS, 32);

    // Take the handshake handle — consumed during negotiation.
    let mut hs = handles
        .remove(&MiniProtocolNum::HANDSHAKE)
        .expect("handshake handle must be registered");

    // Receive ProposeVersions from the client.
    let propose_bytes = hs.recv().await.ok_or_else(|| {
        NtcPeerError::Io(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "connection closed before NtC handshake",
        ))
    })?;

    let proposed = decode_ntc_propose_versions(&propose_bytes)?;

    // Select the highest version that we support and whose network magic matches.
    for &our_ver in &NTC_SUPPORTED_VERSIONS {
        if let Some((_, vd)) = proposed
            .iter()
            .find(|(v, vd)| *v == our_ver && vd.network_magic == network_magic)
        {
            let accept_bytes = encode_ntc_accept_version(our_ver, vd);
            hs.send(accept_bytes).await.map_err(|e| {
                NtcPeerError::Io(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    e.to_string(),
                ))
            })?;

            return Ok(NtcPeerConnection {
                version: our_ver,
                version_data: vd.clone(),
                protocols: handles,
                mux: mux_handle,
            });
        }
    }

    // No compatible version — refuse.
    let refuse_bytes =
        encode_ntc_refuse_version_mismatch(&proposed.iter().map(|(v, _)| *v).collect::<Vec<_>>());
    let _ = hs.send(refuse_bytes).await;

    Err(NtcPeerError::VersionMismatch)
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

    /// Build a ProposeVersions CBOR message for testing.
    fn build_propose_versions(versions: &[(u16, u32, bool)]) -> Vec<u8> {
        let mut enc = Encoder::new();
        enc.array(2).unsigned(0); // tag 0 = ProposeVersions
        enc.map(versions.len() as u64);
        for &(ver, magic, query) in versions {
            enc.unsigned(ver as u64);
            enc.array(2).unsigned(magic as u64).bool(query);
        }
        enc.into_bytes()
    }

    #[test]
    fn decode_ntc_propose_versions_roundtrip() {
        let bytes = build_propose_versions(&[
            (16, 764_824_073, false),
            (15, 764_824_073, false),
            (9, 764_824_073, true),
        ]);
        let proposals = decode_ntc_propose_versions(&bytes).unwrap();
        assert_eq!(proposals.len(), 3);
        assert_eq!(proposals[0].0, HandshakeVersion::NTC_V16);
        assert_eq!(proposals[0].1.network_magic, 764_824_073);
        assert!(!proposals[0].1.query);
        assert_eq!(proposals[2].0, HandshakeVersion::NTC_V9);
        assert!(proposals[2].1.query);
    }

    #[test]
    fn decode_ntc_propose_versions_wrong_magic_skipped_gracefully() {
        // All proposed versions have matching format but different magic.
        let bytes = build_propose_versions(&[(16, 1, false), (15, 2, false)]);
        let proposals = decode_ntc_propose_versions(&bytes).unwrap();
        assert_eq!(proposals.len(), 2);
        assert_eq!(proposals[0].1.network_magic, 1);
        assert_eq!(proposals[1].1.network_magic, 2);
    }

    #[test]
    fn encode_ntc_accept_version_roundtrip() {
        let vd = NodeToClientVersionData {
            network_magic: 1,
            query: true,
        };
        let bytes = encode_ntc_accept_version(HandshakeVersion::NTC_V16, &vd);
        // Decode: [1, 16, [1, true]]
        let mut dec = Decoder::new(&bytes);
        let len = dec.array().unwrap();
        assert_eq!(len, 3);
        let tag = dec.unsigned().unwrap();
        assert_eq!(tag, 1);
        let ver = dec.unsigned().unwrap() as u16;
        assert_eq!(ver, 16);
        let vd_len = dec.array().unwrap();
        assert_eq!(vd_len, 2);
        let magic = dec.unsigned().unwrap() as u32;
        assert_eq!(magic, 1);
        let query = dec.bool().unwrap();
        assert!(query);
    }

    #[test]
    fn encode_ntc_refuse_version_mismatch_is_valid_cbor() {
        let bytes = encode_ntc_refuse_version_mismatch(&[
            HandshakeVersion::NTC_V16,
            HandshakeVersion::NTC_V15,
        ]);
        let mut dec = Decoder::new(&bytes);
        let len = dec.array().unwrap();
        assert_eq!(len, 2);
        let tag = dec.unsigned().unwrap();
        assert_eq!(tag, 2); // Refuse
        let reason_len = dec.array().unwrap();
        assert_eq!(reason_len, 2);
        let reason_tag = dec.unsigned().unwrap();
        assert_eq!(reason_tag, 0); // VersionMismatch
        let versions_len = dec.array().unwrap();
        assert_eq!(versions_len, 2);
    }
}
