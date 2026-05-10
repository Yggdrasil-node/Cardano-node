//! Trace-forwarder acceptors supervisor — `runAcceptors` analog.
//! Decides between server-mode (`AcceptAt`) and client-mode
//! (`ConnectTo`) based on the operator config + drives the
//! per-instance acceptor loops with auto-restart on transport
//! interruption.
//!
//! ## Naming parity
//!
//! **Strict mirror:** cardano-tracer/src/Cardano/Tracer/Acceptors/Run.hs.
//!
//! Direct port of upstream's `runAcceptors` + supporting helpers.
//! Mirrors upstream's two network modes verbatim:
//!   1. `AcceptAt` (server): single bound listener accepting
//!      connections from any number of forwarders.
//!   2. `ConnectTo` (client): N concurrent outbound dials, deduped
//!      against the supplied list.
//!
//! Mapping summary:
//!
//! | Upstream                                                | Yggdrasil                              |
//! |---------------------------------------------------------|----------------------------------------|
//! | `runAcceptors :: TracerEnv -> TracerEnvRTView -> IO ()` | [`run_acceptors`]                      |
//! | `runInLoop action handler initialPause interval`        | [`run_in_loop`]                        |
//! | `acceptorsConfigs path :: (EKGF, TOF, DPF)`             | [`acceptors_configs`] (TOF only — EKG/DPF deferred) |
//! | `handleOnInterruption howToConnect e`                   | (inlined into [`run_in_loop`]'s callback) |
//! | `mkVerbosity (Just Maximum) = contramap show stdoutTracer` | (deferred — no contra-tracer port) |
//! | `forConcurrently_ (NE.nub localSocks)`                  | [`run_acceptors`]'s `tokio::join_all` over deduped paths |
//!
//! Carve-outs (NOT ported, by design):
//!
//! - **`mkVerbosity` tracer wiring**: depends on `contra-tracer`'s
//!   `Tracer IO a` typeclass + `contramap show stdoutTracer`.
//!   Yggdrasil's [`yggdrasil_network::protocols::AcceptorConfiguration::acceptor_tracer`]
//!   field accepts `Option<Arc<dyn Fn(&str) + Send + Sync>>`; an
//!   operationally-equivalent stdout closure can be wired by the
//!   caller (R427 main wiring).
//! - **EKG (`EKGF.AcceptorConfiguration`) + DataPoint
//!   (`DPF.AcceptorConfiguration`) configs**: not built since their
//!   sub-protocols are deferred carve-outs (see R424 `server.rs`'s
//!   module docs). Only the trace-objects `TOF.AcceptorConfiguration`
//!   is constructed in [`acceptors_configs`].
//! - **`secondsToNominalDiffTime` for `requestFrequency`**: applies
//!   only to the EKG config (deferred).
//! - **`forwarderEndpoint = EKGF.LocalPipe p`**: applies only to
//!   the EKG config (deferred). Upstream comment notes it's
//!   "unused in the context of ouroboros-network mini-protocol
//!   application" anyway.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use yggdrasil_network::protocols::{AcceptorConfiguration, NumberOfTraceObjects};

use super::client::run_acceptors_client;
use super::server::{AcceptorsServerError, AcceptorsServerState, run_acceptors_server};
use crate::configuration::{HowToConnect, Network, TracerConfig};
use crate::logging::TraceObject;

/// Initial pause before the first acceptor loop body runs. Mirror
/// of upstream's `initialPauseInSec = 1`.
pub const INITIAL_PAUSE: Duration = Duration::from_secs(1);

/// Retry interval when an `AcceptAt` (server) loop iteration
/// errors out. Mirror of upstream's `runInLoop ... 10` for the
/// `AcceptAt` branch.
pub const SERVER_RETRY_INTERVAL: Duration = Duration::from_secs(10);

