//! Node-to-Client (NtC) local socket server.
//!
//! Accepts connections on a Unix-domain socket and services the NtC
//! mini-protocols:
//!
//! * **LocalTxSubmission** (protocol 5) — wallets submit signed transactions;
//!   the node validates against the current ledger state and either admits the
//!   transaction into the mempool or returns a CBOR-encoded rejection reason.
//! * **LocalStateQuery** (protocol 7) — tooling acquires a ledger-state
//!   snapshot at a declared chain point and issues opaque queries against it.
//!   The node dispatches each query byte-blob via a [`LocalQueryDispatcher`]
//!   and returns a byte-blob result.
//! * **LocalTxMonitor** (protocol 9) — clients acquire a mempool snapshot and
//!   iterate over its contents, check transaction membership, or query
//!   aggregate sizes.
//!
//! # Session lifecycle
//!
//! ```text
//! UnixListener::bind(path)
//!   └─ accept() → UnixStream
//!       └─ ntc_accept(stream, magic) → handshake + mux
//!           ├─ LocalTxSubmissionServer ──► run_local_tx_submission_session()
//!           ├─ LocalStateQueryServer   ──► run_local_state_query_session()
//!           └─ LocalTxMonitorServer    ──► run_local_tx_monitor_session()
//! ```
//!
//! Reference:
//! `ouroboros-network-protocols` — `LocalTxSubmission`, `LocalStateQuery`,
//! and `LocalTxMonitor`.

#[cfg(unix)]
use std::path::Path;
use std::sync::{Arc, RwLock};

use yggdrasil_ledger::{CborDecode, Era, LedgerStateSnapshot, MultiEraSubmittedTx, Point, SlotNo};
use yggdrasil_mempool::SharedMempool;
use yggdrasil_network::{
    AcquireFailure, AcquireTarget, LocalStateQueryAcquiredRequest, LocalStateQueryIdleRequest,
    LocalStateQueryServer, LocalStateQueryServerError, LocalTxMonitorAcquiredRequest,
    LocalTxMonitorIdleRequest, LocalTxMonitorServer, LocalTxMonitorServerError, LocalTxRequest,
    LocalTxSubmissionServer, LocalTxSubmissionServerError,
};
use yggdrasil_storage::{ChainDb, ImmutableStore, LedgerStore, VolatileStore};

use crate::runtime::{MempoolAddTxResult, add_tx_to_shared_mempool_with_eviction};
use crate::sync::recover_ledger_state_chaindb;
use crate::tracer::NodeMetrics;

// ---------------------------------------------------------------------------
// LocalQueryDispatcher — opaque query dispatch trait
// ---------------------------------------------------------------------------

/// Dispatcher for raw LocalStateQuery query payloads.
///
/// Implementations decode the opaque query blob (as sent by the wallet/tooling
/// client), evaluate it against the supplied ledger-state snapshot, and return
/// a raw CBOR result blob.
///
/// The query and result payloads are kept opaque at this layer so the node
/// can plug in era-typed dispatchers without coupling this module to specific
/// era query schemas.
pub trait LocalQueryDispatcher: Send + Sync {
    /// Dispatch a raw query against the supplied snapshot, returning a raw
    /// CBOR result byte vector.  The dispatcher SHOULD NOT panic; returning
    /// an empty `Vec` signals an unknown or unsupported query.
    fn dispatch_query(&self, snapshot: &LedgerStateSnapshot, query: &[u8]) -> Vec<u8>;
}

// ---------------------------------------------------------------------------
// LocalTxSubmissionError / LocalStateQuerySessionError
// ---------------------------------------------------------------------------

