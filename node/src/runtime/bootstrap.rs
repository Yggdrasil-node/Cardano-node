//! Per-peer connection bring-up: handshake, mux setup, and the 5
//! mini-protocol client constructors.
//!
//! Mirrors upstream:
//! - `Ouroboros.Network.NodeToNode.connectTo` — TCP connect, mux
//!   handshake, version negotiation
//! - `Ouroboros.Network.Mux` — mini-protocol bundle wiring
//! - `Ouroboros.Network.Subscription.Worker` — fallback peer iteration
//!   on primary connect failure (yggdrasil collapses this into a simple
//!   ordered retry loop in `bootstrap_with_attempt_state`)
//!
//! Three entry points:
//! - `bootstrap(config)` — single-peer convenience wrapper.
//! - `bootstrap_with_fallbacks(config, fallback_addrs)` — primary plus
//!   ordered fallbacks; the production path called by NtN runtime when
//!   the topology has multiple bootstrap peers.
//! - `bootstrap_with_attempt_state(config, attempt_state, tracer)` —
//!   the underlying driver shared by both entry points; takes a
//!   pre-built `PeerAttemptState` so the `ReconnectingRunState` outer
//!   loop can advance it across multiple reconnect cycles without
//!   resetting the duplicate-skip filter.
//!
//! Extracted from `runtime.rs` in R271g.

use std::net::SocketAddr;

use serde_json::json;
use yggdrasil_network::{
    BlockFetchClient, ChainSyncClient, HandshakeVersion, KeepAliveClient, MiniProtocolNum,
    NodeToNodeVersionData, PeerAttemptState, PeerConnection, PeerError, PeerSharingClient,
    TxSubmissionClient, peer_attempt_state,
};

use crate::tracer::{NodeTracer, trace_fields};

use super::peer_session::{NodeConfig, PeerSession};

/// Connect to an upstream peer and set up all protocol client drivers.
///
/// This is the main runtime entry point for syncing from a remote node.
///
/// # Errors
///
/// Returns `PeerError` if the TCP connection or handshake fails.
pub async fn bootstrap(config: &NodeConfig) -> Result<PeerSession, PeerError> {
    bootstrap_with_fallbacks(config, &[]).await
}

/// Connect to the primary upstream peer, retrying ordered fallbacks on failure.
///
/// The primary address in [`NodeConfig`] is always attempted first. Fallback
/// peers are then tried in the provided order, skipping duplicates.
pub async fn bootstrap_with_fallbacks(
    config: &NodeConfig,
    fallback_peer_addrs: &[SocketAddr],
) -> Result<PeerSession, PeerError> {
    let tracer = NodeTracer::disabled();
    let mut attempt_state = peer_attempt_state(config.peer_addr, fallback_peer_addrs);
    bootstrap_with_attempt_state(config, &mut attempt_state, &tracer).await
}

pub async fn bootstrap_with_attempt_state(
    config: &NodeConfig,
    attempt_state: &mut PeerAttemptState,
    tracer: &NodeTracer,
) -> Result<PeerSession, PeerError> {
    let proposals: Vec<(HandshakeVersion, NodeToNodeVersionData)> = config
        .protocol_versions
        .iter()
        .map(|v| {
            (
                *v,
                NodeToNodeVersionData {
                    network_magic: config.network_magic,
                    initiator_only_diffusion_mode: false,
                    peer_sharing: config.peer_sharing,
                    query: false,
                },
            )
        })
        .collect();

    let candidate_peer_addrs = attempt_state.attempt_order();

    let mut last_error = None;
    let mut connected_peer_addr = config.peer_addr;
    let mut conn_opt = None;

    for (attempt_index, peer_addr) in candidate_peer_addrs.into_iter().enumerate() {
        tracer.trace_runtime(
            "Net.PeerSelection",
            "Info",
            "attempting bootstrap peer",
            trace_fields([
                ("attempt", json!(attempt_index + 1)),
                ("peer", json!(peer_addr.to_string())),
                ("networkMagic", json!(config.network_magic)),
            ]),
        );

        match yggdrasil_network::peer_connect(peer_addr, proposals.clone()).await {
            Ok(conn) => {
                tracer.trace_runtime(
                    "Net.PeerSelection",
                    "Info",
                    "bootstrap peer connected",
                    trace_fields([
                        ("attempt", json!(attempt_index + 1)),
                        ("peer", json!(peer_addr.to_string())),
                    ]),
                );
                connected_peer_addr = peer_addr;
                attempt_state.record_success(peer_addr);
                conn_opt = Some(conn);
                break;
            }
            Err(err) => {
                tracer.trace_runtime(
                    "Net.PeerSelection",
                    "Warning",
                    "bootstrap peer failed",
                    trace_fields([
                        ("attempt", json!(attempt_index + 1)),
                        ("peer", json!(peer_addr.to_string())),
                        ("error", json!(err.to_string())),
                    ]),
                );
                last_error = Some(err);
            }
        }
    }

    let mut conn: PeerConnection = match conn_opt {
        Some(conn) => conn,
        None => return Err(last_error.expect("at least one peer candidate")),
    };

    let cs = conn
        .protocols
        .remove(&MiniProtocolNum::CHAIN_SYNC)
        .ok_or_else(|| PeerError::HandshakeProtocol {
            detail: "missing ChainSync protocol handle".into(),
        })?;
    let bf = conn
        .protocols
        .remove(&MiniProtocolNum::BLOCK_FETCH)
        .ok_or_else(|| PeerError::HandshakeProtocol {
            detail: "missing BlockFetch protocol handle".into(),
        })?;
    let ka = conn
        .protocols
        .remove(&MiniProtocolNum::KEEP_ALIVE)
        .ok_or_else(|| PeerError::HandshakeProtocol {
            detail: "missing KeepAlive protocol handle".into(),
        })?;
    let tx = conn
        .protocols
        .remove(&MiniProtocolNum::TX_SUBMISSION)
        .ok_or_else(|| PeerError::HandshakeProtocol {
            detail: "missing TxSubmission protocol handle".into(),
        })?;

    // Extract weight handles before consuming the ProtocolHandles.
    let mut protocol_weights = vec![
        (MiniProtocolNum::CHAIN_SYNC, cs.weight_handle()),
        (MiniProtocolNum::BLOCK_FETCH, bf.weight_handle()),
        (MiniProtocolNum::KEEP_ALIVE, ka.weight_handle()),
        (MiniProtocolNum::TX_SUBMISSION, tx.weight_handle()),
    ];

    let peer_sharing = conn.protocols.remove(&MiniProtocolNum::PEER_SHARING);
    let peer_sharing = if conn.version_data.peer_sharing > 0 {
        if let Some(ref ps) = peer_sharing {
            protocol_weights.push((MiniProtocolNum::PEER_SHARING, ps.weight_handle()));
        }
        peer_sharing.map(PeerSharingClient::new)
    } else {
        None
    };

    Ok(PeerSession {
        connected_peer_addr,
        chain_sync: ChainSyncClient::new(cs),
        block_fetch: Some(BlockFetchClient::new(bf)),
        keep_alive: KeepAliveClient::new(ka),
        tx_submission: TxSubmissionClient::new(tx),
        peer_sharing,
        mux: conn.mux,
        version: conn.version,
        version_data: conn.version_data,
        protocol_weights,
    })
}
