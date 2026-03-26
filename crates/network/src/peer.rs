//! Peer connection lifecycle — handshake negotiation over the multiplexer
//! followed by data-protocol dispatch.
//!
//! A peer connection proceeds through three stages:
//!
//! 1. **TCP connect** — establish a bidirectional byte stream.
//! 2. **Handshake** — run the Handshake mini-protocol (protocol 0) through
//!    the multiplexer to negotiate a common version and exchange network
//!    parameters.
//! 3. **Data protocols** — on success, the remaining mini-protocol handles
//!    (ChainSync, BlockFetch, TxSubmission2, KeepAlive) are ready for use.
//!
//! Reference: `ouroboros-network-framework/src/Ouroboros/Network/Protocol/Handshake/`.

use std::collections::HashMap;

use crate::handshake::{
    HandshakeMessage, HandshakeVersion, NodeToNodeVersionData, RefuseReason,
};
use crate::multiplexer::MiniProtocolNum;
use crate::mux::{self, MuxError, MuxHandle, ProtocolHandle};
use crate::multiplexer::MiniProtocolDir;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors from peer connection setup.
#[derive(Debug, thiserror::Error)]
pub enum PeerError {
    /// Multiplexer I/O error.
    #[error("mux error: {0}")]
    Mux(#[from] MuxError),

    /// TCP connection error.
    #[error("connection error: {0}")]
    Io(#[from] std::io::Error),

    /// The remote peer refused the handshake.
    #[error("handshake refused: {reason:?}")]
    Refused {
        /// Why the handshake was refused.
        reason: RefuseReason,
    },

    /// No proposed version matches our supported set.
    #[error("no compatible version found")]
    NoCompatibleVersion,

    /// Unexpected message or protocol violation during handshake.
    #[error("handshake protocol error: {detail}")]
    HandshakeProtocol {
        /// Description of the violation.
        detail: String,
    },
}

// ---------------------------------------------------------------------------
// PeerConnection — result of a successful handshake
// ---------------------------------------------------------------------------

/// A negotiated peer connection with active mini-protocol handles.
///
/// After a successful handshake the caller owns per-protocol
/// [`ProtocolHandle`]s for exchanging messages with the remote peer, and a
/// [`MuxHandle`] that controls the background mux/demux tasks.
pub struct PeerConnection {
    /// The negotiated protocol version.
    pub version: HandshakeVersion,
    /// The agreed-upon version parameters.
    pub version_data: NodeToNodeVersionData,
    /// Per-data-protocol handles, keyed by mini-protocol number.
    ///
    /// Contains only the data protocols — the handshake handle is consumed
    /// during negotiation.
    pub protocols: HashMap<MiniProtocolNum, ProtocolHandle>,
    /// Handle to the background mux/demux tasks.
    pub mux: MuxHandle,
}

// ---------------------------------------------------------------------------
// Standard node-to-node data protocols
// ---------------------------------------------------------------------------

/// The set of mini-protocol numbers registered for a standard node-to-node
/// connection: Handshake (for negotiation) plus the five data protocols.
///
/// Includes PeerSharing (10) so warm peers can be queried for peer discovery
/// once promoted to hot.  Reference: `Ouroboros.Network.NodeToNode` —
/// `NodeToNodeProtocols`.
const N2N_PROTOCOLS: [MiniProtocolNum; 6] = [
    MiniProtocolNum::HANDSHAKE,
    MiniProtocolNum::CHAIN_SYNC,
    MiniProtocolNum::BLOCK_FETCH,
    MiniProtocolNum::TX_SUBMISSION,
    MiniProtocolNum::KEEP_ALIVE,
    MiniProtocolNum::PEER_SHARING,
];

/// Default per-protocol channel buffer capacity.
const DEFAULT_BUFFER: usize = 16;

// ---------------------------------------------------------------------------
// connect — initiator side
// ---------------------------------------------------------------------------

/// Connect to a remote peer, run the handshake, and return a negotiated
/// connection.
///
/// The initiator proposes the given `(version, version_data)` pairs.  The
/// responder selects one via `AcceptVersion`, or refuses.
///
/// On success the returned [`PeerConnection`] contains handles for the four
/// node-to-node data protocols.
///
/// Reference: `ouroboros-network/src/Ouroboros/Network/NodeToNode.hs`.
pub async fn connect(
    addr: impl tokio::net::ToSocketAddrs,
    proposals: Vec<(HandshakeVersion, NodeToNodeVersionData)>,
) -> Result<PeerConnection, PeerError> {
    let stream = tokio::net::TcpStream::connect(addr).await?;

    let (mut handles, mux_handle) =
        mux::start(stream, MiniProtocolDir::Initiator, &N2N_PROTOCOLS, DEFAULT_BUFFER);

    // Take the handshake handle — it will be consumed during negotiation.
    let mut hs = handles
        .remove(&MiniProtocolNum::HANDSHAKE)
        .expect("handshake handle must be registered");

    // Send ProposeVersions.
    let propose = HandshakeMessage::ProposeVersions(proposals);
    hs.send(propose.to_cbor()).await?;

    // Receive the server's response.
    let response_bytes = hs.recv().await.ok_or_else(|| PeerError::HandshakeProtocol {
        detail: "connection closed before handshake response".into(),
    })?;

    let response =
        HandshakeMessage::from_cbor(&response_bytes).map_err(|e| PeerError::HandshakeProtocol {
            detail: format!("CBOR decode error: {e}"),
        })?;

    match response {
        HandshakeMessage::AcceptVersion(version, version_data) => Ok(PeerConnection {
            version,
            version_data,
            protocols: handles,
            mux: mux_handle,
        }),
        HandshakeMessage::Refuse(reason) => Err(PeerError::Refused { reason }),
        other => Err(PeerError::HandshakeProtocol {
            detail: format!("unexpected handshake message: {}", other.tag_name()),
        }),
    }
}

// ---------------------------------------------------------------------------
// accept — responder side
// ---------------------------------------------------------------------------

/// Accept an incoming connection, run the handshake, and return a negotiated
/// connection.
///
/// The responder receives the client's `ProposeVersions`, selects the highest
/// common version whose `network_magic` matches, and replies with
/// `AcceptVersion`.  If no compatible version is found, the handshake is
/// refused with [`RefuseReason::VersionMismatch`].
///
/// Reference: `ouroboros-network/src/Ouroboros/Network/NodeToNode.hs`.
pub async fn accept(
    stream: tokio::net::TcpStream,
    network_magic: u32,
    supported_versions: &[HandshakeVersion],
) -> Result<PeerConnection, PeerError> {
    let (mut handles, mux_handle) =
        mux::start(stream, MiniProtocolDir::Responder, &N2N_PROTOCOLS, DEFAULT_BUFFER);

    let mut hs = handles
        .remove(&MiniProtocolNum::HANDSHAKE)
        .expect("handshake handle must be registered");

    // Receive ProposeVersions from the client.
    let propose_bytes = hs.recv().await.ok_or_else(|| PeerError::HandshakeProtocol {
        detail: "connection closed before handshake proposal".into(),
    })?;

    let propose =
        HandshakeMessage::from_cbor(&propose_bytes).map_err(|e| PeerError::HandshakeProtocol {
            detail: format!("CBOR decode error: {e}"),
        })?;

    let proposed = match propose {
        HandshakeMessage::ProposeVersions(versions) => versions,
        other => {
            return Err(PeerError::HandshakeProtocol {
                detail: format!("expected ProposeVersions, got {}", other.tag_name()),
            })
        }
    };

    // Select the highest version that we support and whose network magic
    // matches.  Iterate our supported versions from highest to lowest.
    let mut sorted_supported: Vec<HandshakeVersion> = supported_versions.to_vec();
    sorted_supported.sort_unstable_by(|a, b| b.0.cmp(&a.0)); // descending

    for &our_ver in &sorted_supported {
        if let Some((_, vdata)) = proposed
            .iter()
            .find(|(v, vd)| *v == our_ver && vd.network_magic == network_magic)
        {
            let accept_msg = HandshakeMessage::AcceptVersion(our_ver, vdata.clone());
            hs.send(accept_msg.to_cbor()).await?;

            return Ok(PeerConnection {
                version: our_ver,
                version_data: vdata.clone(),
                protocols: handles,
                mux: mux_handle,
            });
        }
    }

    // No compatible version — refuse.
    let refuse = HandshakeMessage::Refuse(RefuseReason::VersionMismatch(
        proposed.iter().map(|(v, _)| *v).collect(),
    ));
    hs.send(refuse.to_cbor()).await?;

    Err(PeerError::NoCompatibleVersion)
}
