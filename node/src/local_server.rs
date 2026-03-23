//! Node-to-Client (NtC) local socket server.
//!
//! Accepts connections on a Unix-domain socket and services the two NtC
//! mini-protocols:
//!
//! * **LocalTxSubmission** (protocol 5) — wallets submit signed transactions;
//!   the node validates against the current ledger state and either admits the
//!   transaction into the mempool or returns a CBOR-encoded rejection reason.
//! * **LocalStateQuery** (protocol 7) — tooling acquires a ledger-state
//!   snapshot at a declared chain point and issues opaque queries against it.
//!   The node dispatches each query byte-blob via a [`LocalQueryDispatcher`]
//!   and returns a byte-blob result.
//!
//! # Session lifecycle
//!
//! ```text
//! UnixListener::bind(path)
//!   └─ accept() → UnixStream
//!       └─ start_mux_unix([NTC_LOCAL_TX_SUBMISSION, NTC_LOCAL_STATE_QUERY])
//!           ├─ LocalTxSubmissionServer ──► run_local_tx_submission_session()
//!           └─ LocalStateQueryServer   ──► run_local_state_query_session()
//! ```
//!
//! Reference:
//! `ouroboros-network-protocols` — `LocalTxSubmission` and `LocalStateQuery`.

#[cfg(unix)]
use std::path::Path;
use std::sync::{Arc, RwLock};

use yggdrasil_ledger::{CborDecode, Era, LedgerStateSnapshot, MultiEraSubmittedTx, Point, SlotNo};
use yggdrasil_mempool::SharedMempool;
use yggdrasil_network::{
    AcquireFailure, AcquireTarget,
    LocalStateQueryAcquiredRequest, LocalStateQueryIdleRequest,
    LocalStateQueryServer, LocalStateQueryServerError,
    LocalTxRequest, LocalTxSubmissionServer, LocalTxSubmissionServerError,
    MiniProtocolDir, MiniProtocolNum,
};
use yggdrasil_storage::{ChainDb, ImmutableStore, LedgerStore, VolatileStore};

use crate::runtime::{add_tx_to_shared_mempool, MempoolAddTxResult};
use crate::sync::recover_ledger_state_chaindb;

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

