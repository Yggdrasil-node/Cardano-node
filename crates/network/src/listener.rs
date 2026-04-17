//! TCP listener for inbound peer connections.
//!
//! Wraps a bound [`tokio::net::TcpListener`] and provides an
//! [`accept_peer`](PeerListener::accept_peer) method that performs the
//! Ouroboros handshake before returning a ready [`PeerConnection`].
//!
//! Reference: `ouroboros-network-framework` inbound-governor socket accept path.

use crate::handshake::HandshakeVersion;
use crate::peer::{self, PeerConnection, PeerError};
use std::net::SocketAddr;
use tokio::net::TcpListener;

// ---------------------------------------------------------------------------
// PeerListener
// ---------------------------------------------------------------------------

/// A TCP listener that accepts inbound Ouroboros connections.
///
/// Each accepted connection goes through version-negotiated handshake
/// before the protocol handles become available.
///
/// ```text
/// bind(addr) → PeerListener
///   ↓
/// accept_peer() → PeerConnection (handshake done, protocol handles ready)
/// ```
pub struct PeerListener {
    listener: TcpListener,
    network_magic: u32,
    supported_versions: Vec<HandshakeVersion>,
}

impl PeerListener {
    /// Bind a TCP listener to the given address.
    pub async fn bind(
        addr: impl tokio::net::ToSocketAddrs,
        network_magic: u32,
        supported_versions: Vec<HandshakeVersion>,
    ) -> Result<Self, PeerListenerError> {
        let listener = TcpListener::bind(addr)
            .await
            .map_err(PeerListenerError::Bind)?;
        Ok(Self {
            listener,
            network_magic,
            supported_versions,
        })
    }

    /// Create a listener from an already-bound `TcpListener`.
    pub fn from_listener(
        listener: TcpListener,
        network_magic: u32,
        supported_versions: Vec<HandshakeVersion>,
    ) -> Self {
        Self {
            listener,
            network_magic,
            supported_versions,
        }
    }

    /// Returns the local address this listener is bound to.
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// Accept the next inbound connection, perform the handshake, and
    /// return a ready [`PeerConnection`].
    ///
    /// On handshake failure (version mismatch, decode error) the TCP
    /// connection is dropped and an error is returned. Callers should
    /// typically log the error and continue accepting.
    pub async fn accept_peer(&self) -> Result<(PeerConnection, SocketAddr), PeerListenerError> {
        let (stream, addr) = self
            .listener
            .accept()
            .await
            .map_err(PeerListenerError::Accept)?;

        let conn = peer::accept(stream, self.network_magic, &self.supported_versions)
            .await
            .map_err(|e| PeerListenerError::Handshake { addr, source: e })?;

        Ok((conn, addr))
    }
}

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

/// Errors from the peer listener.
#[derive(Debug, thiserror::Error)]
pub enum PeerListenerError {
    /// Failed to bind the TCP listener.
    #[error("bind error: {0}")]
    Bind(std::io::Error),

    /// Failed to accept a TCP connection.
    #[error("accept error: {0}")]
    Accept(std::io::Error),

    /// Handshake failed after TCP connection was accepted.
    #[error("handshake with {addr} failed: {source}")]
    Handshake { addr: SocketAddr, source: PeerError },
}
