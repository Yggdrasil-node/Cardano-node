use yggdrasil_ledger::HeaderHash;

use thiserror::Error;

/// Errors returned by consensus-facing helpers.
#[derive(Debug, Error, Eq, PartialEq)]
pub enum ConsensusError {
    /// The active slot coefficient is outside the valid `[0, 1]` range.
    #[error("invalid active slot coefficient")]
    InvalidActiveSlotCoeff,
    /// A VRF proof was structurally invalid or failed verification.
    #[error("invalid VRF proof")]
    InvalidVrfProof,
    /// The cold-key signature on an operational certificate is invalid.
    #[error("invalid operational certificate signature")]
    InvalidOpCertSignature,
    /// The KES signature on a block header is invalid.
    #[error("invalid KES signature")]
    InvalidKesSignature,
    /// The current KES period is before the certificate's start period.
    #[error("KES period too early: current {current}, cert starts at {cert_start}")]
    KesPeriodTooEarly {
        /// The KES period derived from the current slot.
        current: u64,
        /// The KES period at which the certificate becomes valid.
        cert_start: u64,
    },
    /// The current KES period is at or past the certificate's end period.
    #[error("KES period expired: current {current}, cert ends at {cert_end}")]
    KesPeriodExpired {
        /// The KES period derived from the current slot.
        current: u64,
        /// The exclusive upper bound of the certificate's KES window.
        cert_end: u64,
    },
    /// KES period arithmetic overflowed.
    #[error("KES period overflow")]
    KesPeriodOverflow,
    /// `slots_per_kes_period` was set to zero.
    #[error("invalid slots per KES period (zero)")]
    InvalidSlotsPerKesPeriod,
    /// A rollback was requested that exceeds the security parameter `k`.
    #[error("rollback too deep: requested {requested} blocks, max is {max}")]
    RollbackTooDeep {
        /// The number of blocks requested for rollback.
        requested: u64,
        /// The security parameter `k` (maximum allowed rollback depth).
        max: u64,
    },
    /// The rollback target point was not found in the volatile chain.
    #[error("rollback point not found: slot {slot}, hash {hash:?}")]
    RollbackPointNotFound {
        /// Slot of the requested rollback target.
        slot: u64,
        /// Header hash of the requested rollback target.
        hash: HeaderHash,
    },
    /// A `roll_forward` block number does not follow the current tip.
    #[error("non-contiguous block: expected {expected}, got {got}")]
    NonContiguousBlock {
        /// The block number that was expected.
        expected: u64,
        /// The block number that was received.
        got: u64,
    },
    /// The block's slot is not strictly greater than the previous tip slot.
    ///
    /// Reference: CHAINHEAD slot invariant from
    /// `Ouroboros.Consensus.HeaderValidation`.
    #[error("slot not increasing: tip slot {tip_slot}, block slot {block_slot}")]
    SlotNotIncreasing {
        /// The slot of the current chain tip.
        tip_slot: u64,
        /// The slot of the incoming block.
        block_slot: u64,
    },
    /// The block's prev-hash does not match the current tip's header hash.
    ///
    /// Reference: CHAINHEAD prev-hash invariant from
    /// `Ouroboros.Consensus.HeaderValidation`.
    #[error("prev-hash mismatch: expected {expected:?}, got {got:?}")]
    PrevHashMismatch {
        /// The header hash of the current chain tip.
        expected: HeaderHash,
        /// The prev-hash declared in the incoming block's header.
        got: HeaderHash,
    },
    /// The VRF leader eligibility check failed — the block issuer's VRF
    /// output does not meet the leader threshold for their relative stake.
    #[error("VRF leader eligibility check failed")]
    VrfLeaderCheckFailed,

    /// A block occupied a TPraos overlay slot that is reserved but not active.
    ///
    /// Reference: `NotActiveSlotOVERLAY` in
    /// `Cardano.Protocol.TPraos.Rules.Overlay`.
    #[error("TPraos overlay slot {slot} is not active")]
    TpraosOverlaySlotNotActive {
        /// The reserved overlay slot that must remain silent.
        slot: u64,
    },

    /// A block in an active TPraos overlay slot was not issued by the selected
    /// genesis delegate.
    ///
    /// Reference: `WrongGenesisColdKeyOVERLAY` in
    /// `Cardano.Protocol.TPraos.Rules.Overlay`.
    #[error("wrong genesis delegate cold key: expected {expected:?}, got {actual:?}")]
    WrongGenesisColdKey {
        /// The delegate key hash selected by the overlay schedule.
        expected: [u8; 28],
        /// The block issuer key hash.
        actual: [u8; 28],
    },

