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
use std::sync::{Arc, RwLock};

use crate::runtime::{MempoolAddTxResult, add_txs_to_shared_mempool};
use crate::sync::recover_ledger_state_chaindb;
use yggdrasil_ledger::{
    AlonzoBlock, BabbageBlock, ByronBlock, CborDecode, CborEncode, ConwayBlock,
    Decoder, MultiEraSubmittedTx, Point, ShelleyBlock, SlotNo,
};
use yggdrasil_mempool::SharedMempool;
use yggdrasil_network::{
    BlockFetchServer, BlockFetchServerError, BlockFetchServerRequest,
    ChainSyncServer, ChainSyncServerError, ChainSyncServerRequest,
    KeepAliveServer, KeepAliveServerError,
    MuxHandle, PeerConnection, PeerListener, PeerListenerError,
    PeerRegistry, PeerSharingServer, PeerSharingServerError,
    PeerStatus, SharedPeerAddress,
    TxIdsReply, TxSubmissionServer, TxSubmissionServerError,
};
use yggdrasil_network::multiplexer::MiniProtocolNum;
use yggdrasil_storage::{ChainDb, ImmutableStore, VolatileStore, LedgerStore};

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
    /// Optional PeerSharing server driver.
    pub peer_sharing: Option<PeerSharingServer>,
    /// Mux handle for aborting all background tasks on shutdown.
    pub mux: MuxHandle,
    /// Remote peer address.
    pub remote_addr: SocketAddr,
}

/// A provider of peer addresses for the PeerSharing responder.
///
/// Implementations return a list of known shareable peer addresses when a
/// remote node requests peers over mini-protocol 10.
pub trait PeerSharingProvider: Send + Sync {
    /// Return up to `amount` peer addresses to share with the requester.
    fn shareable_peers(&self, amount: u16) -> Vec<SharedPeerAddress>;
}

/// A sink for transactions pulled from an inbound TxSubmission client.
pub trait TxSubmissionConsumer: Send + Sync {
    /// Consume submitted transaction bytes and return the number accepted.
    fn consume_txs(&self, txs: Vec<Vec<u8>>) -> usize;
}

/// Shared `ChainDb` + shared mempool backed TxSubmission consumer.
///
/// This implementation recovers the current ledger state from coordinated
/// storage, decodes submitted transactions using the current ledger era, and
/// then admits them into the shared mempool using the existing runtime helper.
#[derive(Clone)]
pub struct SharedTxSubmissionConsumer<I, V, L> {
    chain_db: Arc<RwLock<ChainDb<I, V, L>>>,
    mempool: SharedMempool,
    evaluator: Option<Arc<dyn yggdrasil_ledger::plutus_validation::PlutusEvaluator + Send + Sync>>,
}

impl<I: std::fmt::Debug, V: std::fmt::Debug, L: std::fmt::Debug> std::fmt::Debug
    for SharedTxSubmissionConsumer<I, V, L>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedTxSubmissionConsumer")
            .field("chain_db", &self.chain_db)
            .field("mempool", &self.mempool)
            .field("evaluator", &self.evaluator.as_ref().map(|_| "<PlutusEvaluator>"))
            .finish()
    }
}

impl<I, V, L> SharedTxSubmissionConsumer<I, V, L> {
    /// Create a new shared TxSubmission consumer from coordinated storage and a mempool.
    pub fn new(chain_db: Arc<RwLock<ChainDb<I, V, L>>>, mempool: SharedMempool) -> Self {
        Self { chain_db, mempool, evaluator: None }
    }

    /// Create a new shared TxSubmission consumer with a Plutus evaluator.
    pub fn with_evaluator(
        chain_db: Arc<RwLock<ChainDb<I, V, L>>>,
        mempool: SharedMempool,
        evaluator: Option<Arc<dyn yggdrasil_ledger::plutus_validation::PlutusEvaluator + Send + Sync>>,
    ) -> Self {
        Self { chain_db, mempool, evaluator }
    }

    /// Shared mempool receiving admitted inbound transactions.
    pub fn mempool(&self) -> &SharedMempool {
        &self.mempool
    }
}

