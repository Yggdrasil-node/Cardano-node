//! Graceful shutdown signal handling.
//!
//! Mirrors upstream `Cardano.Node.Handlers.Shutdown` (189 lines / 6 KB).
//! Yggdrasil's variant is much smaller because we only handle the signal
//! waiter; upstream additionally manages the IPC-based shutdown protocol
//! (`scIPC = ShutdownConfig`) which Yggdrasil does not yet implement.
//!
//! Reference: <https://github.com/IntersectMBO/cardano-node/blob/master/cardano-node/src/Cardano/Node/Handlers/Shutdown.hs>
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side OS-signal +
//! graceful-shutdown handler (CTRL-C / SIGTERM) running on a
//! dedicated tokio task that signals the runtime supervisor.
//! Mirrors the shutdown-signal half of upstream
//! `Cardano.Node.Run.runNode` + `Ouroboros.Consensus.Node.exit`.
//! Upstream wires this inline; Yggdrasil isolates the tokio-
//! specific signal handling.

/// Wait for the operator's shutdown signal (`SIGINT` or `SIGTERM` on
/// Unix; `Ctrl-C` on non-Unix). Returns the human-readable name of the
/// signal that fired so the trace logs identify which one was caught.
///
/// Used by `node/src/main.rs::run_node` as the top of a `tokio::select!`
/// arm that completes when the signal arrives, gracefully draining the
/// governor / sync / NtC server tasks before exiting.
#[cfg(unix)]
pub async fn wait_for_shutdown_signal() -> &'static str {
    use tokio::signal::unix::{SignalKind, signal};

    let mut sigint = signal(SignalKind::interrupt()).expect("install SIGINT handler");
    let mut sigterm = signal(SignalKind::terminate()).expect("install SIGTERM handler");
    tokio::select! {
        _ = sigint.recv() => "SIGINT",
        _ = sigterm.recv() => "SIGTERM",
    }
}

/// Non-Unix shutdown waiter — only `Ctrl-C` is observed.
#[cfg(not(unix))]
pub async fn wait_for_shutdown_signal() -> &'static str {
    tokio::signal::ctrl_c().await.ok();
    "CtrlC"
}