    /// The block issuer's pool key hash has no entry in the OpCert counter
    /// map and is also absent from the stake distribution.
    ///
    /// Reference: `NoCounterForKeyHashOCERT` in
    /// `Ouroboros.Consensus.Protocol.Praos`.
    #[error("no OpCert counter for pool key hash {hash:?}")]
    NoCounterForKeyHash {
        /// Blake2b-224 of the issuer cold verification key.
        hash: [u8; 28],
    },

    /// The OpCert sequence number is strictly less than the last recorded
    /// counter for the same pool — the certificate has been superseded.
    ///
    /// Reference: `CounterTooSmallOCERT` in
    /// `Ouroboros.Consensus.Protocol.Praos`.
    #[error("OpCert counter too old: stored {stored}, received {received}")]
    OcertCounterTooOld {
        /// The highest previously-seen sequence number.
        stored: u64,
        /// The sequence number in the current block's OpCert.
        received: u64,
    },

    /// The OpCert sequence number is more than one ahead of the stored
    /// counter — only a single increment per certificate is allowed.
    ///
    /// Reference: `CounterOverIncrementedOCERT` in
    /// `Ouroboros.Consensus.Protocol.Praos`.
    #[error("OpCert counter too far ahead: stored {stored}, received {received}")]
    OcertCounterTooFar {
        /// The highest previously-seen sequence number.
        stored: u64,
        /// The sequence number in the current block's OpCert.
        received: u64,
    },

    /// The VRF key hash in the block header does not match the registered
    /// VRF key hash from the pool parameters in the stake distribution.
    ///
    /// Reference: `doValidateVRFSignature` — `vrfHKStake ≠ vrfHKBlock`
    /// check in `Ouroboros.Consensus.Protocol.Praos`.
    #[error("VRF key mismatch: expected {expected:?}, got {actual:?}")]
    VrfKeyMismatch {
        /// The registered VRF key hash from the pool's `PoolParams`.
        expected: [u8; 32],
        /// The VRF key hash derived from the block header.
        actual: [u8; 32],
    },

    /// The block issuer's pool is not present in the stake distribution,
    /// so no registered VRF key hash is available for cross-checking.
    ///
    /// Reference: `doValidateVRFSignature` — `lookupPoolDistr` failure
    /// in `Ouroboros.Consensus.Protocol.Praos`.
    #[error("VRF key check: pool {pool_hash:?} not in stake distribution")]
    VrfKeyUnknownPool {
        /// Blake2b-224 of the issuer cold verification key.
        pool_hash: [u8; 28],
    },