impl<I, V, L> TxSubmissionConsumer for SharedTxSubmissionConsumer<I, V, L>
where
    I: ImmutableStore + Send + Sync,
    V: VolatileStore + Send + Sync,
    L: LedgerStore + Send + Sync,
{
    fn consume_txs(&self, txs: Vec<Vec<u8>>) -> usize {
        let mut ledger_state = {
            let chain_db = match self.chain_db.read() {
                Ok(guard) => guard,
                Err(_) => return 0,
            };
            match recover_ledger_state_chaindb(&chain_db, yggdrasil_ledger::LedgerState::new(yggdrasil_ledger::Era::Byron)) {
                Ok(recovery) => recovery.ledger_state,
                Err(_) => return 0,
            }
        };

        let current_slot = match ledger_state.tip {
            Point::Origin => SlotNo(0),
            Point::BlockPoint(slot, _) => slot,
        };

        let decoded = txs
            .into_iter()
            .filter_map(|raw_tx| {
                MultiEraSubmittedTx::from_cbor_bytes_for_era(ledger_state.current_era, &raw_tx).ok()
            })
            .collect::<Vec<_>>();

        if decoded.is_empty() {
            return 0;
        }

        let eval_ref = self.evaluator.as_ref().map(|e| {
            e.as_ref() as &dyn yggdrasil_ledger::plutus_validation::PlutusEvaluator
        });

        match add_txs_to_shared_mempool(&mut ledger_state, &self.mempool, decoded, current_slot, eval_ref) {
            Ok(results) => results
                .into_iter()
                .filter(|result| matches!(result, MempoolAddTxResult::MempoolTxAdded(_)))
                .count(),
            Err(_) => 0,
        }
    }
}

// ---------------------------------------------------------------------------
// PeerSharing provider backed by a shared PeerRegistry
// ---------------------------------------------------------------------------

/// Shared [`PeerRegistry`]-backed peer-sharing provider that serves warm and
/// hot peers to inbound requester nodes.
///
/// Only peers with status `PeerWarm` or `PeerHot` are returned, matching the
/// upstream policy of advertising established peers only.
#[derive(Clone, Debug)]
pub struct SharedPeerSharingProvider {
    peer_registry: Arc<RwLock<PeerRegistry>>,
}

impl SharedPeerSharingProvider {
    /// Create a new provider from a shared peer registry.
    pub fn new(peer_registry: Arc<RwLock<PeerRegistry>>) -> Self {
        Self { peer_registry }
    }
}

