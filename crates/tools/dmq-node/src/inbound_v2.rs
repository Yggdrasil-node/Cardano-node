//! dmq-node TxSubmission inbound-V2 governor types.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Port of upstream
//! `Ouroboros.Network.TxSubmission.Inbound.V2.Types` — the inbound
//! tx-submission governor's state and decision types. The DMQ
//! `NodeKernel` (`Diffusion/NodeKernel.hs`) holds the inbound-V2
//! `SharedTxState` for `SigSubmission` (`= TxSubmission2 SigId Sig`).
//! dmq-node carries its own copy: the R732 dmq-node-local decision —
//! `crates/consensus`'s tx-submission inbound governor is concrete
//! over ledger transactions, so it cannot be reused for `SigId` /
//! `Sig`.
//!
//! This slice ports the foundational standalone types;
//! `PeerTxState`, `SharedTxState`, the `TxDecision` record, and the
//! governor logic land in subsequent rounds.

use std::time::Duration;

/// Which tx-submission inbound logic a peer connection uses.
///
/// Mirror of upstream `data TxSubmissionLogicVersion`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum TxSubmissionLogicVersion {
    /// The legacy `Inbound.V1` logic.
    TxSubmissionLogicV1,
    /// The new `Inbound.V2` logic.
    TxSubmissionLogicV2,
}

impl TxSubmissionLogicVersion {
    /// Every version, low to high — upstream's `[minBound .. maxBound]`.
    pub const ALL: [TxSubmissionLogicVersion; 2] = [
        TxSubmissionLogicVersion::TxSubmissionLogicV1,
        TxSubmissionLogicVersion::TxSubmissionLogicV2,
    ];
}

/// A count of transactions processed in one governor step — how many
/// were accepted, how many rejected, and the resulting peer score.
///
/// Mirror of upstream `data ProcessedTxCount`. `PartialEq` only —
/// upstream derives `Eq` but `ptxc_score` is `f64`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProcessedTxCount {
    /// How many transactions were just accepted.
    pub ptxc_accepted: i64,
    /// How many transactions were just rejected.
    pub ptxc_rejected: i64,
    /// The peer's resulting score.
    pub ptxc_score: f64,
}

/// An optional delay before tx-submission starts on a connection.
///
/// Mirror of upstream `data TxSubmissionInitDelay`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TxSubmissionInitDelay {
    /// Delay tx-submission start by this duration.
    TxSubmissionInitDelay(Duration),
    /// Start tx-submission with no delay.
    NoTxSubmissionInitDelay,
}

/// The default tx-submission init delay — 60 seconds.
///
/// Mirror of upstream `defaultTxSubmissionInitDelay`.
pub const DEFAULT_TX_SUBMISSION_INIT_DELAY: TxSubmissionInitDelay =
    TxSubmissionInitDelay::TxSubmissionInitDelay(Duration::from_secs(60));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logic_version_all_is_ordered() {
        assert_eq!(
            TxSubmissionLogicVersion::ALL,
            [
                TxSubmissionLogicVersion::TxSubmissionLogicV1,
                TxSubmissionLogicVersion::TxSubmissionLogicV2,
            ]
        );
        assert!(
            TxSubmissionLogicVersion::TxSubmissionLogicV1
                < TxSubmissionLogicVersion::TxSubmissionLogicV2
        );
    }

    #[test]
    fn processed_tx_count_construct() {
        let c = ProcessedTxCount {
            ptxc_accepted: 7,
            ptxc_rejected: 2,
            ptxc_score: 0.5,
        };
        assert_eq!(c.ptxc_accepted, 7);
        assert_eq!(c.ptxc_rejected, 2);
        assert_eq!(c.ptxc_score, 0.5);
    }

    #[test]
    fn default_init_delay_is_sixty_seconds() {
        assert_eq!(
            DEFAULT_TX_SUBMISSION_INIT_DELAY,
            TxSubmissionInitDelay::TxSubmissionInitDelay(Duration::from_secs(60))
        );
        assert_ne!(
            DEFAULT_TX_SUBMISSION_INIT_DELAY,
            TxSubmissionInitDelay::NoTxSubmissionInitDelay
        );
    }
}