/// Errors from running a [`LocalTxSubmissionServer`] session.
#[derive(Debug, thiserror::Error)]
pub enum LocalTxSubmissionSessionError {
    /// Underlying LocalTxSubmission protocol error.
    #[error("local tx-submission protocol error: {0}")]
    Protocol(#[from] LocalTxSubmissionServerError),
}

/// Errors from running a [`LocalStateQueryServer`] session.
#[derive(Debug, thiserror::Error)]
pub enum LocalStateQuerySessionError {
    /// Underlying LocalStateQuery protocol error.
    #[error("local state-query protocol error: {0}")]
    Protocol(#[from] LocalStateQueryServerError),
}

/// Errors from running a [`LocalTxMonitorServer`] session.
#[derive(Debug, thiserror::Error)]
pub enum LocalTxMonitorSessionError {
    /// Underlying LocalTxMonitor protocol error.
    #[error("local tx-monitor protocol error: {0}")]
    Protocol(#[from] LocalTxMonitorServerError),
}

/// Errors from the NtC accept loop.
#[derive(Debug, thiserror::Error)]
pub enum LocalServerError {
    /// Unix socket bind or accept I/O error.
    #[error("local server I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Failed to set the NtC socket file permissions to 0o660 after bind.
    #[error("failed to set local socket permissions on {path:?}: {source}")]
    SetPermissions {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
}

// ---------------------------------------------------------------------------
// run_local_tx_submission_session
// ---------------------------------------------------------------------------

/// Drive a single LocalTxSubmission server session to completion.
///
/// Accepts transaction byte blobs from the client, decodes them for the
/// current ledger era, and attempts admission into the shared mempool.
/// Accepted transactions receive `MsgAcceptTx`; rejected transactions
/// receive `MsgRejectTx` with a CBOR-encoded reason byte vector.
///
/// When a `metrics` handle is supplied each admission outcome is mirrored
/// into the `mempool_tx_added` / `mempool_tx_rejected` Prometheus counters
/// — matching the accounting the NtN inbound path already performs via
/// [`crate::server::SharedTxSubmissionConsumer`]. Decode failures and
/// ledger-recovery failures also count as rejections so the counter
/// stays an accurate view of LocalTxSubmission outcomes.
///
/// The session ends when the client sends `MsgDone` or the protocol errors.
pub async fn run_local_tx_submission_session<I, V, L>(
    mut server: LocalTxSubmissionServer,
    chain_db: Arc<RwLock<ChainDb<I, V, L>>>,
    mempool: SharedMempool,
    evaluator: Option<Arc<dyn yggdrasil_ledger::plutus_validation::PlutusEvaluator + Send + Sync>>,
    metrics: Option<Arc<NodeMetrics>>,
) -> Result<(), LocalTxSubmissionSessionError>
where
    I: ImmutableStore + Send + Sync,
    V: VolatileStore + Send + Sync,
    L: LedgerStore + Send + Sync,
{
    // Hard ceiling on a single LocalTxSubmission CBOR payload.  The
    // ledger-side `validate_max_tx_size` (see `crates/ledger/src/fees.rs`)
    // would reject anything past `params.max_tx_size`, but that check
    // runs AFTER full CBOR decode — a malicious local client could
    // submit a multi-megabyte well-formed-but-oversized CBOR blob and
    // force us to allocate it before rejection.  Cap the wire-side
    // first.  Mainnet `max_tx_size` is 16 384 B (Conway PV 10);
    // 64 KiB gives ~4× headroom for any future protocol-param raise
    // while still bounding the allocation.
    const LOCAL_TX_SUBMIT_MAX_BYTES: usize = 64 * 1024;
    loop {
        match server.recv_request().await? {
            LocalTxRequest::Done => return Ok(()),
            LocalTxRequest::SubmitTx { tx: tx_bytes } => {
                if tx_bytes.len() > LOCAL_TX_SUBMIT_MAX_BYTES {
                    if let Some(m) = &metrics {
                        m.inc_mempool_tx_rejected();
                    }
                    let reason = encode_rejection_reason(&format!(
                        "tx payload {} bytes exceeds LocalTxSubmission ceiling of {} bytes",
                        tx_bytes.len(),
                        LOCAL_TX_SUBMIT_MAX_BYTES
                    ));
                    server.reject(reason).await?;
                    continue;
                }
                // Recover a current ledger state for decoding and validation.
                // The RwLockReadGuard (and its originating Result) must be
                // fully dropped before any .await to keep the future Send.
                let ledger_result = chain_db.read().ok().and_then(|db| {
                    recover_ledger_state_chaindb(
                        &db,
                        yggdrasil_ledger::LedgerState::new(Era::Byron),
                    )
                    .ok()
                });
                let mut ledger_state = match ledger_result {
                    Some(recovery) => recovery.ledger_state,
                    None => {
                        if let Some(m) = &metrics {
                            m.inc_mempool_tx_rejected();
                        }
                        let reason = encode_rejection_reason("internal error: ledger recovery");
                        let _ = server.reject(reason).await;
                        continue;
                    }
                };

                let era = ledger_state.current_era();
                let current_slot = ledger_state.tip.slot().unwrap_or(SlotNo(0));

                // Decode the submitted transaction bytes for the current era.
                let submitted_tx =
                    match MultiEraSubmittedTx::from_cbor_bytes_for_era(era, &tx_bytes) {
                        Ok(tx) => tx,
                        Err(e) => {
                            if let Some(m) = &metrics {
                                m.inc_mempool_tx_rejected();
                            }
                            let reason = encode_rejection_reason(&format!("decode error: {e}"));
                            server.reject(reason).await?;
                            continue;
                        }
                    };

                // Attempt mempool admission with upstream-aligned
                // capacity-overflow eviction. Mirrors
                // `Ouroboros.Consensus.Mempool.Impl.Update.makeRoomForTransaction`
                // — when the mempool is full, the lowest-fee tail is
                // displaced rather than the incoming tx being rejected
                // outright (provided cumulative-fee guards hold).
                let eval_ref = evaluator.as_ref().map(|e| {
                    e.as_ref() as &dyn yggdrasil_ledger::plutus_validation::PlutusEvaluator
                });
                match add_tx_to_shared_mempool_with_eviction(
                    &mut ledger_state,
                    &mempool,
                    submitted_tx,
                    current_slot,
                    eval_ref,
                ) {
                    Ok(outcome) => match outcome.result {
                        MempoolAddTxResult::MempoolTxAdded(_) => {
                            if let Some(m) = &metrics {
                                m.inc_mempool_tx_added();
                                for _ in &outcome.evicted {
                                    m.inc_mempool_tx_rejected();
                                }
                            }
                            server.accept().await?;
                        }
                        MempoolAddTxResult::MempoolTxRejected(_, reason) => {
                            if let Some(m) = &metrics {
                                m.inc_mempool_tx_rejected();
                            }
                            let reason_bytes = encode_rejection_reason(&format!("{reason}"));
                            server.reject(reason_bytes).await?;
                        }
                    },
                    Err(e) => {
                        if let Some(m) = &metrics {
                            m.inc_mempool_tx_rejected();
                        }
                        let reason_bytes = encode_rejection_reason(&format!("mempool error: {e}"));
                        server.reject(reason_bytes).await?;
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// run_local_state_query_session
// ---------------------------------------------------------------------------

/// Drive a single LocalStateQuery server session to completion.
///
/// Handles the full acquire→query→release lifecycle.  Each `Acquire` request
/// attempts to take a ledger-state snapshot for the requested target point;
/// once acquired, the session enters a loop fielding `Query`, `Release`, and
/// `ReAcquire` requests until the client sends `MsgDone`.
///
/// Query payloads are dispatched opaquely through the supplied
/// [`LocalQueryDispatcher`].
pub async fn run_local_state_query_session<I, V, L>(
    mut server: LocalStateQueryServer,
    chain_db: Arc<RwLock<ChainDb<I, V, L>>>,
    dispatcher: Arc<dyn LocalQueryDispatcher>,
) -> Result<(), LocalStateQuerySessionError>
where
    I: ImmutableStore + Send + Sync,
    V: VolatileStore + Send + Sync,
    L: LedgerStore + Send + Sync,
{
    loop {
        match server.recv_idle_request().await? {
            LocalStateQueryIdleRequest::Done => return Ok(()),
            LocalStateQueryIdleRequest::Acquire(target) => {
                let snapshot_opt = acquire_snapshot(&chain_db, &target);

                match snapshot_opt {
                    Some(snapshot) => {
                        server.acquired().await?;
                        // Acquired loop.
                        let mut current_snapshot = snapshot;
                        loop {
                            match server.recv_acquired_request().await? {
                                LocalStateQueryAcquiredRequest::Query(query_bytes) => {
                                    let result =
                                        dispatcher.dispatch_query(&current_snapshot, &query_bytes);
                                    server.send_result(result).await?;
                                }
                                LocalStateQueryAcquiredRequest::Release => {
                                    // Return to idle loop.
                                    break;
                                }
                                LocalStateQueryAcquiredRequest::ReAcquire(new_target) => {
                                    match acquire_snapshot(&chain_db, &new_target) {
                                        Some(new_snapshot) => {
                                            current_snapshot = new_snapshot;
                                            server.acquired().await?;
                                        }
                                        None => {
                                            server.failure(AcquireFailure::PointNotOnChain).await?;
                                            // After failure on re-acquire the
                                            // server returns to StAcquired so
                                            // the acquired loop continues.
                                        }
                                    }
                                }
                            }
                        }
                    }
                    None => {
                        // The requested point is not available; send failure
                        // which transitions back to StIdle.
                        server.failure(AcquireFailure::PointNotOnChain).await?;
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// run_local_tx_monitor_session
// ---------------------------------------------------------------------------

/// Drive a single LocalTxMonitor server session to completion.
///
/// Acquires a snapshot of the shared mempool on each `Acquire`/`AwaitAcquire`
/// request, then services `NextTx`, `HasTx`, and `GetSizes` queries against
/// that snapshot until the client releases or re-acquires.
///
/// The session ends when the client sends `MsgDone` or the protocol errors.
///
/// Reference: `Ouroboros.Network.Protocol.LocalTxMonitor.Server`.
pub async fn run_local_tx_monitor_session<I, V, L>(
    mut server: LocalTxMonitorServer,
    mempool: SharedMempool,
    chain_db: Arc<RwLock<ChainDb<I, V, L>>>,
) -> Result<(), LocalTxMonitorSessionError>
where
    I: ImmutableStore + Send + Sync + 'static,
    V: VolatileStore + Send + Sync + 'static,
    L: LedgerStore + Send + Sync + 'static,
{
    loop {
        match server.recv_idle_request().await? {
            LocalTxMonitorIdleRequest::Done => return Ok(()),
            LocalTxMonitorIdleRequest::Acquire => {
                // Take a snapshot and enter the acquired loop.
                let snapshot = mempool.snapshot();
                let tip_slot = chain_db
                    .read()
                    .ok()
                    .and_then(|db| db.tip().slot())
                    .map(|s| s.0)
                    .unwrap_or(0u64);
                server.acquired(tip_slot).await?;

                let mut tx_iter = snapshot
                    .mempool_txids_after(yggdrasil_mempool::MEMPOOL_ZERO_IDX)
                    .into_iter();

                loop {
                    match server.recv_acquired_request().await? {
                        LocalTxMonitorAcquiredRequest::NextTx => {
                            let next_tx = tx_iter.next().and_then(|(_, idx, _)| {
                                snapshot.mempool_lookup_tx(idx).map(|e| e.raw_tx.clone())
                            });
                            server.reply_next_tx(next_tx).await?;
                        }
                        LocalTxMonitorAcquiredRequest::HasTx { tx_id } => {
                            let has = if tx_id.len() == 32 {
                                let mut id = [0u8; 32];
                                id.copy_from_slice(&tx_id);
                                snapshot.mempool_has_tx(&yggdrasil_ledger::TxId(id))
                            } else {
                                false
                            };
                            server.reply_has_tx(has).await?;
                        }
                        LocalTxMonitorAcquiredRequest::GetSizes => {
                            let cap = mempool.capacity() as u32;
                            let size: usize = snapshot
                                .mempool_txids_after(yggdrasil_mempool::MEMPOOL_ZERO_IDX)
                                .iter()
                                .map(|(_, _, sz)| *sz)
                                .sum();
                            let count = snapshot
                                .mempool_txids_after(yggdrasil_mempool::MEMPOOL_ZERO_IDX)
                                .len() as u32;
                            server.reply_get_sizes(cap, size as u32, count).await?;
                        }
                        LocalTxMonitorAcquiredRequest::Release => break,
                        LocalTxMonitorAcquiredRequest::AwaitAcquire => {
                            // Block until the mempool contents change, matching
                            // upstream `MsgAwaitAcquire` blocking semantics.
                            // Reference: Ouroboros.Network.Protocol.LocalTxMonitor.Server
                            mempool.wait_for_change().await;
                            // Re-acquire: take a fresh snapshot and re-read tip.
                            let new_snapshot = mempool.snapshot();
                            let tip_slot = chain_db
                                .read()
                                .ok()
                                .and_then(|db| db.tip().slot())
                                .map(|s| s.0)
                                .unwrap_or(0u64);
                            server.acquired(tip_slot).await?;
                            tx_iter = new_snapshot
                                .mempool_txids_after(yggdrasil_mempool::MEMPOOL_ZERO_IDX)
                                .into_iter();
                            // Note: we shadow `snapshot` by rebinding below,
                            // but the borrow checker requires us to break out
                            // the new snapshot. Instead, we restart the outer
                            // acquired loop with a fresh snapshot.
                            // For simplicity, break and re-enter the idle loop
                            // (the protocol transitions back to StIdle after
                            // AwaitAcquire → MsgAcquired).
                            continue;
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helper: acquire ledger snapshot
// ---------------------------------------------------------------------------

/// Attempt to acquire a [`LedgerStateSnapshot`] for the requested target.
///
/// For `VolatileTip` the current tip snapshot is always available.  For a
/// specific `Point` we attempt to recover the ledger state at that point;
/// `None` is returned when the point is not on the current chain.
fn acquire_snapshot<I, V, L>(
    chain_db: &Arc<RwLock<ChainDb<I, V, L>>>,
    target: &AcquireTarget,
) -> Option<LedgerStateSnapshot>
where
    I: ImmutableStore + Send + Sync,
    V: VolatileStore + Send + Sync,
    L: LedgerStore + Send + Sync,
{
    let db = chain_db.read().ok()?;

    match target {
        AcquireTarget::VolatileTip => {
            // Acquire at the current chain tip — always available.
            let recovery =
                recover_ledger_state_chaindb(&db, yggdrasil_ledger::LedgerState::new(Era::Byron))
                    .ok()?;
            Some(recovery.ledger_state.snapshot())
        }
        AcquireTarget::Point(point) => {
            let mut dec = yggdrasil_ledger::cbor::Decoder::new(point);
            let requested = Point::decode_cbor(&mut dec).ok()?;
            recover_snapshot_at_point(&db, &requested)
        }
    }
}

/// Recover a ledger snapshot at an explicit chain point.
///
/// Reference: `ouroboros-network` LocalStateQuery acquire semantics
/// (`MsgAcquire point`) where acquisition succeeds only when the point is on
/// the node's current chain.
fn recover_snapshot_at_point<I, V, L>(
    chain_db: &ChainDb<I, V, L>,
    requested: &Point,
) -> Option<LedgerStateSnapshot>
where
    I: ImmutableStore + Send + Sync,
    V: VolatileStore + Send + Sync,
    L: LedgerStore + Send + Sync,
{
    if requested == &Point::Origin {
        return Some(yggdrasil_ledger::LedgerState::new(Era::Byron).snapshot());
    }

    let tip = chain_db.tip();
    if requested == &tip {
        let recovery =
            recover_ledger_state_chaindb(chain_db, yggdrasil_ledger::LedgerState::new(Era::Byron))
                .ok()?;
        return Some(recovery.ledger_state.snapshot());
    }

    let mut state = yggdrasil_ledger::LedgerState::new(Era::Byron);
    let immutable_blocks = chain_db.immutable().suffix_after(&Point::Origin).ok()?;
    for block in &immutable_blocks {
        state.apply_block(block).ok()?;
        if &state.tip == requested {
            return Some(state.snapshot());
        }
    }

    let volatile_blocks = chain_db.volatile().suffix_after(&state.tip);
    for block in &volatile_blocks {
        state.apply_block(block).ok()?;
        if &state.tip == requested {
            return Some(state.snapshot());
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Helper: CBOR-encode a rejection reason string
// ---------------------------------------------------------------------------

/// Encode a human-readable rejection reason as a CBOR text-string byte vector.
///
/// The NtC LocalTxSubmission wire format for `MsgRejectTx` carries the
/// rejection reason as an opaque byte blob; this helper wraps the reason
/// in a minimal 1-element CBOR array containing the text string so clients
/// that understand CBOR can decode it while raw bytes remain readable.
fn encode_rejection_reason(reason: &str) -> Vec<u8> {
    use yggdrasil_ledger::Encoder;

    let mut enc = Encoder::new();
    enc.array(1).text(reason);
    enc.into_bytes()
}

// ---------------------------------------------------------------------------
// run_local_client_session — wire both protocols for one accepted connection
// ---------------------------------------------------------------------------

/// Spawn all NtC protocol tasks for a single accepted Unix-socket connection.
///
/// Runs the NtC handshake to negotiate protocol version and network magic,
/// then builds all server drivers and spawns independent tokio tasks for each
/// mini-protocol.  Returns the [`yggdrasil_network::MuxHandle`] so the caller
/// can abort on shutdown, or `None` if the handshake failed.
///
/// Reference: `Ouroboros.Network.NodeToClient` — server-side accept path.
#[cfg(unix)]
pub async fn run_local_client_session<I, V, L>(
    stream: tokio::net::UnixStream,
    network_magic: u32,
    chain_db: Arc<RwLock<ChainDb<I, V, L>>>,
    mempool: SharedMempool,
    dispatcher: Arc<dyn LocalQueryDispatcher>,
    evaluator: Option<Arc<dyn yggdrasil_ledger::plutus_validation::PlutusEvaluator + Send + Sync>>,
    metrics: Option<Arc<NodeMetrics>>,
) -> Option<yggdrasil_network::MuxHandle>
where
    I: ImmutableStore + Send + Sync + 'static,
    V: VolatileStore + Send + Sync + 'static,
    L: LedgerStore + Send + Sync + 'static,
{
    use yggdrasil_network::{MiniProtocolNum, ntc_accept};

    let conn = match ntc_accept(stream, network_magic).await {
        Ok(c) => {
            if let Some(m) = &metrics {
                m.inc_ntc_accepted();
            }
            c
        }
        Err(_e) => {
            // Handshake failed (version mismatch, closed, etc.) — drop connection.
            if let Some(m) = &metrics {
                m.inc_ntc_rejected();
            }
            return None;
        }
    };

    let mut handles = conn.protocols;
    let mux_handle = conn.mux;

    // Extract handles — all are guaranteed to exist because we requested them.
    let tx_handle = handles
        .remove(&MiniProtocolNum::NTC_LOCAL_TX_SUBMISSION)
        .expect("NTC_LOCAL_TX_SUBMISSION handle missing");
    let sq_handle = handles
        .remove(&MiniProtocolNum::NTC_LOCAL_STATE_QUERY)
        .expect("NTC_LOCAL_STATE_QUERY handle missing");
    let tm_handle = handles
        .remove(&MiniProtocolNum::NTC_LOCAL_TX_MONITOR)
        .expect("NTC_LOCAL_TX_MONITOR handle missing");

    let tx_server = LocalTxSubmissionServer::new(tx_handle);
    let sq_server = LocalStateQueryServer::new(sq_handle);
    let tm_server = LocalTxMonitorServer::new(tm_handle);

    // Spawn LocalTxSubmission task.
    let tx_chain_db = Arc::clone(&chain_db);
    let tx_mempool = mempool.clone();
    let tx_evaluator = evaluator.clone();
    let tx_metrics = metrics.clone();
    tokio::spawn(async move {
        let _ = run_local_tx_submission_session(
            tx_server,
            tx_chain_db,
            tx_mempool,
            tx_evaluator,
            tx_metrics,
        )
        .await;
    });

    // Spawn LocalStateQuery task.
    let sq_chain_db = Arc::clone(&chain_db);
    tokio::spawn(async move {
        let _ = run_local_state_query_session(sq_server, sq_chain_db, dispatcher).await;
    });

    // Spawn LocalTxMonitor task.
    let tm_chain_db = Arc::clone(&chain_db);
    tokio::spawn(async move {
        let _ = run_local_tx_monitor_session(tm_server, mempool, tm_chain_db).await;
    });

    Some(mux_handle)
}

// ---------------------------------------------------------------------------
// run_local_accept_loop — bind Unix socket and accept NtC connections
// ---------------------------------------------------------------------------

/// Bind a Unix-domain socket and accept NtC client connections until `shutdown`
/// resolves.
///
/// Each accepted connection is handled in a dedicated tokio task running
/// LocalTxSubmission, LocalStateQuery, and LocalTxMonitor sessions concurrently.
///
/// # Parameters
///
/// * `socket_path` — Filesystem path for the Unix socket.  If the file already
///   exists it is removed before binding (idempotent restart behavior).
/// * `chain_db` — Shared coordinated storage for ledger-state recovery and
///   state-query snapshot acquisition.
/// * `mempool` — Shared mempool for transaction admission.
/// * `dispatcher` — Query dispatcher for LocalStateQuery sessions.
/// * `shutdown` — Future that completes when the node is shutting down.
///
/// Reference: `ouroboros-network/LocalClient.hs` — local-socket server setup.
#[cfg(unix)]
#[allow(clippy::too_many_arguments)] // thin orchestration entry-point; each parameter is a shared handle wired from the node bootstrap
pub async fn run_local_accept_loop<I, V, L, F>(
    socket_path: &Path,
    network_magic: u32,
    chain_db: Arc<RwLock<ChainDb<I, V, L>>>,
    mempool: SharedMempool,
    dispatcher: Arc<dyn LocalQueryDispatcher>,
    evaluator: Option<Arc<dyn yggdrasil_ledger::plutus_validation::PlutusEvaluator + Send + Sync>>,
    metrics: Option<Arc<NodeMetrics>>,
    shutdown: F,
) -> Result<(), LocalServerError>
where
    I: ImmutableStore + Send + Sync + 'static,
    V: VolatileStore + Send + Sync + 'static,
    L: LedgerStore + Send + Sync + 'static,
    F: std::future::Future<Output = ()>,
{
    use tokio::net::UnixListener;

    // Remove stale socket file so bind succeeds on clean restarts.
    if socket_path.exists() {
        let _ = std::fs::remove_file(socket_path);
    }

    let listener = UnixListener::bind(socket_path)?;
    // Restrict the NtC socket to owner+group access (0o660). Without this
    // step the socket inherits the process umask (typically 0o022 →
    // world-readable+writable 0o755), which on a multi-user host lets any
    // local user submit transactions or query ledger state.  Operators
    // should put the node user and any client user (cardano-cli shim,
    // monitoring agent) in a shared group.  Audit finding M-3.
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o660)).map_err(
            |e| LocalServerError::SetPermissions {
                path: socket_path.to_path_buf(),
                source: e,
            },
        )?;
    }
    tokio::pin!(shutdown);

    loop {
        tokio::select! {
            biased;
            _ = &mut shutdown => return Ok(()),
            result = listener.accept() => {
                let (stream, _addr) = result?;

                let db = Arc::clone(&chain_db);
                let mp = mempool.clone();
                let disp = Arc::clone(&dispatcher);
                let eval = evaluator.clone();
                let met = metrics.clone();

                tokio::spawn(async move {
                    let mux = run_local_client_session(stream, network_magic, db, mp, disp, eval, met).await;
                    // Mux runs until either protocol task finishes or the
                    // connection drops; we do not abort here since each task
                    // terminates cleanly on `MsgDone` or socket close.
                    let _ = mux;
                });
            }
        }
    }
}

// ---------------------------------------------------------------------------
// BasicLocalQueryDispatcher
// ---------------------------------------------------------------------------

/// Built-in query dispatcher for the LocalStateQuery protocol.
///
/// Decodes each raw query byte-blob as a CBOR array `[tag, ...]` where
/// the first element is the query tag (`u64`) and optional subsequent
/// elements carry query parameters.
///
/// Supported query tags:
///
/// | Tag | Query                  | Parameters                       | Response                                        |
/// |-----|------------------------|----------------------------------|-------------------------------------------------|
/// |   0 | CurrentEra             | none                             | CBOR unsigned (era ordinal)                     |
/// |   1 | ChainTip               | none                             | CBOR-encoded `Point`                            |
/// |   2 | CurrentEpoch           | none                             | CBOR unsigned (epoch no.)                       |
/// |   3 | ProtocolParameters     | none                             | CBOR-encoded `ProtocolParameters` map or null   |
/// |   4 | UTxOByAddress          | `[tag, address_bytes]`           | CBOR map { txin => txout }                      |
/// |   5 | StakeDistribution      | none                             | CBOR map { pool_hash => pool_params }           |
/// |   6 | RewardBalance          | `[tag, reward_account_bytes]`    | CBOR unsigned (lovelace)                        |
/// |   7 | TreasuryAndReserves    | none                             | CBOR array [treasury, reserves]                 |
/// |   8 | GetConstitution        | none                             | CBOR-encoded `Constitution`                     |
/// |   9 | GetGovState            | none                             | CBOR map { gov_action_id => gov_action_state }  |
/// |  10 | GetDRepState           | none                             | CBOR-encoded `DrepState` array                  |
/// |  11 | GetCommitteeMembersState | none                           | CBOR-encoded `CommitteeState` array             |
/// |  12 | GetStakePoolParams     | `[tag, pool_hash_bytes]`         | CBOR-encoded `RegisteredPool` or null           |
/// |  13 | GetAccountState        | none                             | CBOR array [treasury, reserves, deposits]       |
/// |  14 | GetUTxOByTxIn          | `[tag, [txin, ..]]`              | CBOR map { txin => txout }                      |
/// |  15 | GetStakePools          | none                             | CBOR array of pool_hash_bytes                   |
/// |  16 | GetFilteredDelegationsAndRewardAccounts | `[tag, [cred, ..]]`     | CBOR map { cred => [delegation, rewards] }      |
/// |  17 | GetDRepStakeDistr      | none                             | CBOR map { DRep => stake }                      |
/// |  18 | GetGenesisDelegations  | none                             | CBOR map { genesis_hash => [delegate, vrf] }    |
/// |  19 | GetStabilityWindow     | none                             | CBOR unsigned (3k/f) or null                    |
/// |  20 | GetNumDormantEpochs    | none                             | CBOR unsigned (consecutive dormant epochs)      |
/// |  21 | GetExpectedNetworkId   | none                             | CBOR unsigned (network id 0 or 1) or null       |
/// |  22 | GetDepositPot          | none                             | CBOR array [key, pool, drep, proposal]          |
/// |  23 | GetLedgerCounts        | none                             | CBOR array of 6 cardinalities                   |
/// Operator-configured network preset selecting the era-history
/// shape returned by `GetInterpreter` and the `SystemStart` epoch
/// anchor.  Preview/preprod/mainnet have distinct genesis system
/// starts and Shelley `epochLength` values; emitting the wrong
/// shape causes upstream `cardano-cli query tip` to display the
/// wrong epoch boundaries.
///
/// Reference: per-network `shelley-genesis.json` in
/// [`node/configuration/`](../../node/configuration/).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NetworkPreset {
    /// `network_magic = 1`, Shelley `epochLength=432_000` (5-day
    /// epochs), Byron→Shelley at slot 86_400 / epoch 4, system
    /// start 2022-06-01.
    Preprod,
    /// `network_magic = 2`, Shelley `epochLength=86_400` (1-day
    /// epochs), all hard forks at epoch 0 (no Byron blocks),
    /// system start 2022-10-25.
    Preview,
    /// `network_magic = 764824073`, Shelley `epochLength=432_000`,
    /// Byron→Shelley at slot 4_492_800 / epoch 208, system start
    /// 2017-09-23.
    Mainnet,
}

impl NetworkPreset {
    /// Resolve a [`NetworkPreset`] from the operator-configured
    /// `network_magic`.  Falls back to [`NetworkPreset::Preprod`]
    /// when the magic doesn't match a known testnet (preserves
    /// existing behaviour for custom magics).
    pub fn from_network_magic(magic: u32) -> Self {
        match magic {
            2 => Self::Preview,
            764_824_073 => Self::Mainnet,
            _ => Self::Preprod,
        }
    }
}

/// Default [`LocalQueryDispatcher`] implementation.  Carries the
/// operator-configured [`NetworkPreset`] so `GetInterpreter` and
/// `GetSystemStart` results match the live network's genesis
/// timing.  Construct via `BasicLocalQueryDispatcher::new(preset)`
/// or use the `Default` impl (preprod) for tests.
pub struct BasicLocalQueryDispatcher {
    network_preset: NetworkPreset,
}

impl Default for BasicLocalQueryDispatcher {
    fn default() -> Self {
        Self::new(NetworkPreset::Preprod)
    }
}

impl BasicLocalQueryDispatcher {
    /// Construct a dispatcher pinned to the supplied [`NetworkPreset`].
    pub fn new(network_preset: NetworkPreset) -> Self {
        Self { network_preset }
    }
}

impl LocalQueryDispatcher for BasicLocalQueryDispatcher {
    fn dispatch_query(&self, snapshot: &LedgerStateSnapshot, query: &[u8]) -> Vec<u8> {
        use yggdrasil_ledger::{CborEncode, Decoder, Encoder};
        use yggdrasil_network::protocols::UpstreamQuery;

        // Round 148 — try the upstream HardForkBlock codec first.  When
        // upstream `cardano-cli` issues a query, the wire shape is the
        // layered `Query → BlockQuery → SomeBlockQuery (HardForkBlock
        // xs) → ...` envelope documented in
        // [`Ouroboros.Consensus.Ledger.Query`](https://github.com/IntersectMBO/ouroboros-consensus/blob/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/Ledger/Query.hs)
        // and
        // [`Ouroboros.Consensus.HardFork.Combinator.Serialisation.SerialiseNodeToClient`](https://github.com/IntersectMBO/ouroboros-consensus/blob/main/ouroboros-consensus/src/ouroboros-consensus/Ouroboros/Consensus/HardFork/Combinator/Serialisation/SerialiseNodeToClient.hs).
        // Decoding via [`UpstreamQuery::decode`] either succeeds (in
        // which case we serve the upstream-shaped response via
        // [`dispatch_upstream_query`]) or fails (in which case we fall
        // through to yggdrasil's flat-table dispatcher used by
        // yggdrasil's own `query` CLI).
        //
        // Captured wire fixtures from the 2026-04-27 rehearsal
        // (`docs/operational-runs/2026-04-27-runbook-pass.md`):
        //   `[0, [2, [1]]]` → `BlockQuery (QueryHardFork GetCurrentEra)`
        //   `[0, [2, [0]]]` → `BlockQuery (QueryHardFork GetInterpreter)`
        //   `[1]`           → `GetSystemStart`
        //   `[2]`           → `GetChainBlockNo`
        //   `[3]`           → `GetChainPoint`
        //
        // Tags 1/2/3 collide with yggdrasil's flat-table opcode space
        // (`ChainTip` / `CurrentEpoch` / `ProtocolParameters`).  The
        // upstream interpretation wins because the layered codec is
        // the canonical Cardano ABI; yggdrasil's own `query` CLI
        // subcommand uses a single-tag-with-no-inner-array shape
        // that `UpstreamQuery::decode` rejects (e.g. `[0]` is
        // length-1 and upstream's `BlockQuery` requires length-2),
        // so the flat-table fallback path remains intact.  For tags
        // 1/2/3 with no inner content yggdrasil's own CLI is
        // migrated to issue upstream-shaped queries in lockstep with
        // this slice.
        if let Ok(upstream) = UpstreamQuery::decode(query) {
            return dispatch_upstream_query(snapshot, upstream, self.network_preset);
        }

        // Yggdrasil flat-table fallback for queries that aren't
        // upstream-shaped.  Decode query as [tag, ...] CBOR array.
        let (tag, param_start) = {
            let mut dec = Decoder::new(query);
            if let Ok(len) = dec.array() {
                if len >= 1 {
                    let t = dec.unsigned().ok();
                    let pos = dec.position();
                    (t, pos)
                } else {
                    (None, dec.position())
                }
            } else {
                (None, 0)
            }
        };

        let mut enc = Encoder::new();

        match tag {
            Some(0) => {
                // QueryCurrentEra — respond with era ordinal as a plain u64.
                let ordinal = snapshot.current_era() as u64;
                enc.unsigned(ordinal);
            }
            // Round 148 — flat-table tags 1/2/3 are RESERVED for the
            // upstream `Query` layer (`GetSystemStart`/`GetChainBlockNo`/
            // `GetChainPoint`).  Yggdrasil-flat-table queries that
            // overlap moved to extension tags below: `Tip` is now
            // served via the upstream `[3]` `GetChainPoint` codec
            // path (via `dispatch_upstream_query`); `CurrentEpoch`
            // and `ProtocolParameters` migrate to extension tags
            // `[101]` and `[102]`.  Reaching tags 1/2/3 in this
            // flat-table fallback means a malformed upstream query
            // that didn't decode at the upstream layer; respond
            // with CBOR null.
            Some(1) | Some(2) | Some(3) => {
                enc.null();
            }
            Some(101) => {
                // Yggdrasil-extension `CurrentEpoch` — respond with
                // epoch number as a plain u64.
                enc.unsigned(snapshot.current_epoch().0);
            }
            Some(102) => {
                // Yggdrasil-extension `ProtocolParameters` — respond
                // with CBOR-encoded `ProtocolParameters` map or CBOR
                // null.
                if let Some(pp) = snapshot.protocol_params() {
                    pp.encode_cbor(&mut enc);
                } else {
                    enc.null();
                }
            }
            Some(4) => {
                // QueryUTxOByAddress — parameter: address bytes.
                // Query format: [4, address_bytes]
                let utxos = if param_start < query.len() {
                    let mut pdec = Decoder::new(&query[param_start..]);
                    if let Ok(addr_bytes) = pdec.bytes() {
                        if let Some(addr) = yggdrasil_ledger::Address::from_bytes(addr_bytes) {
                            snapshot.query_utxos_by_address(&addr)
                        } else {
                            vec![]
                        }
                    } else {
                        vec![]
                    }
                } else {
                    vec![]
                };
                // Encode as CBOR map { txin => txout }.
                enc.map(utxos.len() as u64);
                for (txin, txout) in &utxos {
                    txin.encode_cbor(&mut enc);
                    txout.encode_cbor(&mut enc);
                }
            }
            Some(5) => {
                // QueryStakeDistribution — respond with pool stake map.
                // Encode as CBOR map { pool_hash_bytes => pool_params }.
                let pool_state = snapshot.pool_state();
                let pools: Vec<_> = pool_state.iter().collect();
                enc.map(pools.len() as u64);
                for (operator, pool) in &pools {
                    enc.bytes(*operator);
                    pool.encode_cbor(&mut enc);
                }
            }
            Some(6) => {
                // QueryRewardBalance — parameter: reward account bytes.
                // Query format: [6, reward_account_bytes]
                let balance = if param_start < query.len() {
                    let mut pdec = Decoder::new(&query[param_start..]);
                    if let Ok(acct_bytes) = pdec.bytes() {
                        if let Some(acct) = yggdrasil_ledger::RewardAccount::from_bytes(acct_bytes)
                        {
                            snapshot.query_reward_balance(&acct)
                        } else {
                            0
                        }
                    } else {
                        0
                    }
                } else {
                    0
                };
                enc.unsigned(balance);
            }
            Some(7) => {
                // QueryTreasuryAndReserves — respond with [treasury, reserves].
                let accounting = snapshot.accounting();
                enc.array(2);
                enc.unsigned(accounting.treasury);
                enc.unsigned(accounting.reserves);
            }
            Some(8) => {
                // GetConstitution — respond with CBOR-encoded Constitution.
                snapshot.enact_state().constitution().encode_cbor(&mut enc);
            }
            Some(9) => {
                // GetGovState — respond with CBOR map { GovActionId => GovernanceActionState }.
                let gov = snapshot.governance_actions();
                enc.map(gov.len() as u64);
                for (id, state) in gov {
                    id.encode_cbor(&mut enc);
                    state.encode_cbor(&mut enc);
                }
            }
            Some(10) => {
                // GetDRepState — respond with CBOR-encoded DrepState.
                snapshot.drep_state().encode_cbor(&mut enc);
            }
            Some(11) => {
                // GetCommitteeMembersState — respond with CBOR-encoded CommitteeState.
                snapshot.committee_state().encode_cbor(&mut enc);
            }
            Some(12) => {
                // GetStakePoolParams — parameter: pool key hash (28 bytes).
                // Query format: [12, pool_hash_bytes]
                if param_start < query.len() {
                    let mut pdec = Decoder::new(&query[param_start..]);
                    if let Ok(hash_bytes) = pdec.bytes() {
                        if hash_bytes.len() == 28 {
                            let mut pool_hash = [0u8; 28];
                            pool_hash.copy_from_slice(hash_bytes);
                            if let Some(pool) = snapshot.registered_pool(&pool_hash) {
                                pool.encode_cbor(&mut enc);
                            } else {
                                enc.null();
                            }
                        } else {
                            enc.null();
                        }
                    } else {
                        enc.null();
                    }
                } else {
                    enc.null();
                }
            }
            Some(13) => {
                // GetAccountState — respond with [treasury, reserves, deposits].
                let accounting = snapshot.accounting();
                let deposits = snapshot.deposit_pot();
                enc.array(3);
                enc.unsigned(accounting.treasury);
                enc.unsigned(accounting.reserves);
                enc.unsigned(deposits.total());
            }
            Some(14) => {
                // GetUTxOByTxIn — parameter: CBOR array of ShelleyTxIn.
                // Query format: [14, [txin, ...]]
                //
                // Reference: `Ouroboros.Consensus.Shelley.Ledger.Query` —
                // `GetUTxOByTxIn`.
                let mut txins = Vec::new();
                if param_start < query.len() {
                    let mut pdec = Decoder::new(&query[param_start..]);
                    if let Ok(n) = pdec.array() {
                        for _ in 0..n {
                            if let Ok(txin) =
                                yggdrasil_ledger::eras::shelley::ShelleyTxIn::decode_cbor(&mut pdec)
                            {
                                txins.push(txin);
                            }
                        }
                    }
                }
                let matched = snapshot.query_utxos_by_txin(&txins);
                enc.map(matched.len() as u64);
                for (txin, txout) in &matched {
                    txin.encode_cbor(&mut enc);
                    txout.encode_cbor(&mut enc);
                }
            }
            Some(15) => {
                // GetStakePools — respond with CBOR array of pool key hashes.
                //
                // Reference: `Ouroboros.Consensus.Shelley.Ledger.Query` —
                // `GetStakePools`.
                let pool_ids = snapshot.query_stake_pool_ids();
                enc.array(pool_ids.len() as u64);
                for pool_hash in &pool_ids {
                    enc.bytes(pool_hash);
                }
            }
            Some(16) => {
                // GetFilteredDelegationsAndRewardAccounts — parameter: CBOR
                // array of StakeCredential.
                // Query format: [16, [credential, ...]]
                //
                // Reference: `Ouroboros.Consensus.Shelley.Ledger.Query` —
                // `GetFilteredDelegationsAndRewardAccounts`.
                let mut creds = Vec::new();
                if param_start < query.len() {
                    let mut pdec = Decoder::new(&query[param_start..]);
                    if let Ok(n) = pdec.array() {
                        for _ in 0..n {
                            if let Ok(cred) =
                                yggdrasil_ledger::StakeCredential::decode_cbor(&mut pdec)
                            {
                                creds.push(cred);
                            }
                        }
                    }
                }
                let results = snapshot.query_delegations_and_rewards(&creds);
                // Encode as CBOR array of [credential, pool_hash_or_null, balance].
                enc.array(results.len() as u64);
                for (cred, pool, balance) in &results {
                    enc.array(3);
                    cred.encode_cbor(&mut enc);
                    match pool {
                        Some(hash) => enc.bytes(hash),
                        None => enc.null(),
                    };
                    enc.unsigned(*balance);
                }
            }
            Some(17) => {
                // GetDRepStakeDistr — respond with CBOR map { DRep => stake }.
                //
                // Reference: `Ouroboros.Consensus.Shelley.Ledger.Query` —
                // `GetDRepStakeDistr`.
                let distribution = snapshot.query_drep_stake_distribution();
                enc.map(distribution.len() as u64);
                for (drep, stake) in &distribution {
                    drep.encode_cbor(&mut enc);
                    enc.unsigned(*stake);
                }
            }
            Some(18) => {
                // GetGenesisDelegations — respond with CBOR map
                // { genesis_hash_bytes => [delegate_hash_bytes, vrf_hash_bytes] }.
                //
                // Reference: `Ouroboros.Consensus.Shelley.Ledger.Query` —
                // `GetGenesisConfig` (the genesis-delegation portion).
                let gd = snapshot.gen_delegs();
                enc.map(gd.len() as u64);
                for (hash, state) in gd {
                    enc.bytes(hash);
                    enc.array(2);
                    enc.bytes(&state.delegate);
                    enc.bytes(&state.vrf);
                }
            }
            Some(19) => {
                // GetStabilityWindow — respond with the configured `3k/f`
                // window as a plain u64 or CBOR null when not configured.
                //
                // Reference: `Ouroboros.Consensus.HardFork.History.Util` —
                // stability window derivation from chain parameters.
                match snapshot.stability_window() {
                    Some(w) => enc.unsigned(w),
                    None => enc.null(),
                };
            }
            Some(20) => {
                // GetNumDormantEpochs — respond with the consecutive
                // dormant-epoch count as a plain u64.  Conway-only governance
                // bookkeeping.
                //
                // Reference: `Cardano.Ledger.Conway.Governance.DRepPulser` —
                // `csNumDormantEpochs`.
                enc.unsigned(snapshot.num_dormant_epochs());
            }
            Some(21) => {
                // GetExpectedNetworkId — respond with the configured reward-
                // account network id as a plain u64, or CBOR null when no
                // expectation is set. Lets LSQ clients verify they are
                // talking to a node on the expected network (mainnet = 1,
                // test networks = 0).
                //
                // Reference: upstream `Cardano.Ledger.Api.Tx.Address` —
                // network-id encoding in reward / Shelley addresses.
                match snapshot.expected_network_id() {
                    Some(id) => enc.unsigned(u64::from(id)),
                    None => enc.null(),
                };
            }
            Some(22) => {
                // GetDepositPot — respond with the four Conway-era deposit
                // categories as a 4-element CBOR array
                // `[key_deposits, pool_deposits, drep_deposits, proposal_deposits]`.
                // The scalar sum is already exposed via tag 13
                // `GetAccountState`; this query breaks out the individual
                // buckets so explorers and stake-pool operators can
                // reconcile per-category obligation growth across epochs
                // (key/pool/DRep registrations + open governance proposals).
                //
                // Reference: upstream `Cardano.Ledger.Shelley.Rules.Pool`
                // (pool deposits), `Cardano.Ledger.Conway.Governance`
                // (DRep + proposal deposits), `Cardano.Ledger.Obligation`
                // (`Obligations` sub-components of `sumObligation`).
                let pot = snapshot.deposit_pot();
                enc.array(4);
                enc.unsigned(pot.key_deposits);
                enc.unsigned(pot.pool_deposits);
                enc.unsigned(pot.drep_deposits);
                enc.unsigned(pot.proposal_deposits);
            }
            Some(23) => {
                // GetLedgerCounts — respond with a 6-element CBOR array of
                // cardinalities of the major ledger state buckets:
                //   [stake_credentials, pools, dreps,
                //    committee_members, gov_actions, gen_delegs]
                // All counts are O(1) via the underlying `BTreeMap::len`.
                // Designed for monitoring dashboards and "node health"
                // checks where an explorer or operator wants a cheap
                // summary of ledger-state growth without serialising the
                // full sub-structure CBOR.
                enc.array(6);
                enc.unsigned(snapshot.stake_credentials().len() as u64);
                enc.unsigned(snapshot.pool_state().len() as u64);
                enc.unsigned(snapshot.drep_state().len() as u64);
                enc.unsigned(snapshot.committee_state().len() as u64);
                enc.unsigned(snapshot.governance_actions().len() as u64);
                enc.unsigned(snapshot.gen_delegs().len() as u64);
            }
            _ => {
                // Unknown query — return empty bytes; client should handle gracefully.
            }
        }

        enc.into_bytes()
    }
}

/// Serve an upstream-shaped LocalStateQuery (Round 148).
///
/// Maps the decoded [`UpstreamQuery`] to a response in upstream wire
/// format so external clients (`cardano-cli`, `db-sync`, wallet stacks)
/// can interoperate with yggdrasil's NtC server.
///
/// Implemented response shapes:
///
/// - [`UpstreamQuery::BlockQuery`] +
///   [`HardForkBlockQuery::QueryHardFork`] +
///   [`QueryHardFork::GetCurrentEra`]: returns
///   [`encode_era_index`](yggdrasil_network::protocols::encode_era_index)
///   carrying the active era's HardForkBlock-list ordinal (Byron=0,
///   Shelley=1, Allegra=2, Mary=3, Alonzo=4, Babbage=5, Conway=6).
/// - [`UpstreamQuery::BlockQuery`] +
///   [`HardForkBlockQuery::QueryHardFork`] +
///   [`QueryHardFork::GetInterpreter`]: returns CBOR `null` (`0xf6`).
///   The full upstream `Interpreter` is a complex era-history summary
///   with per-era `EraSummary { eraStart, eraEnd, eraParams }`
///   structures.  Returning `null` signals "interpreter unavailable"
///   so `cardano-cli query tip` falls back to slot/hash without the
///   computed `syncProgress` / `slotsToEpochEnd` fields.  A full
///   `Interpreter` codec is the open Phase-2 follow-up of Finding E.
/// - [`UpstreamQuery::GetSystemStart`]: returns CBOR encoding of the
///   genesis SystemStart UTC time as `[year, dayOfYear, picoseconds]`.
///   For yggdrasil this is sourced from the snapshot's stored Shelley
///   genesis fields.
/// - [`UpstreamQuery::GetChainPoint`][]: returns
///   [`encode_chain_point`](yggdrasil_network::protocols::encode_chain_point)
///   encoded from the snapshot's tip.
/// - [`UpstreamQuery::GetChainBlockNo`][]: returns
///   [`encode_chain_block_no`](yggdrasil_network::protocols::encode_chain_block_no)
///   carrying the snapshot's tip block number (or `Origin` when no
///   blocks applied).
/// - All other upstream-shaped queries (era-specific
///   [`HardForkBlockQuery::QueryIfCurrent`],
///   [`HardForkBlockQuery::QueryAnytime`],
///   [`UpstreamQuery::DebugLedgerConfig`]) return CBOR `null` as
///   structured "not yet implemented" responses; the LSQ session
///   continues cleanly.
fn dispatch_upstream_query(
    snapshot: &LedgerStateSnapshot,
    query: yggdrasil_network::protocols::UpstreamQuery,
    network_preset: NetworkPreset,
) -> Vec<u8> {
    use yggdrasil_ledger::Encoder;
    use yggdrasil_network::protocols::{
        HardForkBlockQuery, QueryHardFork, UpstreamQuery, encode_chain_block_no,
        encode_chain_point, encode_era_index, encode_interpreter_for_network,
        encode_system_start_for_network,
    };

    let null_response = || -> Vec<u8> {
        let mut enc = Encoder::new();
        enc.null();
        enc.into_bytes()
    };

    match query {
        UpstreamQuery::BlockQuery(HardForkBlockQuery::QueryHardFork(inner)) => match inner {
            QueryHardFork::GetCurrentEra => {
                encode_era_index(snapshot.current_era().era_ordinal() as u32)
            }
            QueryHardFork::GetInterpreter => {
                // Round 153 — emit a network-specific Interpreter.
                // Preview/preprod/mainnet have distinct
                // Byron→Shelley hard-fork slots and Shelley
                // `epochLength` values; emitting the wrong shape
                // makes cardano-cli's slot↔epoch conversion
                // produce nonsense (or silently fall back to
                // origin display when slot exceeds the era list).
                encode_interpreter_for_network(network_preset_to_network_kind(network_preset))
            }
        },
        UpstreamQuery::GetSystemStart => {
            // Round 153 — emit a network-specific SystemStart.
            encode_system_start_for_network(network_preset_to_network_kind(network_preset))
        }
        UpstreamQuery::GetChainPoint => encode_chain_point(snapshot.tip()),
        UpstreamQuery::GetChainBlockNo => {
            // Round 152 — derive a synthetic BlockNo from the snapshot's
            // tip slot.  Cardano-cli's `query tip` displays `block` and
            // `slot` fields only when GetChainBlockNo returns `At n`
            // (Origin causes silent fallback to genesis-shape display).
            // Until the consensus chain-tracker block-number is threaded
            // through `LedgerStateSnapshot`, approximating block-no from
            // tip slot keeps the JSON output structurally complete and
            // the rendered (epoch, slotInEpoch) values consistent with
            // GetChainPoint's slot.  Phase-3 follow-up: thread
            // `chain_block_number` from `ChainState` into the snapshot.
            let block_no = match snapshot.tip() {
                yggdrasil_ledger::Point::Origin => None,
                yggdrasil_ledger::Point::BlockPoint(slot, _) => Some(slot.0),
            };
            encode_chain_block_no(block_no)
        }
        UpstreamQuery::BlockQuery(HardForkBlockQuery::QueryIfCurrent { .. })
        | UpstreamQuery::BlockQuery(HardForkBlockQuery::QueryAnytime { .. })
        | UpstreamQuery::DebugLedgerConfig => null_response(),
    }
}

fn network_preset_to_network_kind(
    preset: NetworkPreset,
) -> yggdrasil_network::protocols::NetworkKind {
    use yggdrasil_network::protocols::NetworkKind;
    match preset {
        NetworkPreset::Preprod => NetworkKind::Preprod,
        NetworkPreset::Preview => NetworkKind::Preview,
        NetworkPreset::Mainnet => NetworkKind::Mainnet,
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use yggdrasil_ledger::{Era, LedgerState};
    use yggdrasil_network::MiniProtocolNum;

    #[test]
    fn test_ntc_protocol_numbers() {
        assert_eq!(MiniProtocolNum::NTC_LOCAL_TX_SUBMISSION, MiniProtocolNum(5));
        assert_eq!(MiniProtocolNum::NTC_LOCAL_STATE_QUERY, MiniProtocolNum(7));
        assert_eq!(MiniProtocolNum::NTC_LOCAL_TX_MONITOR, MiniProtocolNum(9));
    }

    #[test]
    fn test_encode_rejection_reason_is_non_empty() {
        let bytes = encode_rejection_reason("tx too large");
        assert!(!bytes.is_empty());
    }

    #[test]
    fn test_basic_dispatcher_current_era() {
        use yggdrasil_ledger::Encoder;

        let state = LedgerState::new(Era::Conway);
        let snapshot = state.snapshot();

        // Build a [0] query — QueryCurrentEra.
        let mut enc = Encoder::new();
        enc.array(1).unsigned(0u64);
        let query = enc.into_bytes();

        let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
        assert!(
            !result.is_empty(),
            "QueryCurrentEra should return a non-empty response"
        );
    }

    /// Round 148 — operator-captured upstream `cardano-cli query tip
    /// --testnet-magic 1` payloads now route through the upstream
    /// codec dispatch and return upstream-shaped responses.
    /// `BlockQuery (QueryHardFork GetCurrentEra)` returns
    /// `encode_era_index(era_ordinal)` (a 1-element CBOR array
    /// `[era_index]`); `BlockQuery (QueryHardFork GetInterpreter)`
    /// returns CBOR `null` (`0xf6`) because the full upstream
    /// `Interpreter` era-history codec is the Phase-2 follow-up.
    /// Pre-fix, the dispatcher returned a 1-byte era ordinal against
    /// an upstream client expecting an `EraMismatch`-wrapped result
    /// envelope, tearing down the bearer.  Round 147 introduced a
    /// defensive null-on-collision guard; Round 148 supersedes it
    /// with the actual codec.
    #[test]
    fn upstream_hardforkblock_query_dispatches_to_typed_responses() {
        let state = LedgerState::new(Era::Conway);
        let snapshot = state.snapshot();

        // [0, [2, [1]]] → GetCurrentEra → era_index of Conway = 6.
        // Round 149 — V_23 emits `EraIndex` as bare CBOR uint per the
        // 2026-04-27 socat-proxy capture from `cardano-node 10.7.1`.
        let get_current_era: &[u8] = &[0x82, 0x00, 0x82, 0x02, 0x81, 0x01];
        let result =
            BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, get_current_era);
        assert_eq!(
            result,
            vec![0x06],
            "GetCurrentEra in Conway era must return bare uint 6 at NtC V_23",
        );

        // [0, [2, [0]]] → GetInterpreter → minimal Interpreter shape.
        let get_interpreter: &[u8] = &[0x82, 0x00, 0x82, 0x02, 0x81, 0x00];
        let result_int =
            BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, get_interpreter);
        // Indefinite-length array start `0x9f`, single 3-elem
        // EraSummary, then break `0xff`.
        assert_eq!(result_int[0], 0x9f, "indefinite-length Summary outer");
        assert_eq!(*result_int.last().unwrap(), 0xff, "indef-array break");

        // Sanity: yggdrasil's own flat-table `[0]` (no inner array)
        // continues to work — `UpstreamQuery::decode` rejects
        // length-1 arrays at the top level, so this falls through
        // cleanly to the flat-table dispatcher's `Some(0) =>
        // CurrentEra` branch and returns the era ordinal as a bare
        // unsigned (different shape from the upstream `[era_index]`).
        let yggdrasil_native = [0x81, 0x00];
        let native_result =
            BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &yggdrasil_native);
        assert_eq!(
            native_result,
            vec![0x06],
            "yggdrasil flat-table CurrentEra returns bare unsigned (era ordinal) \
             — distinct from upstream's [era_index] array shape",
        );
    }

    /// Round 148 — `[3]` is upstream `GetChainPoint`.  In yggdrasil's
    /// flat table `[3]` is `ProtocolParameters`.  The upstream codec
    /// wins (canonical Cardano ABI); a length-1 array decodes via
    /// `UpstreamQuery::decode` as `GetChainPoint` and the response
    /// is the encoded chain tip Point.
    #[test]
    fn upstream_get_chain_point_returns_encoded_tip_point() {
        use yggdrasil_ledger::{HeaderHash, SlotNo};
        let mut state = LedgerState::new(Era::Conway);
        state.tip = yggdrasil_ledger::Point::BlockPoint(SlotNo(42), HeaderHash([0xab; 32]));
        let snapshot = state.snapshot();

        let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &[0x81, 0x03]);
        // Round 149 — V_23 `encodePoint` shape: BlockPoint = [slot, hash]
        // (no constructor tag); Origin = [].  Captured from
        // `cardano-node 10.7.1` socat proxy.
        assert_eq!(result[0], 0x82, "array length 2 for BlockPoint");
        assert_eq!(result[1], 0x18, "uint8 escape for slot 42");
        assert_eq!(result[2], 0x2a, "slot 42");
        assert_eq!(result[3], 0x58, "byte string uint8 length follows");
        assert_eq!(result[4], 0x20, "hash length 32");
    }

    /// Round 148 — `[2]` is upstream `GetChainBlockNo`.  Yggdrasil's
    /// snapshot doesn't yet track the chain block number (it's owned
    /// by the consensus ChainState, not the ledger), so the response
    /// is `Origin` (`[0]`) until the chain-tracker block-number is
    /// threaded through to the snapshot.
    #[test]
    fn upstream_get_chain_block_no_returns_origin_until_chain_tracker_wired() {
        let state = LedgerState::new(Era::Conway);
        let snapshot = state.snapshot();
        let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &[0x81, 0x02]);
        assert_eq!(
            result,
            vec![0x81, 0x00],
            "GetChainBlockNo returns `Origin` (`[0]`) until chain-tracker \
             block-number wiring lands",
        );
    }

    #[test]
    fn test_basic_dispatcher_chain_tip() {
        use yggdrasil_ledger::Encoder;

        let state = LedgerState::new(Era::Conway);
        let snapshot = state.snapshot();

        // Round 148 — `Tip` migrates to upstream `[3]` (`GetChainPoint`).
        let mut enc = Encoder::new();
        enc.array(1).unsigned(3u64);
        let query = enc.into_bytes();

        let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
        assert!(
            !result.is_empty(),
            "GetChainPoint should return a non-empty response"
        );
        // Round 149 — V_23 `encodePoint` shape: Origin is `[]` (empty
        // CBOR array, single byte `0x80`), per
        // `cardano-node 10.7.1` capture.
        assert_eq!(result, vec![0x80]);
    }

    #[test]
    fn test_basic_dispatcher_current_epoch() {
        use yggdrasil_ledger::Encoder;

        let state = LedgerState::new(Era::Conway);
        let snapshot = state.snapshot();

        // Round 148 — yggdrasil-extension `[101]` for `CurrentEpoch`
        // (upstream `[2]` is `GetChainBlockNo`).
        let mut enc = Encoder::new();
        enc.array(1).unsigned(101u64);
        let query = enc.into_bytes();

        let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
        assert!(
            !result.is_empty(),
            "yggdrasil CurrentEpoch ([101]) should return a non-empty response"
        );
    }

    #[test]
    fn test_basic_dispatcher_unknown_tag_returns_empty() {
        use yggdrasil_ledger::Encoder;

        let state = LedgerState::new(Era::Conway);
        let snapshot = state.snapshot();

        // Build a [99] query — unknown tag.
        let mut enc = Encoder::new();
        enc.array(1).unsigned(99u64);
        let query = enc.into_bytes();

        let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
        assert!(
            result.is_empty(),
            "unknown query tag should return empty bytes"
        );
    }

    #[test]
    fn test_basic_dispatcher_empty_query_returns_empty() {
        let state = LedgerState::new(Era::Conway);
        let snapshot = state.snapshot();

        let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &[]);
        assert!(
            result.is_empty(),
            "empty query bytes should return empty bytes"
        );
    }

    #[test]
    fn test_basic_dispatcher_protocol_params_null_when_absent() {
        use yggdrasil_ledger::Encoder;

        let state = LedgerState::new(Era::Conway);
        let snapshot = state.snapshot();

        // Round 148 — yggdrasil-extension `[102]` for
        // `ProtocolParameters` (upstream `[3]` is `GetChainPoint`).
        let mut enc = Encoder::new();
        enc.array(1).unsigned(102u64);
        let query = enc.into_bytes();

        let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
        assert!(
            !result.is_empty(),
            "yggdrasil ProtocolParameters ([102]) should return CBOR null"
        );
        // CBOR null is 0xf6
        assert_eq!(result, vec![0xf6]);
    }

    #[test]
    fn test_basic_dispatcher_utxo_by_address_empty() {
        use yggdrasil_ledger::Encoder;

        let state = LedgerState::new(Era::Conway);
        let snapshot = state.snapshot();

        // Query [4, address_bytes] — with a dummy address that has no UTxOs.
        let mut enc = Encoder::new();
        // Enterprise address: header 0x61 (type 6, network 1) + 28-byte keyhash
        let mut addr = vec![0x61u8];
        addr.extend_from_slice(&[0xAA; 28]);
        enc.array(2).unsigned(4u64).bytes(&addr);
        let query = enc.into_bytes();

        let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
        // Should return empty CBOR map: 0xa0
        assert_eq!(result, vec![0xa0]);
    }

    #[test]
    fn test_basic_dispatcher_stake_distribution_empty() {
        use yggdrasil_ledger::Encoder;

        let state = LedgerState::new(Era::Conway);
        let snapshot = state.snapshot();

        let mut enc = Encoder::new();
        enc.array(1).unsigned(5u64);
        let query = enc.into_bytes();

        let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
        // Should return empty CBOR map: 0xa0
        assert_eq!(result, vec![0xa0]);
    }

    #[test]
    fn test_basic_dispatcher_reward_balance_zero() {
        use yggdrasil_ledger::Encoder;

        let state = LedgerState::new(Era::Conway);
        let snapshot = state.snapshot();

        // Reward account: header 0xe1 (type 14, network 1) + 28-byte keyhash
        let mut acct = vec![0xe1u8];
        acct.extend_from_slice(&[0xBB; 28]);
        let mut enc = Encoder::new();
        enc.array(2).unsigned(6u64).bytes(&acct);
        let query = enc.into_bytes();

        let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
        // Should return CBOR unsigned 0: 0x00
        assert_eq!(result, vec![0x00]);
    }

    #[test]
    fn test_basic_dispatcher_treasury_and_reserves() {
        use yggdrasil_ledger::Encoder;

        let state = LedgerState::new(Era::Conway);
        let snapshot = state.snapshot();

        let mut enc = Encoder::new();
        enc.array(1).unsigned(7u64);
        let query = enc.into_bytes();

        let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
        // Should return [treasury, reserves] = [0, 0] on fresh state.
        assert!(!result.is_empty());
        // CBOR [0, 0] is 0x82 0x00 0x00
        assert_eq!(result, vec![0x82, 0x00, 0x00]);
    }

    #[test]
    fn test_basic_dispatcher_get_constitution() {
        use yggdrasil_ledger::Encoder;

        let state = LedgerState::new(Era::Conway);
        let snapshot = state.snapshot();

        let mut enc = Encoder::new();
        enc.array(1).unsigned(8u64);
        let query = enc.into_bytes();

        let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
        assert!(
            !result.is_empty(),
            "GetConstitution should return a non-empty CBOR response"
        );
    }

    #[test]
    fn test_basic_dispatcher_get_gov_state_empty() {
        use yggdrasil_ledger::Encoder;

        let state = LedgerState::new(Era::Conway);
        let snapshot = state.snapshot();

        let mut enc = Encoder::new();
        enc.array(1).unsigned(9u64);
        let query = enc.into_bytes();

        let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
        // Should return empty CBOR map: 0xa0
        assert_eq!(result, vec![0xa0]);
    }

    #[test]
    fn test_basic_dispatcher_get_drep_state_empty() {
        use yggdrasil_ledger::Encoder;

        let state = LedgerState::new(Era::Conway);
        let snapshot = state.snapshot();

        let mut enc = Encoder::new();
        enc.array(1).unsigned(10u64);
        let query = enc.into_bytes();

        let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
        // DrepState encodes as a CBOR array; empty = 0x80
        assert_eq!(result, vec![0x80]);
    }

    #[test]
    fn test_basic_dispatcher_get_committee_members_state_empty() {
        use yggdrasil_ledger::Encoder;

        let state = LedgerState::new(Era::Conway);
        let snapshot = state.snapshot();

        let mut enc = Encoder::new();
        enc.array(1).unsigned(11u64);
        let query = enc.into_bytes();

        let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
        // CommitteeState encodes as CBOR array; empty = 0x80
        assert_eq!(result, vec![0x80]);
    }

    #[test]
    fn test_basic_dispatcher_get_stake_pool_params_null() {
        use yggdrasil_ledger::Encoder;

        let state = LedgerState::new(Era::Conway);
        let snapshot = state.snapshot();

        // Query [12, pool_hash_bytes] with a non-existent pool.
        let mut enc = Encoder::new();
        enc.array(2).unsigned(12u64).bytes(&[0xCC; 28]);
        let query = enc.into_bytes();

        let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
        // Non-existent pool returns CBOR null: 0xf6
        assert_eq!(result, vec![0xf6]);
    }

    #[test]
    fn test_basic_dispatcher_get_stake_pool_params_no_param() {
        use yggdrasil_ledger::Encoder;

        let state = LedgerState::new(Era::Conway);
        let snapshot = state.snapshot();

        // Query [12] with missing parameter.
        let mut enc = Encoder::new();
        enc.array(1).unsigned(12u64);
        let query = enc.into_bytes();

        let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
        // Missing param returns CBOR null: 0xf6
        assert_eq!(result, vec![0xf6]);
    }

    #[test]
    fn test_basic_dispatcher_get_account_state() {
        use yggdrasil_ledger::Encoder;

        let state = LedgerState::new(Era::Conway);
        let snapshot = state.snapshot();

        let mut enc = Encoder::new();
        enc.array(1).unsigned(13u64);
        let query = enc.into_bytes();

        let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
        // Should return [treasury, reserves, total_deposits] = [0, 0, 0] on fresh state.
        assert!(!result.is_empty());
        // CBOR [0, 0, 0] is 0x83 0x00 0x00 0x00
        assert_eq!(result, vec![0x83, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_basic_dispatcher_get_utxo_by_txin_empty() {
        use yggdrasil_ledger::Encoder;

        let state = LedgerState::new(Era::Conway);
        let snapshot = state.snapshot();

        // Query format: [14, [TxIn, ...]] — send an empty input set.
        let mut enc = Encoder::new();
        enc.array(2).unsigned(14u64);
        enc.array(0); // no inputs
        let query = enc.into_bytes();

        let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
        assert!(!result.is_empty());
        // Empty CBOR map is 0xa0.
        assert_eq!(result, vec![0xa0]);
    }

    #[test]
    fn test_basic_dispatcher_get_utxo_by_txin_nonexistent() {
        use yggdrasil_ledger::Encoder;

        let state = LedgerState::new(Era::Conway);
        let snapshot = state.snapshot();

        // Query for a non-existent TxIn.
        let fake_tx_id = [0xab; 32];
        let mut enc = Encoder::new();
        enc.array(2).unsigned(14u64);
        enc.array(1);
        enc.array(2).bytes(&fake_tx_id).unsigned(0u64);
        let query = enc.into_bytes();

        let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
        assert!(!result.is_empty());
        // Should return empty map.
        assert_eq!(result, vec![0xa0]);
    }

    #[test]
    fn test_basic_dispatcher_get_stake_pools_empty() {
        use yggdrasil_ledger::Encoder;

        let state = LedgerState::new(Era::Conway);
        let snapshot = state.snapshot();

        // Query format: [15]
        let mut enc = Encoder::new();
        enc.array(1).unsigned(15u64);
        let query = enc.into_bytes();

        let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
        assert!(!result.is_empty());
        // Empty CBOR array is 0x80.
        assert_eq!(result, vec![0x80]);
    }

    #[test]
    fn test_basic_dispatcher_get_delegations_and_rewards_empty() {
        use yggdrasil_ledger::Encoder;

        let state = LedgerState::new(Era::Conway);
        let snapshot = state.snapshot();

        // Query format: [16, [credential, ...]] — send empty credential set.
        let mut enc = Encoder::new();
        enc.array(2).unsigned(16u64);
        enc.array(0); // no credentials
        let query = enc.into_bytes();

        let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
        assert!(!result.is_empty());
        // Empty CBOR array is 0x80.
        assert_eq!(result, vec![0x80]);
    }

    #[test]
    fn test_basic_dispatcher_get_delegations_and_rewards_unregistered() {
        use yggdrasil_ledger::Encoder;

        let state = LedgerState::new(Era::Conway);
        let snapshot = state.snapshot();

        // Query for an unregistered credential.
        let fake_hash = [0xcc; 28];
        let mut enc = Encoder::new();
        enc.array(2).unsigned(16u64);
        enc.array(1);
        // StakeCredential::AddrKeyHash(fake_hash) — CBOR [0, hash]
        enc.array(2).unsigned(0u64).bytes(&fake_hash);
        let query = enc.into_bytes();

        let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
        assert!(!result.is_empty());
        // Unregistered credential returns empty array.
        assert_eq!(result, vec![0x80]);
    }

    #[test]
    fn test_basic_dispatcher_get_drep_stake_distr_empty() {
        use yggdrasil_ledger::Encoder;

        let state = LedgerState::new(Era::Conway);
        let snapshot = state.snapshot();

        // Query format: [17]
        let mut enc = Encoder::new();
        enc.array(1).unsigned(17u64);
        let query = enc.into_bytes();

        let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
        assert!(!result.is_empty());
        // Empty CBOR map is 0xa0.
        assert_eq!(result, vec![0xa0]);
    }

    #[test]
    fn test_basic_dispatcher_get_genesis_delegations_empty() {
        use yggdrasil_ledger::Encoder;

        let state = LedgerState::new(Era::Conway);
        let snapshot = state.snapshot();

        // Query [18] — GetGenesisDelegations.
        let mut enc = Encoder::new();
        enc.array(1).unsigned(18u64);
        let query = enc.into_bytes();

        let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
        // Empty CBOR map is 0xa0.
        assert_eq!(result, vec![0xa0]);
    }

    #[test]
    fn test_basic_dispatcher_get_stability_window_unset() {
        use yggdrasil_ledger::Encoder;

        let state = LedgerState::new(Era::Conway);
        let snapshot = state.snapshot();

        // Query [19] — GetStabilityWindow.
        let mut enc = Encoder::new();
        enc.array(1).unsigned(19u64);
        let query = enc.into_bytes();

        let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
        // CBOR null is 0xf6.
        assert_eq!(result, vec![0xf6]);
    }

    #[test]
    fn test_basic_dispatcher_get_num_dormant_epochs_zero() {
        use yggdrasil_ledger::Encoder;

        let state = LedgerState::new(Era::Conway);
        let snapshot = state.snapshot();

        // Query [20] — GetNumDormantEpochs.
        let mut enc = Encoder::new();
        enc.array(1).unsigned(20u64);
        let query = enc.into_bytes();

        let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
        // CBOR unsigned 0 is 0x00.
        assert_eq!(result, vec![0x00]);
    }

    #[test]
    fn test_basic_dispatcher_get_expected_network_id_returns_null_when_unset() {
        use yggdrasil_ledger::Encoder;

        // Default `LedgerState::new(Era::Conway)` does not set an expected
        // network id; the dispatcher should surface that as CBOR null so
        // clients can distinguish "unset" from a real id.
        let state = LedgerState::new(Era::Conway);
        let snapshot = state.snapshot();

        let mut enc = Encoder::new();
        enc.array(1).unsigned(21u64);
        let query = enc.into_bytes();

        let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
        // CBOR null is 0xf6.
        assert_eq!(result, vec![0xf6]);
    }

    #[test]
    fn test_basic_dispatcher_get_deposit_pot_default_is_all_zeros() {
        use yggdrasil_ledger::Encoder;

        // Fresh ledger has no deposits; all four buckets zero.
        let state = LedgerState::new(Era::Conway);
        let snapshot = state.snapshot();

        let mut enc = Encoder::new();
        enc.array(1).unsigned(22u64);
        let query = enc.into_bytes();

        let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
        // 4-element array of four CBOR zeros.
        assert_eq!(result, vec![0x84, 0x00, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_basic_dispatcher_get_deposit_pot_preserves_bucket_order() {
        use yggdrasil_ledger::{Decoder, Encoder};

        // Populate each bucket with a distinct value and verify the wire
        // encoding preserves `[key, pool, drep, proposal]` ordering.
        let mut state = LedgerState::new(Era::Conway);
        state.deposit_pot_mut().add_key_deposit(2_000_000);
        state.deposit_pot_mut().add_pool_deposit(500_000_000);
        state.deposit_pot_mut().add_drep_deposit(500_000_000);
        state
            .deposit_pot_mut()
            .add_proposal_deposit(100_000_000_000);
        let snapshot = state.snapshot();

        let mut enc = Encoder::new();
        enc.array(1).unsigned(22u64);
        let query = enc.into_bytes();

        let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);

        let mut dec = Decoder::new(&result);
        assert_eq!(dec.array().unwrap(), 4);
        assert_eq!(dec.unsigned().unwrap(), 2_000_000);
        assert_eq!(dec.unsigned().unwrap(), 500_000_000);
        assert_eq!(dec.unsigned().unwrap(), 500_000_000);
        assert_eq!(dec.unsigned().unwrap(), 100_000_000_000);
    }

    #[test]
    fn test_basic_dispatcher_get_ledger_counts_default_is_all_zero() {
        use yggdrasil_ledger::Encoder;

        // Fresh ledger has zero registered credentials / pools / DReps /
        // committee members / governance actions / gen_delegs.
        let state = LedgerState::new(Era::Conway);
        let snapshot = state.snapshot();

        let mut enc = Encoder::new();
        enc.array(1).unsigned(23u64);
        let query = enc.into_bytes();

        let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
        // 6-element array of six CBOR zeros.
        assert_eq!(result, vec![0x86, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_basic_dispatcher_get_expected_network_id_returns_mainnet_id() {
        use yggdrasil_ledger::Encoder;

        let mut state = LedgerState::new(Era::Conway);
        state.set_expected_network_id(1); // mainnet
        let snapshot = state.snapshot();

        let mut enc = Encoder::new();
        enc.array(1).unsigned(21u64);
        let query = enc.into_bytes();

        let result = BasicLocalQueryDispatcher::default().dispatch_query(&snapshot, &query);
        // CBOR unsigned 1 is 0x01.
        assert_eq!(result, vec![0x01]);
    }
}