impl PeerSharingProvider for SharedPeerSharingProvider {
    fn shareable_peers(&self, amount: u16) -> Vec<SharedPeerAddress> {
        let registry = match self.peer_registry.read() {
            Ok(guard) => guard,
            Err(_) => return Vec::new(),
        };

        registry
            .iter()
            .filter(|(_, entry)| {
                matches!(
                    entry.status,
                    PeerStatus::PeerWarm | PeerStatus::PeerHot
                )
            })
            .take(amount as usize)
            .map(|(addr, _)| SharedPeerAddress { addr: *addr })
            .collect()
    }
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
        let ps = conn
            .protocols
            .remove(&MiniProtocolNum::PEER_SHARING)
            .map(PeerSharingServer::new);
        Some(Self {
            chain_sync: ChainSyncServer::new(cs),
            block_fetch: BlockFetchServer::new(bf),
            keep_alive: KeepAliveServer::new(ka),
            tx_submission: TxSubmissionServer::new(ts),
            peer_sharing: ps,
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
    /// Look up blocks in the given range `(from, to]`.
    ///
    /// The lower bound is exclusive and the upper bound is inclusive, which
    /// matches BlockFetch usage after ChainSync advances the current point.
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
    /// point + header + tip, or `None` if at tip.
    fn next_header(&self, cursor: &Option<Vec<u8>>) -> Option<(Vec<u8>, Vec<u8>, Vec<u8>)>;

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
                    Some((point, header, tip)) => {
                        cursor = Some(point);
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
                            if let Some((point, header, tip)) = provider.next_header(&cursor) {
                                cursor = Some(point);
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
// TxSubmission server task
// ---------------------------------------------------------------------------

/// Run the TxSubmission server loop, pulling transactions from the remote peer.
///
/// The server requests batches of transaction ids, requests the corresponding
/// bodies, hands those bodies to the provided consumer, then acknowledges the
/// advertised ids on the next request. The loop terminates cleanly when the
/// remote client responds with `MsgDone` to a blocking request.
pub async fn run_txsubmission_server(
    mut server: TxSubmissionServer,
    consumer: &dyn TxSubmissionConsumer,
) -> Result<(), TxSubmissionServerError> {
    const TXSUBMISSION_BATCH_SIZE: u16 = 16;

    server.recv_init().await?;
    let mut ack = 0u16;

    loop {
        match server.request_tx_ids(true, ack, TXSUBMISSION_BATCH_SIZE).await? {
            TxIdsReply::Done => return Ok(()),
            TxIdsReply::TxIds(txids) => {
                ack = txids.len().min(u16::MAX as usize) as u16;
                let requested = txids.into_iter().map(|item| item.txid).collect();
                let txs = server.request_txs(requested).await?;
                let _accepted = consumer.consume_txs(txs);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// PeerSharing server task
// ---------------------------------------------------------------------------

/// Run the PeerSharing server loop, serving known peers from a
/// [`PeerSharingProvider`].
///
/// Terminates when the client sends `MsgDone` or the connection drops.
pub async fn run_peersharing_server(
    mut server: PeerSharingServer,
    provider: &dyn PeerSharingProvider,
) -> Result<(), PeerSharingServerError> {
    server
        .serve_loop(|amount| provider.shareable_peers(amount))
        .await
}

// ---------------------------------------------------------------------------
// ChainDb-backed providers
// ---------------------------------------------------------------------------

/// Shared read handle to a [`ChainDb`] for concurrent provider access.
///
/// Follows the same `Arc<RwLock<T>>` pattern used by [`SharedMempool`] in the
/// mempool crate.  The sync pipeline holds the write lock during block
/// application; inbound server tasks take short-lived read locks for
/// lookups.
///
/// [`SharedMempool`]: yggdrasil_mempool::SharedMempool
#[derive(Clone, Debug)]
pub struct SharedChainDb<I, V, L> {
    inner: Arc<RwLock<ChainDb<I, V, L>>>,
}

impl<I, V, L> SharedChainDb<I, V, L> {
    /// Wrap an existing [`ChainDb`] in a new shared handle.
    pub fn new(chain_db: ChainDb<I, V, L>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(chain_db)),
        }
    }

    /// Create a shared handle from a pre-existing `Arc`.
    pub fn from_arc(arc: Arc<RwLock<ChainDb<I, V, L>>>) -> Self {
        Self { inner: arc }
    }

    /// Obtain a read-only reference to the underlying `Arc<RwLock<_>>`.
    pub fn inner(&self) -> &Arc<RwLock<ChainDb<I, V, L>>> {
        &self.inner
    }
}

impl<I, V, L> BlockProvider for SharedChainDb<I, V, L>
where
    I: ImmutableStore + Send + Sync,
    V: VolatileStore + Send + Sync,
    L: LedgerStore + Send + Sync,
{
    fn get_block_range(&self, from: &[u8], to: &[u8]) -> Vec<Vec<u8>> {
        let from_point = match Point::from_cbor_bytes(from) {
            Ok(p) => p,
            Err(_) => return Vec::new(),
        };
        let to_point = match Point::from_cbor_bytes(to) {
            Ok(p) => p,
            Err(_) => return Vec::new(),
        };
        let to_hash = match to_point.hash() {
            Some(h) => h,
            None => return Vec::new(),
        };

        let db = match self.inner.read() {
            Ok(guard) => guard,
            Err(_) => return Vec::new(),
        };

        // Collect blocks in (from, to] across both stores.
        let mut blocks = Vec::new();

        if let Ok(suffix) = db.immutable().suffix_after(&from_point) {
            blocks.extend(suffix);
        }

        if let Some(pos) = blocks.iter().position(|b| b.header.hash == to_hash) {
            blocks.truncate(pos + 1);
            return blocks.into_iter().filter_map(|b| b.raw_cbor).collect();
        }

        if let Ok(vol_prefix) = db.volatile().prefix_up_to(&to_point) {
            let skip = match from_point.hash() {
                Some(from_hash) => vol_prefix
                    .iter()
                    .position(|b| b.header.hash == from_hash)
                    .map(|pos| pos + 1)
                    .or_else(|| {
                        blocks.last().and_then(|last_immutable| {
                            vol_prefix
                                .iter()
                                .position(|b| b.header.slot_no > last_immutable.header.slot_no)
                        })
                    })
                    .unwrap_or(0),
                None => blocks
                    .last()
                    .and_then(|last_immutable| {
                        vol_prefix
                            .iter()
                            .position(|b| b.header.slot_no > last_immutable.header.slot_no)
                    })
                    .unwrap_or(0),
            };

            blocks.extend(vol_prefix.into_iter().skip(skip));
        }

        if let Some(pos) = blocks.iter().position(|b| b.header.hash == to_hash) {
            blocks.truncate(pos + 1);
            blocks.into_iter().filter_map(|b| b.raw_cbor).collect()
        } else {
            Vec::new()
        }
    }
}

fn block_point(block: &yggdrasil_ledger::Block) -> Point {
    Point::BlockPoint(block.header.slot_no, block.header.hash)
}

fn extract_chainsync_header(raw_block: &[u8]) -> Option<Vec<u8>> {
    mod era_tag {
        pub const BYRON_EBB: u64 = 0;
        pub const BYRON_MAIN: u64 = 1;
        pub const SHELLEY: u64 = 2;
        pub const ALLEGRA: u64 = 3;
        pub const MARY: u64 = 4;
        pub const ALONZO: u64 = 5;
        pub const BABBAGE: u64 = 6;
        pub const CONWAY: u64 = 7;
    }

    let mut dec = Decoder::new(raw_block);
    if dec.array().ok()? != 2 {
        return None;
    }

    let tag = dec.unsigned().ok()?;
    let body_start = dec.position();
    dec.skip().ok()?;
    let body_bytes = dec.slice(body_start, dec.position()).ok()?;

    match tag {
        era_tag::BYRON_EBB => match ByronBlock::decode_ebb(body_bytes).ok()? {
            ByronBlock::EpochBoundary { raw_header, .. } => Some(raw_header),
            ByronBlock::MainBlock { .. } => None,
        },
        era_tag::BYRON_MAIN => match ByronBlock::decode_main(body_bytes).ok()? {
            ByronBlock::MainBlock { raw_header, .. } => Some(raw_header),
            ByronBlock::EpochBoundary { .. } => None,
        },
        era_tag::SHELLEY | era_tag::ALLEGRA | era_tag::MARY => {
            Some(ShelleyBlock::from_cbor_bytes(body_bytes).ok()?.header.to_cbor_bytes())
        }
        era_tag::ALONZO => Some(AlonzoBlock::from_cbor_bytes(body_bytes).ok()?.header.to_cbor_bytes()),
        era_tag::BABBAGE => Some(BabbageBlock::from_cbor_bytes(body_bytes).ok()?.header.to_cbor_bytes()),
        era_tag::CONWAY => Some(ConwayBlock::from_cbor_bytes(body_bytes).ok()?.header.to_cbor_bytes()),
        _ => None,
    }
}

impl<I, V, L> ChainProvider for SharedChainDb<I, V, L>
where
    I: ImmutableStore + Send + Sync,
    V: VolatileStore + Send + Sync,
    L: LedgerStore + Send + Sync,
{
    fn chain_tip(&self) -> Vec<u8> {
        let db = match self.inner.read() {
            Ok(guard) => guard,
            Err(_) => return Point::Origin.to_cbor_bytes(),
        };
        db.tip().to_cbor_bytes()
    }

    fn next_header(&self, cursor: &Option<Vec<u8>>) -> Option<(Vec<u8>, Vec<u8>, Vec<u8>)> {
        let cursor_point = match cursor {
            Some(bytes) => Point::from_cbor_bytes(bytes).ok()?,
            None => Point::Origin,
        };

        let db = self.inner.read().ok()?;
        let tip = db.tip();

        // No data beyond the cursor.
        if cursor_point == tip {
            return None;
        }

        // Get the first block after the cursor.
        let next = find_next_block(
            db.immutable(),
            db.volatile(),
            &cursor_point,
        )?;

        let next_point = block_point(&next).to_cbor_bytes();
        let header_cbor = extract_chainsync_header(next.raw_cbor.as_deref()?)?;
        let tip_cbor = tip.to_cbor_bytes();
        Some((next_point, header_cbor, tip_cbor))
    }

    fn find_intersect(&self, points: &[Vec<u8>]) -> Option<(Vec<u8>, Vec<u8>)> {
        let db = self.inner.read().ok()?;
        let tip = db.tip();

        // Walk the candidate list front-to-back; the client sends points
        // from most-recent to oldest, so the first hit is the best.
        for raw_point in points {
            if let Ok(point) = Point::from_cbor_bytes(raw_point) {
                let found = match point.hash() {
                    Some(h) => {
                        db.immutable().get_block(&h).is_some()
                            || db.volatile().get_block(&h).is_some()
                    }
                    None => true, // Origin always intersects.
                };
                if found {
                    return Some((raw_point.clone(), tip.to_cbor_bytes()));
                }
            }
        }

        None
    }
}

/// Find the first block strictly after `cursor` in the combined chain.
fn find_next_block<I: ImmutableStore, V: VolatileStore>(
    immutable: &I,
    volatile: &V,
    cursor: &Point,
) -> Option<yggdrasil_ledger::Block> {
    // If cursor is Origin, return the very first block.
    if *cursor == Point::Origin {
        let imm_suffix = immutable.suffix_after(&Point::Origin).ok()?;
        if let Some(first) = imm_suffix.into_iter().next() {
            return Some(first);
        }
        // No immutable blocks — try volatile.
        let vol_tip = volatile.tip();
        if vol_tip == Point::Origin {
            return None;
        }
        let vol_blocks = volatile.prefix_up_to(&vol_tip).ok()?;
        return vol_blocks.into_iter().next();
    }

    // Try immutable suffix after cursor (first element is next).
    if let Ok(suffix) = immutable.suffix_after(cursor) {
        if let Some(next) = suffix.into_iter().next() {
            return Some(next);
        }
        // cursor is the immutable tip — next is the first volatile block.
        let vol_tip = volatile.tip();
        if vol_tip != Point::Origin {
            if let Ok(vol_blocks) = volatile.prefix_up_to(&vol_tip) {
                return vol_blocks.into_iter().next();
            }
        }
        return None;
    }

    // cursor might be in volatile — find it and return the next.
    let vol_tip = volatile.tip();
    if vol_tip == Point::Origin {
        return None;
    }
    let vol_blocks = volatile.prefix_up_to(&vol_tip).ok()?;
    let cursor_hash = cursor.hash()?;
    let pos = vol_blocks.iter().position(|b| b.header.hash == cursor_hash)?;
    vol_blocks.into_iter().nth(pos + 1)
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
/// [`InboundPeerSession`] for each, and spawns protocol server tasks.
/// When `block_provider` and `chain_provider` are supplied, BlockFetch
/// and ChainSync server tasks are spawned alongside KeepAlive.  When a
/// `peer_sharing_provider` is supplied, PeerSharing server tasks are
/// spawned for connections that negotiated the protocol.
///
/// The loop runs until the `shutdown` future resolves or a fatal listener
/// error occurs.
pub async fn run_inbound_accept_loop<F: std::future::Future<Output = ()>>(
    listener: &PeerListener,
    block_provider: Option<Arc<dyn BlockProvider>>,
    chain_provider: Option<Arc<dyn ChainProvider>>,
    tx_submission_consumer: Option<Arc<dyn TxSubmissionConsumer>>,
    peer_sharing_provider: Option<Arc<dyn PeerSharingProvider>>,
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

                let bp = block_provider.clone();
                let cp = chain_provider.clone();
                let tx_consumer = tx_submission_consumer.clone();
                let ps_provider = peer_sharing_provider.clone();

                tokio::spawn(async move {
                    let ka = tokio::spawn(run_keepalive_server(session.keep_alive));

                    let bf = bp.map(|provider| {
                        tokio::spawn(async move {
                            let _ = run_blockfetch_server(session.block_fetch, &*provider).await;
                        })
                    });

                    let cs = cp.map(|provider| {
                        tokio::spawn(async move {
                            let _ = run_chainsync_server(session.chain_sync, &*provider).await;
                        })
                    });

                    let tx = tx_consumer.map(|consumer| {
                        tokio::spawn(async move {
                            let _ = run_txsubmission_server(session.tx_submission, &*consumer).await;
                        })
                    });

                    let ps = session.peer_sharing.and_then(|server| {
                        ps_provider.map(|provider| {
                            tokio::spawn(async move {
                                let _ = run_peersharing_server(server, &*provider).await;
                            })
                        })
                    });

                    // Wait for KeepAlive to finish (indicates peer disconnected
                    // or sent MsgDone). Then abort the remaining tasks.
                    let _ = ka.await;
                    if let Some(h) = bf { h.abort(); }
                    if let Some(h) = cs { h.abort(); }
                    if let Some(h) = tx { h.abort(); }
                    if let Some(h) = ps { h.abort(); }
                    session.mux.abort();
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BlockProvider, ChainProvider, PeerSharingProvider, SharedChainDb,
        SharedPeerSharingProvider, TxSubmissionConsumer,
        run_inbound_accept_loop,
    };
    use std::sync::{Arc, Mutex, RwLock};
    use std::collections::HashMap;
    use std::time::Duration;
    use yggdrasil_ledger::{
        Block, BlockHeader, BlockNo, CborDecode, CborEncode, Encoder, Era, HeaderHash, Point,
        ShelleyBlock, ShelleyHeader, ShelleyHeaderBody, ShelleyOpCert, ShelleyVrfCert, SlotNo,
    };
    use yggdrasil_network::{HandshakeVersion, PeerListener, TxIdAndSize};
    use yggdrasil_storage::{
        ChainDb, ImmutableStore, InMemoryImmutable, InMemoryLedgerStore, InMemoryVolatile,
        VolatileStore,
    };
    use crate::NodeConfig;
    use crate::runtime::bootstrap;

    const SHELLEY_ERA_TAG: u64 = 2;

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

    fn make_shelley_block(slot: u64, block_number: u64, prev_hash: Option<[u8; 32]>) -> (Block, ShelleyHeader) {
        let header = ShelleyHeader {
            body: ShelleyHeaderBody {
                block_number,
                slot,
                prev_hash,
                issuer_vkey: [0x11; 32],
                vrf_vkey: [0x22; 32],
                nonce_vrf: sample_vrf_cert(0x30),
                leader_vrf: sample_vrf_cert(0x40),
                block_body_size: 0,
                block_body_hash: [0x55; 32],
                operational_cert: sample_opcert(0x60),
                protocol_version: (2, 0),
            },
            signature: vec![0xDD; 448],
        };

        let shelley_block = ShelleyBlock {
            header: header.clone(),
            transaction_bodies: Vec::new(),
            transaction_witness_sets: Vec::new(),
            transaction_metadata_set: HashMap::new(),
        };

        let body_bytes = shelley_block.to_cbor_bytes();
        let mut enc = Encoder::new();
        enc.array(2);
        enc.unsigned(SHELLEY_ERA_TAG);
        enc.raw(&body_bytes);
        let raw_cbor = enc.into_bytes();

        let header_hash = header.header_hash();
        (
            Block {
                era: Era::Shelley,
                header: BlockHeader {
                    hash: header_hash,
                    prev_hash: HeaderHash(prev_hash.unwrap_or([0; 32])),
                    slot_no: SlotNo(slot),
                    block_no: BlockNo(block_number),
                    issuer_vkey: header.body.issuer_vkey,
                },
                transactions: Vec::new(),
                raw_cbor: Some(raw_cbor),
            },
            header,
        )
    }

    #[test]
    fn block_provider_uses_exclusive_lower_bound_from_origin() {
        let (block, _) = make_shelley_block(10, 1, Some([0xAA; 32]));
        let expected_raw = block.raw_cbor.clone().expect("raw block");
        let upper = Point::BlockPoint(block.header.slot_no, block.header.hash).to_cbor_bytes();

        let mut immutable = InMemoryImmutable::default();
        immutable.append_block(block).expect("append immutable block");
        let db = ChainDb::new(
            immutable,
            InMemoryVolatile::default(),
            InMemoryLedgerStore::default(),
        );
        let provider = SharedChainDb::new(db);

        let blocks = provider.get_block_range(&Point::Origin.to_cbor_bytes(), &upper);
        assert_eq!(blocks, vec![expected_raw]);
    }

    #[test]
    fn block_provider_skips_lower_bound_block() {
        let (first_block, first_header) = make_shelley_block(10, 1, Some([0xAA; 32]));
        let first_point = Point::BlockPoint(first_block.header.slot_no, first_block.header.hash);
        let (second_block, _) = make_shelley_block(20, 2, Some(first_header.header_hash().0));
        let expected_raw = second_block.raw_cbor.clone().expect("raw block");
        let upper = Point::BlockPoint(second_block.header.slot_no, second_block.header.hash).to_cbor_bytes();

        let mut immutable = InMemoryImmutable::default();
        immutable
            .append_block(first_block)
            .expect("append immutable block");
        let mut volatile = InMemoryVolatile::default();
        volatile.add_block(second_block).expect("append volatile block");
        let db = ChainDb::new(immutable, volatile, InMemoryLedgerStore::default());
        let provider = SharedChainDb::new(db);

        let blocks = provider.get_block_range(&first_point.to_cbor_bytes(), &upper);
        assert_eq!(blocks, vec![expected_raw]);
    }

    #[test]
    fn chain_provider_returns_header_bytes_and_advances_by_point() {
        let (first_block, first_header) = make_shelley_block(10, 1, Some([0xAA; 32]));
        let first_point = Point::BlockPoint(first_block.header.slot_no, first_block.header.hash);
        let (second_block, second_header) =
            make_shelley_block(20, 2, Some(first_header.header_hash().0));
        let second_point = Point::BlockPoint(second_block.header.slot_no, second_block.header.hash);

        let mut immutable = InMemoryImmutable::default();
        immutable
            .append_block(first_block)
            .expect("append immutable block");
        let mut volatile = InMemoryVolatile::default();
        volatile.add_block(second_block).expect("append volatile block");
        let db = ChainDb::new(immutable, volatile, InMemoryLedgerStore::default());
        let provider = SharedChainDb::new(db);

        let (cursor_point, first_raw_header, first_tip) = provider
            .next_header(&None)
            .expect("first chainsync response");
        assert_eq!(Point::from_cbor_bytes(&cursor_point).expect("first point"), first_point);
        assert_eq!(ShelleyHeader::from_cbor_bytes(&first_raw_header).expect("first header"), first_header);
        assert_eq!(first_tip, second_point.to_cbor_bytes());

        let (next_point, second_raw_header, second_tip) = provider
            .next_header(&Some(cursor_point))
            .expect("second chainsync response");
        assert_eq!(Point::from_cbor_bytes(&next_point).expect("second point"), second_point);
        assert_eq!(ShelleyHeader::from_cbor_bytes(&second_raw_header).expect("second header"), second_header);
        assert_eq!(second_tip, second_point.to_cbor_bytes());

        assert!(provider.next_header(&Some(second_point.to_cbor_bytes())).is_none());
        assert_eq!(provider.find_intersect(&[second_point.to_cbor_bytes()]), Some((second_point.to_cbor_bytes(), second_point.to_cbor_bytes())));
        assert_eq!(provider.chain_tip(), second_point.to_cbor_bytes());
    }

    #[derive(Default)]
    struct RecordingTxSubmissionConsumer {
        received: Mutex<Vec<Vec<u8>>>,
    }

    impl TxSubmissionConsumer for RecordingTxSubmissionConsumer {
        fn consume_txs(&self, txs: Vec<Vec<u8>>) -> usize {
            let accepted = txs.len();
            self.received.lock().expect("poisoned").extend(txs);
            accepted
        }
    }

    #[tokio::test]
    async fn inbound_accept_loop_runs_txsubmission_server() {
        let listener = PeerListener::bind(
            "127.0.0.1:0",
            42,
            vec![HandshakeVersion(15)],
        )
        .await
        .expect("bind listener");
        let listen_addr = listener.local_addr().expect("listen addr");
        let consumer = Arc::new(RecordingTxSubmissionConsumer::default());

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let accept_task = tokio::spawn({
            let consumer = Arc::clone(&consumer);
            async move {
                run_inbound_accept_loop(
                    &listener,
                    None,
                    None,
                    Some(consumer),
                    None,
                    async move {
                        let _ = shutdown_rx.await;
                    },
                )
                .await
            }
        });

        let mut session = bootstrap(&NodeConfig {
            peer_addr: listen_addr,
            network_magic: 42,
            protocol_versions: vec![HandshakeVersion(15)],
        })
        .await
        .expect("bootstrap client");

        session.tx_submission.init().await.expect("init txsubmission");

        let first_request = session
            .tx_submission
            .recv_request()
            .await
            .expect("recv tx ids request");
        let (ack, req) = match first_request {
            yggdrasil_network::TxServerRequest::RequestTxIds { blocking, ack, req } => {
                assert!(blocking);
                (ack, req)
            }
            other => panic!("expected tx ids request, got {other:?}"),
        };
        assert_eq!(ack, 0);
        assert_eq!(req, 16);

        let txid = yggdrasil_ledger::TxId([7; 32]);
        session
            .tx_submission
            .reply_tx_ids(vec![TxIdAndSize { txid, size: 3 }])
            .await
            .expect("reply tx ids");

        let second_request = session
            .tx_submission
            .recv_request()
            .await
            .expect("recv tx bodies request");
        match second_request {
            yggdrasil_network::TxServerRequest::RequestTxs { txids } => {
                assert_eq!(txids, vec![txid]);
            }
            other => panic!("expected tx request, got {other:?}"),
        }

        session
            .tx_submission
            .reply_txs(vec![vec![1, 2, 3]])
            .await
            .expect("reply tx bodies");

        let third_request = session
            .tx_submission
            .recv_request()
            .await
            .expect("recv follow-up tx ids request");
        match third_request {
            yggdrasil_network::TxServerRequest::RequestTxIds { blocking, ack, req } => {
                assert!(blocking);
                assert_eq!(ack, 1);
                assert_eq!(req, 16);
            }
            other => panic!("expected follow-up tx ids request, got {other:?}"),
        }

        session
            .tx_submission
            .send_done()
            .await
            .expect("send done");

        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(
            consumer.received.lock().expect("poisoned").clone(),
            vec![vec![1, 2, 3]]
        );

        let _ = shutdown_tx.send(());
        accept_task.await.expect("accept task join").expect("accept loop");
    }

    #[test]
    fn shared_peer_sharing_provider_returns_warm_and_hot_peers() {
        use yggdrasil_network::{PeerRegistry, PeerSource, PeerStatus};
        use std::net::SocketAddr;

        let mut registry = PeerRegistry::default();
        let warm: SocketAddr = "1.2.3.4:3001".parse().unwrap();
        let hot: SocketAddr = "5.6.7.8:3001".parse().unwrap();
        let cold: SocketAddr = "9.10.11.12:3001".parse().unwrap();

        registry.insert_source(warm, PeerSource::PeerSourceBootstrap);
        registry.insert_source(hot, PeerSource::PeerSourceBootstrap);
        registry.insert_source(cold, PeerSource::PeerSourceBootstrap);

        registry.set_status(warm, PeerStatus::PeerWarm);
        registry.set_status(hot, PeerStatus::PeerHot);
        // cold stays PeerCold by default

        let provider = SharedPeerSharingProvider::new(Arc::new(RwLock::new(registry)));
        let peers = provider.shareable_peers(10);

        let addrs: Vec<SocketAddr> = peers.iter().map(|p| p.addr).collect();
        assert!(addrs.contains(&warm), "warm peer should be shareable");
        assert!(addrs.contains(&hot), "hot peer should be shareable");
        assert!(!addrs.contains(&cold), "cold peer should not be shareable");
    }

    #[test]
    fn shared_peer_sharing_provider_respects_amount_limit() {
        use yggdrasil_network::{PeerRegistry, PeerSource, PeerStatus};
        use std::net::SocketAddr;

        let mut registry = PeerRegistry::default();
        for i in 1..=5u8 {
            let addr: SocketAddr = format!("10.0.0.{i}:3001").parse().unwrap();
            registry.insert_source(addr, PeerSource::PeerSourceBootstrap);
            registry.set_status(addr, PeerStatus::PeerWarm);
        }

        let provider = SharedPeerSharingProvider::new(Arc::new(RwLock::new(registry)));
        let peers = provider.shareable_peers(2);
        assert_eq!(peers.len(), 2, "should return at most the requested amount");
    }
}
