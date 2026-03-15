//! Inbound peer session handling — server-side protocol orchestration.
//!
//! When the node accepts an inbound connection via [`PeerListener`], the
//! resulting [`PeerConnection`] contains protocol handles for the four data
//! mini-protocols.  This module provides helpers to wrap those handles in
//! server drivers and run them concurrently for a single inbound peer.
//!
//! The session runs until the remote peer disconnects or the node shuts
//! down.  Each protocol runs as an independent tokio task so a slow
//! BlockFetch batch does not stall KeepAlive responses.
//!
//! Reference: `ouroboros-network-framework`'s inbound-governor session
//! lifecycle.

use std::net::SocketAddr;

use yggdrasil_network::{
    BlockFetchServer, BlockFetchServerError, BlockFetchServerRequest,
    ChainSyncServer, ChainSyncServerError, ChainSyncServerRequest,
    KeepAliveServer, KeepAliveServerError,
    MuxHandle, PeerConnection, PeerListener, PeerListenerError,
    TxSubmissionServer,
};
use yggdrasil_network::multiplexer::MiniProtocolNum;

// ---------------------------------------------------------------------------
// InboundPeerSession
// ---------------------------------------------------------------------------

/// Server-side protocol drivers for a single accepted inbound peer.
///
/// Constructed from a [`PeerConnection`] returned by [`PeerListener::accept_peer`].
pub struct InboundPeerSession {
    /// ChainSync server driver.
    pub chain_sync: ChainSyncServer,
    /// BlockFetch server driver.
    pub block_fetch: BlockFetchServer,
    /// KeepAlive server driver.
    pub keep_alive: KeepAliveServer,
    /// TxSubmission server driver (server-driven request flow).
    pub tx_submission: TxSubmissionServer,
    /// Mux handle for aborting all background tasks on shutdown.
    pub mux: MuxHandle,
    /// Remote peer address.
    pub remote_addr: SocketAddr,
}

impl InboundPeerSession {
    /// Build an inbound session from an accepted [`PeerConnection`].
    ///
    /// Consumes the per-protocol handles from the connection and wraps
    /// them in server drivers.  Returns `None` if any required protocol
    /// handle is missing.
    pub fn from_connection(
        mut conn: PeerConnection,
        remote_addr: SocketAddr,
    ) -> Option<Self> {
        let cs = conn.protocols.remove(&MiniProtocolNum::CHAIN_SYNC)?;
        let bf = conn.protocols.remove(&MiniProtocolNum::BLOCK_FETCH)?;
        let ka = conn.protocols.remove(&MiniProtocolNum::KEEP_ALIVE)?;
        let ts = conn.protocols.remove(&MiniProtocolNum::TX_SUBMISSION)?;
        Some(Self {
            chain_sync: ChainSyncServer::new(cs),
            block_fetch: BlockFetchServer::new(bf),
            keep_alive: KeepAliveServer::new(ka),
            tx_submission: TxSubmissionServer::new(ts),
            mux: conn.mux,
            remote_addr,
        })
    }
}

// ---------------------------------------------------------------------------
// KeepAlive server task
// ---------------------------------------------------------------------------

/// Run the KeepAlive echo loop until the client sends `MsgDone` or the
/// connection drops.
pub async fn run_keepalive_server(
    mut server: KeepAliveServer,
) -> Result<(), KeepAliveServerError> {
    loop {
        match server.recv_keep_alive().await? {
            Some(cookie) => server.respond(cookie).await?,
            None => return Ok(()), // client sent MsgDone
        }
    }
}

// ---------------------------------------------------------------------------
// BlockFetch server task (storage-backed)
// ---------------------------------------------------------------------------

/// A trait for looking up raw block bytes by hash range.
///
/// The node layer implements this over its storage backend (e.g. `ChainDb`).
pub trait BlockProvider: Send + Sync {
    /// Look up blocks in the given range `[from, to]` (inclusive).
    ///
    /// Returns the raw CBOR bytes for each block in chain order, or an
    /// empty vec if the range is unavailable.
    fn get_block_range(
        &self,
        from: &[u8],
        to: &[u8],
    ) -> Vec<Vec<u8>>;
}

/// Run the BlockFetch server loop, serving blocks from a [`BlockProvider`].
pub async fn run_blockfetch_server(
    mut server: BlockFetchServer,
    provider: &dyn BlockProvider,
) -> Result<(), BlockFetchServerError> {
    loop {
        match server.recv_request().await? {
            BlockFetchServerRequest::RequestRange(range) => {
                let blocks = provider.get_block_range(&range.lower, &range.upper);
                server.serve_batch(blocks).await?;
            }
            BlockFetchServerRequest::ClientDone => return Ok(()),
        }
    }
}