    /// The block header's major protocol version exceeds the
    /// operator-configured `MaxMajorProtVer`, so this node considers
    /// itself obsolete relative to the block's proposed protocol.
    ///
    /// Reference: `ObsoleteNode` in
    /// `Cardano.Protocol.Praos.Rules.Prtcl.headerView`.
    #[error(
        "obsolete node: header protocol major {header_major} exceeds configured \
         max_major_protocol_version {max_major}"
    )]
    ObsoleteNode {
        /// The major component of the header's protocol version.
        header_major: u64,
        /// The configured accept ceiling.
        max_major: u64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_rollback_too_deep() {
        let e = ConsensusError::RollbackTooDeep {
            requested: 100,
            max: 10,
        };
        let s = format!("{e}");
        assert!(s.contains("100"));
        assert!(s.contains("10"));
    }

    #[test]
    fn display_non_contiguous() {
        let e = ConsensusError::NonContiguousBlock {
            expected: 42,
            got: 99,
        };
        let s = format!("{e}");
        assert!(s.contains("42"));
        assert!(s.contains("99"));
    }

    #[test]
    fn display_kes_period_too_early() {
        let e = ConsensusError::KesPeriodTooEarly {
            current: 5,
            cert_start: 10,
        };
        let s = format!("{e}");
        assert!(s.contains("5"));
        assert!(s.contains("10"));
    }

    #[test]
    fn display_kes_period_expired() {
        let e = ConsensusError::KesPeriodExpired {
            current: 100,
            cert_end: 50,
        };
        let s = format!("{e}");
        assert!(s.contains("100"));
        assert!(s.contains("50"));
    }

    #[test]
    fn display_rollback_point_not_found() {
        let e = ConsensusError::RollbackPointNotFound {
            slot: 42,
            hash: HeaderHash([0xAB; 32]),
        };
        let s = format!("{e}");
        assert!(s.contains("42"));
    }

    #[test]
    fn display_slot_not_increasing() {
        let e = ConsensusError::SlotNotIncreasing {
            tip_slot: 1_000,
            block_slot: 999,
        };
        let s = format!("{e}");
        assert!(s.contains("1000") || s.contains("1_000"));
        assert!(s.contains("999"));
    }

    #[test]
    fn display_prev_hash_mismatch() {
        let e = ConsensusError::PrevHashMismatch {
            expected: HeaderHash([0xAA; 32]),
            got: HeaderHash([0xBB; 32]),
        };
        let s = format!("{e}");
        assert!(s.contains("expected"));
        assert!(s.contains("got"));
    }

    #[test]
    fn display_ocert_counter_too_old() {
        let e = ConsensusError::OcertCounterTooOld {
            stored: 100,
            received: 50,
        };
        let s = format!("{e}");
        // Both values must appear so the operator sees exactly which
        // counter was out-of-order.
        assert!(s.contains("100"));
        assert!(s.contains("50"));
    }

    #[test]
    fn display_ocert_counter_too_far() {
        let e = ConsensusError::OcertCounterTooFar {
            stored: 7,
            received: 20,
        };
        let s = format!("{e}");
        assert!(s.contains('7'));
        assert!(s.contains("20"));
    }

    #[test]
    fn display_obsolete_node() {
        // Slice 43 added this variant — ensure the Display message surfaces
        // both the offending header version and the configured ceiling,
        // matching the "fix one or the other" operator diagnosis pattern.
        let e = ConsensusError::ObsoleteNode {
            header_major: 15,
            max_major: 10,
        };
        let s = format!("{e}");
        assert!(
            s.contains("15"),
            "Display must mention the offending header major: {s}",
        );
        assert!(
            s.contains("10"),
            "Display must mention the configured ceiling: {s}",
        );
        assert!(
            s.to_lowercase().contains("obsolete"),
            "Display must identify the rule by name: {s}",
        );
    }

    #[test]
    fn display_vrf_key_mismatch_names_both_hashes() {
        // `VrfKeyMismatch` uses Debug formatting for the 32-byte hashes;
        // verify the message surfaces at least one distinguishable byte
        // prefix from each so an operator can confirm the mismatch is
        // real rather than a spurious identity compare.
        let e = ConsensusError::VrfKeyMismatch {
            expected: [0xAA; 32],
            actual: [0xBB; 32],
        };
        let s = format!("{e}");
        // Debug format of [u8; 32] produces decimals; 0xAA == 170, 0xBB == 187.
        assert!(s.contains("170"), "must show expected hash byte 0xAA: {s}");
        assert!(s.contains("187"), "must show actual hash byte 0xBB: {s}");
    }

    #[test]
    fn all_variants_are_displayable() {
        let variants: Vec<ConsensusError> = vec![
            ConsensusError::InvalidActiveSlotCoeff,
            ConsensusError::InvalidVrfProof,
            ConsensusError::InvalidOpCertSignature,
            ConsensusError::InvalidKesSignature,
            ConsensusError::KesPeriodOverflow,
            ConsensusError::InvalidSlotsPerKesPeriod,
            ConsensusError::VrfLeaderCheckFailed,
            ConsensusError::NoCounterForKeyHash { hash: [0; 28] },
            ConsensusError::OcertCounterTooOld {
                stored: 5,
                received: 3,
            },
            ConsensusError::OcertCounterTooFar {
                stored: 5,
                received: 10,
            },
            ConsensusError::VrfKeyMismatch {
                expected: [0xAA; 32],
                actual: [0xBB; 32],
            },
            ConsensusError::VrfKeyUnknownPool {
                pool_hash: [0xCC; 28],
            },
            ConsensusError::ObsoleteNode {
                header_major: 99,
                max_major: 10,
            },
        ];
        for v in &variants {
            assert!(!format!("{v}").is_empty());
        }
    }

    #[test]
    fn error_is_eq() {
        assert_eq!(
            ConsensusError::InvalidVrfProof,
            ConsensusError::InvalidVrfProof
        );
        assert_ne!(
            ConsensusError::InvalidVrfProof,
            ConsensusError::InvalidKesSignature
        );
    }
}