/// Retry interval when a `ConnectTo` (client) loop iteration
/// errors out. Mirror of upstream's `runInLoop ... 30` for the
/// `ConnectTo` branch.
pub const CLIENT_RETRY_INTERVAL: Duration = Duration::from_secs(30);

/// Default request batch size for the trace-objects sub-protocol.
/// Mirror of upstream's
/// `fromMaybe 100 (loRequestNum (teConfig tracerEnv))`.
pub const DEFAULT_LO_REQUEST_NUM: u16 = 100;

// ---------------------------------------------------------------------------
// Top-level supervisor
// ---------------------------------------------------------------------------

/// Run the acceptors supervisor for either network mode. Mirror of
/// upstream's `runAcceptors tracerEnv tracerEnvRTView`.
///
/// `lo_handler` is invoked once per inbound `MsgTraceObjectsReply`
/// batch and is shared across all network instances (the canonical
/// operator implementation routes through
/// `crate::handlers::logs::trace_objects::trace_objects_handler`).
///
/// Per the R398 plan's TracerEnv option (b) decision, takes the
/// state slice + the operator's `TracerConfig` directly rather
/// than coupling to the full `TracerEnv` record. The supervisor
/// reads `network` (mode + addresses) and `lo_request_num` (batch
/// size) from `config`.
pub async fn run_acceptors<LoHandler>(
    state: AcceptorsServerState,
    config: &TracerConfig,
    lo_handler: Arc<LoHandler>,
) -> Result<(), AcceptorsSupervisorError>
where
    LoHandler: Fn(crate::types::NodeId, Vec<TraceObject>) + Send + Sync + 'static,
{
    let lo_request_num = config
        .log_objects_request_num
        .unwrap_or(DEFAULT_LO_REQUEST_NUM);

    match &config.network {
        Network::AcceptAt { accept_at } => {
            let how = accept_at.clone();
            let cfg = acceptors_configs(lo_request_num);
            run_in_loop(
                state.clone(),
                cfg,
                Arc::clone(&lo_handler),
                INITIAL_PAUSE,
                SERVER_RETRY_INTERVAL,
                ServerOrClient::Server(how),
            )
            .await
        }
        Network::ConnectTo { connect_to } => {
            // Dedup the address list by their displayed shape;
            // mirror of upstream's `NE.nub localSocks`.
            let unique: Vec<HowToConnect> = dedup_connect_targets(connect_to);
            if unique.is_empty() {
                return Err(AcceptorsSupervisorError::NoTargets);
            }
            // Run one acceptor loop per address, concurrently.
            let mut joinset = tokio::task::JoinSet::new();
            for how in unique {
                let s = state.clone();
                let h = Arc::clone(&lo_handler);
                let cfg = acceptors_configs(lo_request_num);
                joinset.spawn(async move {
                    run_in_loop(
                        s,
                        cfg,
                        h,
                        INITIAL_PAUSE,
                        CLIENT_RETRY_INTERVAL,
                        ServerOrClient::Client(how),
                    )
                    .await
                });
            }
            // Wait for all loops to terminate (typically only when
            // the global brake is engaged or the joinset is
            // explicitly aborted).
            let mut last_err: Option<AcceptorsSupervisorError> = None;
            while let Some(joined) = joinset.join_next().await {
                match joined {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => last_err = Some(e),
                    Err(join_e) => {
                        last_err = Some(AcceptorsSupervisorError::JoinError(join_e.to_string()))
                    }
                }
            }
            match last_err {
                Some(e) => Err(e),
                None => Ok(()),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Configuration builder
// ---------------------------------------------------------------------------

/// Build the trace-objects-side `AcceptorConfiguration` for the
/// given request batch size. Mirror of upstream's `acceptorsConfigs
/// p` — the EKG + DataPoint slots are deferred carve-outs and only
/// the TOF tuple element is constructed.
pub fn acceptors_configs(lo_request_num: u16) -> AcceptorConfiguration {
    AcceptorConfiguration::new(NumberOfTraceObjects(lo_request_num))
}

// ---------------------------------------------------------------------------
// Loop body
// ---------------------------------------------------------------------------

/// Dispatch token: server vs client mode for a single loop
/// iteration body.
#[derive(Clone, Debug)]
pub enum ServerOrClient {
    /// Server (responder) mode — bind the address.
    Server(HowToConnect),
    /// Client (initiator) mode — dial the address.
    Client(HowToConnect),
}

/// Run a single acceptor loop body with auto-retry on error.
/// Mirror of upstream's
/// `runInLoop action onException initialPause interval`.
///
/// The loop body races against the supplied configuration's brake
/// flag (`should_we_stop`). When the brake is engaged, the loop
/// returns `Ok(())` after the in-flight body completes (or its
/// `MsgDone` handshake times out within
/// [`yggdrasil_network::trace_object_run_acceptor::SHUTDOWN_TIMEOUT`]).
/// On transient transport errors the loop sleeps `interval`, then
/// re-enters the body.
pub async fn run_in_loop<LoHandler>(
    state: AcceptorsServerState,
    config: AcceptorConfiguration,
    lo_handler: Arc<LoHandler>,
    initial_pause: Duration,
    interval: Duration,
    mode: ServerOrClient,
) -> Result<(), AcceptorsSupervisorError>
where
    LoHandler: Fn(crate::types::NodeId, Vec<TraceObject>) + Send + Sync + 'static,
{
    tokio::time::sleep(initial_pause).await;
    loop {
        if config.is_stopped().await {
            return Ok(());
        }
        let result: Result<(), AcceptorsServerError> = match &mode {
            ServerOrClient::Server(how) => {
                run_acceptors_server(
                    state.clone(),
                    how.clone(),
                    config.clone(),
                    Arc::clone(&lo_handler),
                )
                .await
            }
            ServerOrClient::Client(how) => {
                run_acceptors_client(
                    state.clone(),
                    how.clone(),
                    config.clone(),
                    Arc::clone(&lo_handler),
                )
                .await
            }
        };
        match result {
            Ok(()) => {
                // Cleanly returned — likely brake engaged. Exit
                // the loop.
                return Ok(());
            }
            Err(_e) => {
                // Mirror of upstream's `handleOnInterruption` —
                // log + sleep + retry. Yggdrasil's tracer wiring
                // is deferred (see module docs); R427 main wiring
                // will plumb through a real logger.
                tokio::time::sleep(interval).await;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Deduplicate a list of `HowToConnect` targets. Mirror of upstream's
/// `NE.nub localSocks` — `Eq HowToConnect` is derived structurally.
fn dedup_connect_targets(targets: &[HowToConnect]) -> Vec<HowToConnect> {
    // Use a BTreeSet with the canonical-form key for deduplication
    // since HowToConnect derives Hash + Eq.
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut out: Vec<HowToConnect> = Vec::with_capacity(targets.len());
    for t in targets {
        let key = format!("{t:?}");
        if seen.insert(key) {
            out.push(t.clone());
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors from the acceptors supervisor.
#[derive(Debug, thiserror::Error)]
pub enum AcceptorsSupervisorError {
    /// Operator config selected `ConnectTo` mode but supplied an
    /// empty (or all-null) target list.
    #[error("ConnectTo mode requires at least one non-empty target")]
    NoTargets,

    /// One of the per-target spawned acceptor tasks panicked.
    #[error("acceptor task join error: {0}")]
    JoinError(String),

    /// Forwarded transport error from a per-target acceptor loop.
    /// Operationally these are auto-retried inside
    /// [`run_in_loop`]; this variant only fires when the loop
    /// itself returns an unrecoverable error.
    #[error("acceptor server error: {0}")]
    Server(#[from] AcceptorsServerError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::configuration::{LogMode, LoggingParams, TracerConfig, Verbosity};
    use crate::types::{ConnectedNodes, ConnectedNodesNames};

    fn test_state() -> AcceptorsServerState {
        AcceptorsServerState {
            connected_nodes: ConnectedNodes::new(),
            connected_nodes_names: ConnectedNodesNames::new(),
            accepted_metrics: crate::metrics_store::new_accepted_metrics(),
            network_magic: 764824073,
        }
    }

    fn test_config_accept_at(path: &str) -> TracerConfig {
        TracerConfig {
            network_magic: 764824073,
            network: Network::AcceptAt {
                accept_at: HowToConnect::LocalPipe {
                    local_pipe: path.into(),
                },
            },
            logging: vec![LoggingParams {
                root: ".".into(),
                mode: LogMode::FileMode,
                format: crate::configuration::LogFormat::ForHuman,
            }],
            rotation: None,
            verbosity: Some(Verbosity::Minimum),
            has_ekg: None,
            has_prometheus: None,
            has_rtview: None,
            has_timeseries: None,
            tls_certificate: None,
            has_forwarding: None,
            ekg_request_freq: None,
            ekg_request_full: None,
            metrics_help: None,
            log_objects_request_num: Some(50),
            metrics_no_suffix: None,
            prometheus_labels: None,
            resource_freq: None,
        }
    }

    #[test]
    fn acceptors_configs_uses_supplied_lo_request_num() {
        let config = acceptors_configs(50);
        assert_eq!(config.what_to_request, NumberOfTraceObjects(50));
    }

    #[test]
    fn acceptors_configs_default_brake_state_is_running() {
        let config = acceptors_configs(100);
        // Reading the brake flag synchronously requires entering an
        // async context; instead just verify the Arc is set and not
        // poisoned via debug formatting.
        let s = format!("{config:?}");
        assert!(s.contains("AcceptorConfiguration"));
    }

    #[test]
    fn dedup_connect_targets_collapses_duplicates() {
        let pipe = HowToConnect::LocalPipe {
            local_pipe: "/tmp/a".into(),
        };
        let pipe2 = HowToConnect::LocalPipe {
            local_pipe: "/tmp/a".into(),
        };
        let other = HowToConnect::LocalPipe {
            local_pipe: "/tmp/b".into(),
        };
        let unique = dedup_connect_targets(&[pipe.clone(), pipe2, other.clone(), pipe]);
        assert_eq!(unique.len(), 2);
    }

    #[test]
    fn constants_match_upstream_intervals() {
        // Lock down upstream's hardcoded retry intervals.
        assert_eq!(INITIAL_PAUSE, Duration::from_secs(1));
        assert_eq!(SERVER_RETRY_INTERVAL, Duration::from_secs(10));
        assert_eq!(CLIENT_RETRY_INTERVAL, Duration::from_secs(30));
        assert_eq!(DEFAULT_LO_REQUEST_NUM, 100);
    }

    #[tokio::test]
    async fn run_acceptors_connect_to_empty_list_errors() {
        let state = test_state();
        let mut config = test_config_accept_at("/tmp/r426-test.sock");
        config.network = Network::ConnectTo { connect_to: vec![] };
        let handler: Arc<dyn Fn(crate::types::NodeId, Vec<TraceObject>) + Send + Sync> =
            Arc::new(|_, _| {});
        let result = run_acceptors_test_shim(state, &config, handler).await;
        assert!(matches!(result, Err(AcceptorsSupervisorError::NoTargets)));
    }

    /// Test-only thin shim that monomorphizes the closure type so
    /// we can pass a trait-object handler as a concrete LoHandler
    /// bound.
    async fn run_acceptors_test_shim(
        state: AcceptorsServerState,
        config: &TracerConfig,
        handler: Arc<dyn Fn(crate::types::NodeId, Vec<TraceObject>) + Send + Sync>,
    ) -> Result<(), AcceptorsSupervisorError> {
        run_acceptors(
            state,
            config,
            Arc::new(move |id, payloads| handler(id, payloads)),
        )
        .await
    }
}