// ---------------------------------------------------------------------------
// ChainSync server task (storage-backed)
// ---------------------------------------------------------------------------

/// A trait for serving chain headers and finding intersections.
///
/// The node layer implements this over its storage + consensus state.
pub trait ChainProvider: Send + Sync {
    /// Return the current chain tip as CBOR-encoded point.
    fn chain_tip(&self) -> Vec<u8>;

    /// Given the cursor (last sent point), return the next roll-forward
    /// header + tip, or `None` if at tip.
    fn next_header(&self, cursor: &Option<Vec<u8>>) -> Option<(Vec<u8>, Vec<u8>)>;

    /// Find the best intersection from the client's candidate points.
    ///
    /// Returns `(found_point, tip)` or `None` if no intersection.
    fn find_intersect(&self, points: &[Vec<u8>]) -> Option<(Vec<u8>, Vec<u8>)>;
}

/// Run the ChainSync server loop, serving headers from a [`ChainProvider`].
pub async fn run_chainsync_server(
    mut server: ChainSyncServer,
    provider: &dyn ChainProvider,
) -> Result<(), ChainSyncServerError> {
    let mut cursor: Option<Vec<u8>> = None;

    loop {
        match server.recv_request().await? {
            ChainSyncServerRequest::RequestNext => {
                match provider.next_header(&cursor) {
                    Some((header, tip)) => {
                        cursor = Some(header.clone());
                        server.roll_forward(header, tip).await?;
                    }
                    None => {
                        // No new data — tell client to wait, then block until
                        // data arrives (simplified: immediate await + retry).
                        server.await_reply().await?;
                        // In a production server this would use a notification
                        // channel from the chain-sync pipeline. For now, yield
                        // and return the tip when the provider has data.
                        loop {
                            tokio::task::yield_now().await;
                            if let Some((header, tip)) = provider.next_header(&cursor) {
                                cursor = Some(header.clone());
                                server.roll_forward(header, tip).await?;
                                break;
                            }
                        }
                    }
                }
            }
            ChainSyncServerRequest::FindIntersect { points } => {
                match provider.find_intersect(&points) {
                    Some((point, tip)) => {
                        cursor = Some(point.clone());
                        server.intersect_found(point, tip).await?;
                    }
                    None => {
                        let tip = provider.chain_tip();
                        server.intersect_not_found(tip).await?;
                    }
                }
            }
            ChainSyncServerRequest::Done => return Ok(()),
        }
    }
}

// ---------------------------------------------------------------------------
// Inbound listener loop
// ---------------------------------------------------------------------------

/// Errors from the inbound listener service.
#[derive(Debug, thiserror::Error)]
pub enum InboundServiceError {
    /// Listener setup failed.
    #[error("listener error: {0}")]
    Listener(#[from] PeerListenerError),

    /// A protocol handle was missing from the accepted connection.
    #[error("missing protocol handle for inbound peer {addr}")]
    MissingProtocol { addr: SocketAddr },
}

/// Run the inbound connection accept loop.
///
/// Accepts connections on the given [`PeerListener`], builds an
/// [`InboundPeerSession`] for each, and spawns the KeepAlive server task.
/// Other protocol tasks (ChainSync, BlockFetch, TxSubmission) require
/// storage/mempool providers and are spawned only when providers are
/// available.
///
/// The loop runs until the `shutdown` future resolves or a fatal listener
/// error occurs.
pub async fn run_inbound_accept_loop<F: std::future::Future<Output = ()>>(
    listener: &PeerListener,
    shutdown: F,
) -> Result<(), InboundServiceError> {
    tokio::pin!(shutdown);

    loop {
        tokio::select! {
            biased;
            _ = &mut shutdown => return Ok(()),
            result = listener.accept_peer() => {
                let (conn, addr) = result?;

                let session = InboundPeerSession::from_connection(conn, addr)
                    .ok_or(InboundServiceError::MissingProtocol { addr })?;

                // Spawn KeepAlive echo — no external state needed.
                tokio::spawn(async move {
                    let _ = run_keepalive_server(session.keep_alive).await;
                    // BlockFetch, ChainSync, TxSubmission server tasks will be
                    // spawned here once storage/mempool providers are wired in.
                    session.mux.abort();
                });
            }
        }
    }
}
