//! Peer governor — promotion, demotion, and valency enforcement.
//!
//! The governor evaluates the current [`PeerRegistry`] state against
//! configured targets and produces [`GovernorAction`] decisions.  The
//! runtime executes those actions by connecting/disconnecting peers and
//! updating the registry.
//!
//! This follows the upstream Ouroboros design where the governor is a
//! pure decision function separated from effectful connection management.
//!
//! Reference: `Ouroboros.Network.PeerSelection.Governor`.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side parent shell that splits
//! upstream `Ouroboros.Network.PeerSelection.Governor.hs` into
//! five sub-modules: `types.rs`, `state.rs`, `churn.rs`,
//! `peer_metric.rs`, `counters.rs`. The upstream module is a
//! single ~3000-line file containing all governor state + decision
//! functions; Yggdrasil splits along functional seams (state +
//! tick orchestration / churn cycle / peer scoring / view-layer
//! counters) for cohesion.

// `governor.rs` is a thin orchestration shell after R270a–R270e split
// the per-domain implementation into `governor/{types,churn,peer_metric,state,counters}.rs`.
// The `#[cfg(test)]` imports below exist only for the `governor/tests.rs`
// descendant test module, which exercises the re-exported public surface.

#[cfg(test)]
use crate::ledger_peers_provider::LedgerStateJudgement;
#[cfg(test)]
use crate::multiplexer::MiniProtocolNum;
#[cfg(test)]
use crate::peer_registry::{PeerRegistry, PeerSource, PeerStatus};
#[cfg(test)]
use crate::root_peers::{UseBootstrapPeers, UseLedgerPeers};
#[cfg(test)]
use std::net::SocketAddr;
#[cfg(test)]
use std::time::{Duration, Instant};

pub mod types;
pub use types::{
    AssociationMode, GovernorTargets, LocalRootTargets, NodePeerSharing, PeerSelectionMode,
    compute_association_mode, is_node_able_to_make_progress, peer_selection_mode,
    requires_bootstrap_peers,
};

pub mod churn;
pub use churn::{
    ChurnConfig, ChurnMode, ChurnPhase, ChurnRegime, ConsensusMode, FetchMode, churn_decrease,
    churn_decrease_active, churn_decrease_established, churn_mode_from_fetch_mode,
    fetch_mode_from_judgement, pick_churn_regime,
};
pub mod peer_metric;
pub use peer_metric::{
    HIGH_DENSITY_BONUS, HotPeerScheduling, LOW_DENSITY_THRESHOLD, PeerFailureRecord, PeerMetrics,
    PickPolicy, RequestBackoffState, Xorshift64, hot_peers_remote,
};

pub mod state;
pub use state::{
    GovernorAction, GovernorState, PeerLifetimeStats, enforce_local_root_valency,
    evaluate_cold_to_warm_big_ledger_promotions, evaluate_cold_to_warm_promotions,
    evaluate_forget_cold_peers, evaluate_forget_failed_peers, evaluate_hot_promotions,
    evaluate_hot_to_warm_big_ledger_demotions, evaluate_hot_to_warm_demotions,
    evaluate_known_peer_discovery, evaluate_peer_share_requests, evaluate_request_big_ledger_peers,
    evaluate_request_public_roots, evaluate_sensitive_hot_demotions,
    evaluate_sensitive_warm_demotions, evaluate_warm_to_cold_big_ledger_demotions,
    evaluate_warm_to_cold_demotions, evaluate_warm_to_hot_big_ledger_promotions,
    evaluate_warm_to_hot_promotions, filter_sensitive_promotions, governor_tick,
    has_only_trustable_established_peers, is_big_ledger, is_trustable_peer,
    trustable_local_root_set,
};

pub mod counters;
pub use counters::{
    ConnectionManagerCounters, OutboundConnectionsState, PeerSelectionCounters,
    PeerSelectionTimeouts, compute_outbound_connections_state,
};

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests;
