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
//!       └─ start_mux_unix([..., NTC_LOCAL_TX_MONITOR])
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
    AcquireFailure, AcquireTarget,
    LocalStateQueryAcquiredRequest, LocalStateQueryIdleRequest,
    LocalStateQueryServer, LocalStateQueryServerError,
    LocalTxMonitorAcquiredRequest, LocalTxMonitorIdleRequest,
    LocalTxMonitorServer, LocalTxMonitorServerError,
    LocalTxRequest, LocalTxSubmissionServer, LocalTxSubmissionServerError,
    MiniProtocolDir,
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
    evaluator: Option<Arc<dyn yggdrasil_ledger::plutus_validation::PlutusEvaluator + Send + Sync>>,
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
                let eval_ref = evaluator.as_ref().map(|e| {
                    e.as_ref() as &dyn yggdrasil_ledger::plutus_validation::PlutusEvaluator
                });
                match add_tx_to_shared_mempool(
                    &mut ledger_state,
                    &mempool,
                    submitted_tx,
                    current_slot,
                    eval_ref,
                ) {
                    Ok(MempoolAddTxResult::MempoolTxAdded(_)) => {
                        server.accept().await?;
                    }
                    Ok(MempoolAddTxResult::MempoolTxRejected(_, reason)) => {
                        let reason_bytes = encode_rejection_reason(&format!("{reason}"));
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
pub async fn run_local_tx_monitor_session(
    mut server: LocalTxMonitorServer,
    mempool: SharedMempool,
) -> Result<(), LocalTxMonitorSessionError> {
    loop {
        match server.recv_idle_request().await? {
            LocalTxMonitorIdleRequest::Done => return Ok(()),
            LocalTxMonitorIdleRequest::Acquire => {
                // Take a snapshot and enter the acquired loop.
                let snapshot = mempool.snapshot();
                let tip_slot = 0u64; // Slot of last applied block; 0 when unknown.
                server.acquired(tip_slot).await?;

                let mut tx_iter = snapshot.mempool_txids_after(yggdrasil_mempool::MEMPOOL_ZERO_IDX).into_iter();

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
                            // Re-acquire: take a fresh snapshot.
                            let new_snapshot = mempool.snapshot();
                            server.acquired(tip_slot).await?;
                            tx_iter = new_snapshot.mempool_txids_after(yggdrasil_mempool::MEMPOOL_ZERO_IDX).into_iter();
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
/// Starts the mux over the provided `stream`, builds all server drivers, and
/// spawns independent tokio tasks for each mini-protocol.  Returns the
/// [`yggdrasil_network::MuxHandle`] so the caller can abort on shutdown.
#[cfg(unix)]
pub async fn run_local_client_session<I, V, L>(
    stream: tokio::net::UnixStream,
    chain_db: Arc<RwLock<ChainDb<I, V, L>>>,
    mempool: SharedMempool,
    dispatcher: Arc<dyn LocalQueryDispatcher>,
    evaluator: Option<Arc<dyn yggdrasil_ledger::plutus_validation::PlutusEvaluator + Send + Sync>>,
) -> yggdrasil_network::MuxHandle
where
    I: ImmutableStore + Send + Sync + 'static,
    V: VolatileStore + Send + Sync + 'static,
    L: LedgerStore + Send + Sync + 'static,
{
    use yggdrasil_network::{start_mux_unix, MiniProtocolNum};

    let protocols = [
        MiniProtocolNum::NTC_LOCAL_TX_SUBMISSION,
        MiniProtocolNum::NTC_LOCAL_STATE_QUERY,
        MiniProtocolNum::NTC_LOCAL_TX_MONITOR,
    ];
    let (mut handles, mux_handle) =
        start_mux_unix(stream, MiniProtocolDir::Responder, &protocols, 32);

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
    tokio::spawn(async move {
        let _ = run_local_tx_submission_session(tx_server, tx_chain_db, tx_mempool, tx_evaluator).await;
    });

    // Spawn LocalStateQuery task.
    let sq_chain_db = Arc::clone(&chain_db);
    tokio::spawn(async move {
        let _ = run_local_state_query_session(sq_server, sq_chain_db, dispatcher).await;
    });

    // Spawn LocalTxMonitor task.
    tokio::spawn(async move {
        let _ = run_local_tx_monitor_session(tm_server, mempool).await;
    });

    mux_handle
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
pub async fn run_local_accept_loop<I, V, L, F>(
    socket_path: &Path,
    chain_db: Arc<RwLock<ChainDb<I, V, L>>>,
    mempool: SharedMempool,
    dispatcher: Arc<dyn LocalQueryDispatcher>,
    evaluator: Option<Arc<dyn yggdrasil_ledger::plutus_validation::PlutusEvaluator + Send + Sync>>,
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

                tokio::spawn(async move {
                    let mux = run_local_client_session(stream, db, mp, disp, eval).await;
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
pub struct BasicLocalQueryDispatcher;

impl LocalQueryDispatcher for BasicLocalQueryDispatcher {
    fn dispatch_query(&self, snapshot: &LedgerStateSnapshot, query: &[u8]) -> Vec<u8> {
        use yggdrasil_ledger::{CborEncode, Decoder, Encoder};

        // Decode query as [tag, ...] CBOR array.
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
            Some(1) => {
                // QueryChainTip — respond with CBOR-encoded Point.
                snapshot.tip().encode_cbor(&mut enc);
            }
            Some(2) => {
                // QueryCurrentEpoch — respond with epoch number as a plain u64.
                enc.unsigned(snapshot.current_epoch().0);
            }
            Some(3) => {
                // QueryProtocolParameters — respond with CBOR-encoded
                // ProtocolParameters map or CBOR null.
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
                        if let Some(acct) = yggdrasil_ledger::RewardAccount::from_bytes(acct_bytes) {
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
                            if let Ok(txin) = yggdrasil_ledger::eras::shelley::ShelleyTxIn::decode_cbor(&mut pdec) {
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
                            if let Ok(cred) = yggdrasil_ledger::StakeCredential::decode_cbor(&mut pdec) {
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

        let result = BasicLocalQueryDispatcher.dispatch_query(&snapshot, &query);
        assert!(!result.is_empty(), "QueryCurrentEra should return a non-empty response");
    }

    #[test]
    fn test_basic_dispatcher_chain_tip() {
        use yggdrasil_ledger::Encoder;

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
        use yggdrasil_ledger::Encoder;

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
        use yggdrasil_ledger::Encoder;

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

    #[test]
    fn test_basic_dispatcher_protocol_params_null_when_absent() {
        use yggdrasil_ledger::Encoder;

        let state = LedgerState::new(Era::Conway);
        let snapshot = state.snapshot();

        let mut enc = Encoder::new();
        enc.array(1).unsigned(3u64);
        let query = enc.into_bytes();

        let result = BasicLocalQueryDispatcher.dispatch_query(&snapshot, &query);
        assert!(!result.is_empty(), "QueryProtocolParameters should return CBOR null");
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

        let result = BasicLocalQueryDispatcher.dispatch_query(&snapshot, &query);
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

        let result = BasicLocalQueryDispatcher.dispatch_query(&snapshot, &query);
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

        let result = BasicLocalQueryDispatcher.dispatch_query(&snapshot, &query);
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

        let result = BasicLocalQueryDispatcher.dispatch_query(&snapshot, &query);
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

        let result = BasicLocalQueryDispatcher.dispatch_query(&snapshot, &query);
        assert!(!result.is_empty(), "GetConstitution should return a non-empty CBOR response");
    }

    #[test]
    fn test_basic_dispatcher_get_gov_state_empty() {
        use yggdrasil_ledger::Encoder;

        let state = LedgerState::new(Era::Conway);
        let snapshot = state.snapshot();

        let mut enc = Encoder::new();
        enc.array(1).unsigned(9u64);
        let query = enc.into_bytes();

        let result = BasicLocalQueryDispatcher.dispatch_query(&snapshot, &query);
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

        let result = BasicLocalQueryDispatcher.dispatch_query(&snapshot, &query);
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

        let result = BasicLocalQueryDispatcher.dispatch_query(&snapshot, &query);
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

        let result = BasicLocalQueryDispatcher.dispatch_query(&snapshot, &query);
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

        let result = BasicLocalQueryDispatcher.dispatch_query(&snapshot, &query);
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

        let result = BasicLocalQueryDispatcher.dispatch_query(&snapshot, &query);
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

        let result = BasicLocalQueryDispatcher.dispatch_query(&snapshot, &query);
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

        let result = BasicLocalQueryDispatcher.dispatch_query(&snapshot, &query);
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

        let result = BasicLocalQueryDispatcher.dispatch_query(&snapshot, &query);
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

        let result = BasicLocalQueryDispatcher.dispatch_query(&snapshot, &query);
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

        let result = BasicLocalQueryDispatcher.dispatch_query(&snapshot, &query);
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

        let result = BasicLocalQueryDispatcher.dispatch_query(&snapshot, &query);
        assert!(!result.is_empty());
        // Empty CBOR map is 0xa0.
        assert_eq!(result, vec![0xa0]);
    }
}
