//! DMQ mini-protocol policy — the `SigSubmission` decision policy and
//! ingress limit.
//!
//! ## Naming parity
//!
//! **Strict mirror:** deps/dmq-node/dmq-node/src/DMQ/Policy.hs.
//!
//! `TxDecisionPolicy` (`Ouroboros.Network.TxSubmission.Inbound.V2`)
//! and `MiniProtocolLimits` (`Network.Mux.Types`) are network / mux
//! types upstream; `crates/network` does not expose them by those
//! names, so dmq-node carries its own mirrors — the R731 / R732
//! dmq-node-local pattern. The constants here are the DMQ-specific
//! policy values.

/// Maximum size, in bytes, of a single DMQ signature.
///
/// Mirror of upstream `maxSigSize = 2800`.
pub const MAX_SIG_SIZE: u64 = 2800;

/// Maximum number of signatures in-flight per peer.
///
/// Mirror of upstream `maxSigsInflight = 33`.
pub const MAX_SIGS_INFLIGHT: u64 = 33;

/// The `SigSubmission` mini-protocol decision policy.
///
/// Mirror of upstream `TxDecisionPolicy`, carried locally.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SigDecisionPolicy {
    /// Maximum number of signature identifiers to request at once.
    pub max_num_tx_ids_to_request: u64,
    /// Maximum number of unacknowledged signature identifiers.
    pub max_unacknowledged_tx_ids: u64,
    /// Maximum total in-flight signature bytes per peer.
    pub txs_size_inflight_per_peer: u64,
    /// How many peers may carry the same signature in-flight.
    pub tx_inflight_multiplicity: u64,
    /// Minimum lifetime of a buffered signature, in seconds.
    pub buffered_txs_min_lifetime_secs: u64,
    /// Peer-score decay rate.
    pub score_rate: f64,
    /// Maximum peer score, in seconds.
    pub score_max: f64,
}

/// The `SigSubmission` decision policy used by dmq-node.
///
/// Mirror of upstream `sigDecisionPolicy`:
/// `maxUnacknowledgedTxIds` is `4 * maxSigsInflight` (132),
/// `txsSizeInflightPerPeer` is `maxSigSize * maxSigsInflight` (92 400),
/// `scoreMax` is `15 * 60` seconds.
pub fn sig_decision_policy() -> SigDecisionPolicy {
    SigDecisionPolicy {
        max_num_tx_ids_to_request: MAX_SIGS_INFLIGHT,
        max_unacknowledged_tx_ids: 4 * MAX_SIGS_INFLIGHT,
        txs_size_inflight_per_peer: MAX_SIG_SIZE * MAX_SIGS_INFLIGHT,
        tx_inflight_multiplicity: 1,
        buffered_txs_min_lifetime_secs: 0,
        score_rate: 0.1,
        score_max: 15.0 * 60.0,
    }
}

/// A mini-protocol's ingress-queue limit.
///
/// Mirror of upstream `MiniProtocolLimits`, carried locally.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MiniProtocolLimits {
    /// Maximum bytes queued on the protocol's ingress side.
    pub maximum_ingress_queue: u64,
}

/// Add the 10% margin upstream `Policy.hs` applies to the ingress
/// limit — `addMargin x = x + x \`div\` 10`.
fn add_margin(x: u64) -> u64 {
    x + x / 10
}

/// The `SigSubmission` mini-protocol ingress limit.
///
/// Mirror of upstream `sigSubmissionIngressLimit` — `addMargin` of the
/// decision policy's `txsSizeInflightPerPeer`.
pub fn sig_submission_ingress_limit() -> MiniProtocolLimits {
    MiniProtocolLimits {
        maximum_ingress_queue: add_margin(sig_decision_policy().txs_size_inflight_per_peer),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sig_decision_policy_matches_upstream() {
        let p = sig_decision_policy();
        assert_eq!(p.max_num_tx_ids_to_request, 33);
        assert_eq!(p.max_unacknowledged_tx_ids, 132);
        assert_eq!(p.txs_size_inflight_per_peer, 92_400);
        assert_eq!(p.tx_inflight_multiplicity, 1);
        assert_eq!(p.buffered_txs_min_lifetime_secs, 0);
        assert_eq!(p.score_rate, 0.1);
        assert_eq!(p.score_max, 900.0);
    }

    #[test]
    fn ingress_limit_applies_the_ten_percent_margin() {
        // addMargin(92_400) = 92_400 + 9_240 = 101_640.
        assert_eq!(add_margin(92_400), 101_640);
        assert_eq!(
            sig_submission_ingress_limit().maximum_ingress_queue,
            101_640
        );
    }
}
