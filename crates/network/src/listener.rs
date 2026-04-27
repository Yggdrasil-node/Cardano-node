//! TCP listener for inbound peer connections.
//!
//! Wraps a bound [`tokio::net::TcpListener`] and exposes:
//!
//! * [`PeerListener::accept_tcp`] — accept the next inbound TCP connection
//!   without performing any protocol-level work. Cheap and never blocks
//!   on a misbehaving peer's data; safe to call from a hot accept loop
//!   that wants to apply rate-limiting *before* a handshake runs.
//! * [`PeerListener::handshake_on`] — perform the Ouroboros handshake on
//!   an already-accepted TCP stream, with a hard outer deadline.
//! * [`PeerListener::accept_peer`] — convenience wrapper combining the
//!   two; retained for backwards compatibility.
//!
//! Reference: `ouroboros-network-framework` inbound-governor socket
//! accept path (`Ouroboros.Network.Server2`).

use crate::handshake::HandshakeVersion;
use crate::peer::{self, PeerConnection, PeerError};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};

/// Maximum time a single inbound handshake may take before the outer
/// deadline aborts it. Matches the handshake-timeout constant used by
/// `Ouroboros.Network.Handshake.Acceptable` (5 seconds).
///
/// This is independent of the per-state limits in
/// [`crate::protocol_limits::handshake`] — those bound the time spent in
/// each handshake state, while this caps the entire negotiation.
pub const HANDSHAKE_DEADLINE: Duration = Duration::from_secs(5);

// ---------------------------------------------------------------------------
// PeerListener
// ---------------------------------------------------------------------------

/// A TCP listener that accepts inbound Ouroboros connections.
///
/// ```text
/// bind(addr) → PeerListener
///   ↓
/// accept_tcp() → (TcpStream, SocketAddr)   ← cheap, returns immediately
///   ↓                                        on the first byte of a SYN
/// (rate-limit / connection-manager check goes here)
///   ↓
/// handshake_on(stream, addr) → PeerConnection
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

    /// Accept the next inbound TCP connection without performing the
    /// handshake.
    ///
    /// This is the appropriate primitive for a hot accept loop that
    /// wants to enforce rate limits or connection-manager admission
    /// *before* spending CPU and memory on handshake decoding.
    pub async fn accept_tcp(&self) -> Result<(TcpStream, SocketAddr), PeerListenerError> {
        let (stream, addr) = self
            .listener
            .accept()
            .await
            .map_err(PeerListenerError::Accept)?;
        Ok((stream, addr))
    }

    /// Run the Ouroboros handshake on an already-accepted TCP stream,
    /// bounded by [`HANDSHAKE_DEADLINE`].
    ///
    /// Stalled or slowloris-style peers that fail to send a handshake
    /// proposal within the deadline are dropped cleanly with
    /// [`PeerListenerError::HandshakeTimeout`].
    pub async fn handshake_on(
        &self,
        stream: TcpStream,
        addr: SocketAddr,
    ) -> Result<PeerConnection, PeerListenerError> {
        match tokio::time::timeout(
            HANDSHAKE_DEADLINE,
            peer::accept(stream, self.network_magic, &self.supported_versions),
        )
        .await
        {
            Ok(Ok(conn)) => Ok(conn),
            Ok(Err(e)) => Err(PeerListenerError::Handshake { addr, source: e }),
            Err(_) => Err(PeerListenerError::HandshakeTimeout {
                addr,
                deadline: HANDSHAKE_DEADLINE,
            }),
        }
    }

    /// Accept the next inbound connection and immediately run the
    /// handshake.
    ///
    /// Convenience wrapper combining [`accept_tcp`](Self::accept_tcp) and
    /// [`handshake_on`](Self::handshake_on). Retained for callers that
    /// don't need to interpose rate-limiting between TCP accept and
    /// handshake.  New code should prefer the split form so a slow peer
    /// cannot block the accept loop.
    pub async fn accept_peer(&self) -> Result<(PeerConnection, SocketAddr), PeerListenerError> {
        let (stream, addr) = self.accept_tcp().await?;
        let conn = self.handshake_on(stream, addr).await?;
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

    /// Handshake exceeded the outer deadline.
    #[error("handshake with {addr} exceeded deadline {deadline:?}")]
    HandshakeTimeout {
        addr: SocketAddr,
        deadline: Duration,
    },
}
