//! Top-level NtC accept loop and per-connection orchestrator.
//!
//! Mirrors upstream `Ouroboros.Network.NodeToClient` server-side accept
//! path. `run_local_accept_loop` binds a Unix-domain socket and accepts
//! client connections; `run_local_client_session` runs the NtC handshake
//! and spawns the three per-mini-protocol session tasks
//! (`LocalTxSubmission`, `LocalStateQuery`, `LocalTxMonitor`) for the
//! accepted connection.
//!
//! Reference: <https://github.com/IntersectMBO/ouroboros-network/blob/master/ouroboros-network/src/Ouroboros/Network/NodeToClient.hs>

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use yggdrasil_consensus::mempool::SharedMempool;
use yggdrasil_network::{LocalStateQueryServer, LocalTxMonitorServer, LocalTxSubmissionServer};
use yggdrasil_storage::{ChainDb, ImmutableStore, LedgerStore, VolatileStore};

use crate::local_server::{
    LocalQueryDispatcher, LocalServerError, run_local_state_query_session,
    run_local_tx_monitor_session, run_local_tx_submission_session,
};
use crate::tracer::NodeMetrics;

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
#[allow(clippy::too_many_arguments)] // thin orchestration entry-point; each parameter is a shared handle wired from the node bootstrap
pub async fn run_local_client_session<I, V, L>(
    stream: tokio::net::UnixStream,
    network_magic: u32,
    chain_db: Arc<RwLock<ChainDb<I, V, L>>>,
    mempool: SharedMempool,
    dispatcher: Arc<dyn LocalQueryDispatcher>,
    evaluator: Option<Arc<dyn yggdrasil_ledger::plutus_validation::PlutusEvaluator + Send + Sync>>,
    metrics: Option<Arc<NodeMetrics>>,
    storage_dir: Option<PathBuf>,
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
    let sq_storage_dir = storage_dir.clone();
    tokio::spawn(async move {
        let _ =
            run_local_state_query_session(sq_server, sq_chain_db, dispatcher, sq_storage_dir).await;
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
    storage_dir: Option<PathBuf>,
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
                let sd = storage_dir.clone();

                tokio::spawn(async move {
                    let mux =
                        run_local_client_session(stream, network_magic, db, mp, disp, eval, met, sd)
                            .await;
                    // Mux runs until either protocol task finishes or the
                    // connection drops; we do not abort here since each task
                    // terminates cleanly on `MsgDone` or socket close.
                    let _ = mux;
                });
            }
        }
    }
}