/// Errors from the NtC accept loop.
#[derive(Debug, thiserror::Error)]
pub enum LocalServerError {
    /// Unix socket bind or accept I/O error.
    #[error("local server I/O error: {0}")]
    Io(#[from] std::io::Error),
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
/// The session ends when the client sends `MsgDone` or the protocol errors.
pub async fn run_local_tx_submission_session<I, V, L>(
    mut server: LocalTxSubmissionServer,
    chain_db: Arc<RwLock<ChainDb<I, V, L>>>,
    mempool: SharedMempool,
) -> Result<(), LocalTxSubmissionSessionError>
where
    I: ImmutableStore + Send + Sync,
    V: VolatileStore + Send + Sync,
    L: LedgerStore + Send + Sync,
{
    loop {
        match server.recv_request().await? {
            LocalTxRequest::Done => return Ok(()),
            LocalTxRequest::SubmitTx { tx: tx_bytes } => {
                // Recover a current ledger state for decoding and validation.
                // The RwLockReadGuard (and its originating Result) must be
                // fully dropped before any .await to keep the future Send.
                let ledger_result = chain_db
                    .read()
                    .ok()
                    .and_then(|db| {
                        recover_ledger_state_chaindb(
                            &db,
                            yggdrasil_ledger::LedgerState::new(Era::Byron),
                        )
                        .ok()
                    });
                let mut ledger_state = match ledger_result {
                    Some(recovery) => recovery.ledger_state,
                    None => {
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
                            let reason = encode_rejection_reason(&format!("decode error: {e}"));
                            server.reject(reason).await?;
                            continue;
                        }
                    };

                // Attempt mempool admission.
                match add_tx_to_shared_mempool(
                    &mut ledger_state,
                    &mempool,
                    submitted_tx,
                    current_slot,
                ) {
                    Ok(MempoolAddTxResult::MempoolTxAdded(_)) => {
                        server.accept().await?;
                    }
                    Ok(MempoolAddTxResult::MempoolTxRejected(_, reason)) => {
                        let reason_bytes = encode_rejection_reason(&format!("{reason:?}"));
                        server.reject(reason_bytes).await?;
                    }
                    Err(e) => {
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
                                    let result = dispatcher
                                        .dispatch_query(&current_snapshot, &query_bytes);
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
                                            server
                                                .failure(AcquireFailure::PointNotOnChain)
                                                .await?;
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
            let recovery = recover_ledger_state_chaindb(
                &db,
                yggdrasil_ledger::LedgerState::new(Era::Byron),
            )
            .ok()?;
            Some(recovery.ledger_state.snapshot())
        }
        AcquireTarget::Point(point) => {
            // Acquire at a specific historical point.
            // Recover the full ledger state and check that the tip matches.
            let recovery = recover_ledger_state_chaindb(
                &db,
                yggdrasil_ledger::LedgerState::new(Era::Byron),
            )
            .ok()?;
            let snapshot = recovery.ledger_state.snapshot();
            if snapshot.tip() == &Point::Origin {
                Some(snapshot)
            } else {
                // Decode the requested point and compare with snapshot tip.
                let mut dec = yggdrasil_ledger::cbor::Decoder::new(point);
                let requested = Point::decode_cbor(&mut dec).ok();
                if requested.as_ref() == Some(snapshot.tip()) {
                    Some(snapshot)
                } else {
                    // Specific historical point replay is not yet implemented.
                    // Report unavailability so the client can retry with
                    // VolatileTip or back off.
                    None
                }
            }
        }
    }
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
    use yggdrasil_ledger::{CborEncode, Encoder};

    let mut enc = Encoder::new();
    enc.array(1).text(reason);
    enc.into_bytes()
}

// ---------------------------------------------------------------------------
// run_local_client_session — wire both protocols for one accepted connection
// ---------------------------------------------------------------------------

/// Spawn both NtC protocol tasks for a single accepted Unix-socket connection.
///
/// Starts the mux over the provided `stream`, builds both server drivers, and
/// spawns independent tokio tasks for each mini-protocol.  Returns the
/// [`yggdrasil_network::MuxHandle`] so the caller can abort on shutdown.
#[cfg(unix)]
pub async fn run_local_client_session<I, V, L>(
    stream: tokio::net::UnixStream,
    chain_db: Arc<RwLock<ChainDb<I, V, L>>>,
    mempool: SharedMempool,
    dispatcher: Arc<dyn LocalQueryDispatcher>,
) -> yggdrasil_network::MuxHandle
where
    I: ImmutableStore + Send + Sync + Clone + 'static,
    V: VolatileStore + Send + Sync + Clone + 'static,
    L: LedgerStore + Send + Sync + Clone + 'static,
{
    use yggdrasil_network::{start_mux_unix, MiniProtocolNum};

    let protocols = [
        MiniProtocolNum::NTC_LOCAL_TX_SUBMISSION,
        MiniProtocolNum::NTC_LOCAL_STATE_QUERY,
    ];
    let (mut handles, mux_handle) =
        start_mux_unix(stream, MiniProtocolDir::Responder, &protocols, 32);

    // Extract handles — both are guaranteed to exist because we requested them.
    let tx_handle = handles
        .remove(&MiniProtocolNum::NTC_LOCAL_TX_SUBMISSION)
        .expect("NTC_LOCAL_TX_SUBMISSION handle missing");
    let sq_handle = handles
        .remove(&MiniProtocolNum::NTC_LOCAL_STATE_QUERY)
        .expect("NTC_LOCAL_STATE_QUERY handle missing");

    let tx_server = LocalTxSubmissionServer::new(tx_handle);
    let sq_server = LocalStateQueryServer::new(sq_handle);

    // Spawn LocalTxSubmission task.
    let tx_chain_db = Arc::clone(&chain_db);
    tokio::spawn(async move {
        let _ = run_local_tx_submission_session(tx_server, tx_chain_db, mempool).await;
    });

    // Spawn LocalStateQuery task.
    let sq_chain_db = Arc::clone(&chain_db);
    tokio::spawn(async move {
        let _ = run_local_state_query_session(sq_server, sq_chain_db, dispatcher).await;
    });

    mux_handle
}

// ---------------------------------------------------------------------------
// run_local_accept_loop — bind Unix socket and accept NtC connections
// ---------------------------------------------------------------------------

/// Bind a Unix-domain socket and accept NtC client connections until `shutdown`
/// resolves.
///
/// Each accepted connection is handled in a dedicated tokio task running both
/// LocalTxSubmission and LocalStateQuery sessions concurrently.
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
pub async fn run_local_accept_loop<I, V, L, F>(
    socket_path: &Path,
    chain_db: Arc<RwLock<ChainDb<I, V, L>>>,
    mempool: SharedMempool,
    dispatcher: Arc<dyn LocalQueryDispatcher>,
    shutdown: F,
) -> Result<(), LocalServerError>
where
    I: ImmutableStore + Send + Sync + Clone + 'static,
    V: VolatileStore + Send + Sync + Clone + 'static,
    L: LedgerStore + Send + Sync + Clone + 'static,
    F: std::future::Future<Output = ()>,
{
    use tokio::net::UnixListener;

    // Remove stale socket file so bind succeeds on clean restarts.
    if socket_path.exists() {
        let _ = std::fs::remove_file(socket_path);
    }

    let listener = UnixListener::bind(socket_path)?;
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

                tokio::spawn(async move {
                    let mux = run_local_client_session(stream, db, mp, disp).await;
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

/// Minimal built-in query dispatcher for the LocalStateQuery protocol.
///
/// Decodes each raw query byte-blob as a 1-element CBOR array where the
/// first item is the query tag (`u64`), and returns an appropriate CBOR
/// response.  Unknown query tags return an empty byte vector.
///
/// Supported query tags (provisional, matching `cardano-api` conventions):
///
/// | Tag | Query              | Response                     |
/// |-----|--------------------|------------------------------|
/// |   0 | CurrentEra         | CBOR unsigned (era ordinal)  |
/// |   1 | ChainTip           | CBOR-encoded `Point`         |
/// |   2 | CurrentEpoch       | CBOR unsigned (epoch no.)    |
///
/// This dispatcher is intentionally minimal.  Production deployments should
/// supply a richer dispatcher that handles full per-era query schemas.
pub struct BasicLocalQueryDispatcher;

impl LocalQueryDispatcher for BasicLocalQueryDispatcher {
    fn dispatch_query(&self, snapshot: &LedgerStateSnapshot, query: &[u8]) -> Vec<u8> {
        use yggdrasil_ledger::{CborDecode, CborEncode, Decoder, Encoder};

        // Decode query as [tag, ...] CBOR array.
        let tag = {
            let mut dec = Decoder::new(query);
            if let Ok(len) = dec.array() {
                if len >= 1 {
                    dec.unsigned().ok()
                } else {
                    None
                }
            } else {
                None
            }
        };

        let mut enc = Encoder::new();

        match tag {
            Some(0) => {
                // QueryCurrentEra — respond with era ordinal as a plain u64.
                let ordinal = snapshot.current_era() as u64;
                enc.unsigned(ordinal);
            }
            Some(1) => {
                // QueryChainTip — respond with CBOR-encoded Point.
                snapshot.tip().encode_cbor(&mut enc);
            }
            Some(2) => {
                // QueryCurrentEpoch — respond with epoch number as a plain u64.
                enc.unsigned(snapshot.current_epoch().0);
            }
            _ => {
                // Unknown query — return empty bytes; client should handle gracefully.
            }
        }

        enc.into_bytes()
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use yggdrasil_ledger::{Era, LedgerState, Point};
    use yggdrasil_network::MiniProtocolNum;

    #[test]
    fn test_ntc_protocol_numbers() {
        assert_eq!(MiniProtocolNum::NTC_LOCAL_TX_SUBMISSION, MiniProtocolNum(5));
        assert_eq!(MiniProtocolNum::NTC_LOCAL_STATE_QUERY, MiniProtocolNum(7));
    }

    #[test]
    fn test_encode_rejection_reason_is_non_empty() {
        let bytes = encode_rejection_reason("tx too large");
        assert!(!bytes.is_empty());
    }

    #[test]
    fn test_basic_dispatcher_current_era() {
        use yggdrasil_ledger::{CborEncode, Encoder};

        let state = LedgerState::new(Era::Conway);
        let snapshot = state.snapshot();

        // Build a [0] query — QueryCurrentEra.
        let mut enc = Encoder::new();
        enc.array(1).unsigned(0u64);
        let query = enc.into_bytes();

        let result = BasicLocalQueryDispatcher.dispatch_query(&snapshot, &query);
        assert!(!result.is_empty(), "QueryCurrentEra should return a non-empty response");
    }

    #[test]
    fn test_basic_dispatcher_chain_tip() {
        use yggdrasil_ledger::{CborEncode, Encoder};

        let state = LedgerState::new(Era::Conway);
        let snapshot = state.snapshot();

        // Build a [1] query — QueryChainTip.
        let mut enc = Encoder::new();
        enc.array(1).unsigned(1u64);
        let query = enc.into_bytes();

        let result = BasicLocalQueryDispatcher.dispatch_query(&snapshot, &query);
        assert!(!result.is_empty(), "QueryChainTip should return a non-empty response");
    }

    #[test]
    fn test_basic_dispatcher_current_epoch() {
        use yggdrasil_ledger::{CborEncode, Encoder};

        let state = LedgerState::new(Era::Conway);
        let snapshot = state.snapshot();

        // Build a [2] query — QueryCurrentEpoch.
        let mut enc = Encoder::new();
        enc.array(1).unsigned(2u64);
        let query = enc.into_bytes();

        let result = BasicLocalQueryDispatcher.dispatch_query(&snapshot, &query);
        assert!(!result.is_empty(), "QueryCurrentEpoch should return a non-empty response");
    }

    #[test]
    fn test_basic_dispatcher_unknown_tag_returns_empty() {
        use yggdrasil_ledger::{CborEncode, Encoder};

        let state = LedgerState::new(Era::Conway);
        let snapshot = state.snapshot();

        // Build a [99] query — unknown tag.
        let mut enc = Encoder::new();
        enc.array(1).unsigned(99u64);
        let query = enc.into_bytes();

        let result = BasicLocalQueryDispatcher.dispatch_query(&snapshot, &query);
        assert!(result.is_empty(), "unknown query tag should return empty bytes");
    }

    #[test]
    fn test_basic_dispatcher_empty_query_returns_empty() {
        let state = LedgerState::new(Era::Conway);
        let snapshot = state.snapshot();

        let result = BasicLocalQueryDispatcher.dispatch_query(&snapshot, &[]);
        assert!(result.is_empty(), "empty query bytes should return empty bytes");
    }
}
