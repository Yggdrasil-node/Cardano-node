//! Diffusion pipelining support (Block Diffusion Pipelining via Delayed
//! Validation — DPvDV).
//!
//! Allows a node to announce block **headers** to downstream peers before
//! the block **body** has been validated, provided a safety criterion is
//! met.  If the body later turns out invalid (a "trap header"), the
//! announcement is rolled back.
//!
//! The criterion prevents an adversary from inducing unbounded work:
//! a header can be pipelined unless we have already pipelined a trap
//! header at the same block number from the same issuer identity.
//!
//! Reference: `Ouroboros.Consensus.Block.SupportsDiffusionPipelining`
//! and `Ouroboros.Consensus.Shelley.Node.DiffusionPipelining`.

pub mod identity;
pub mod state;

pub use identity::{HotIdentity, TentativeHeaderState, TentativeHeaderView};
pub use state::{
    DiffusionPipeliningSupport, PeerPipeliningState, PipeliningEvent, TentativeHeader,
    TentativeState,
};

#[cfg(test)]
mod tests {
    use super::*;
    use yggdrasil_crypto::blake2b::hash_bytes_224;
    use yggdrasil_crypto::ed25519::VerificationKey;
    use yggdrasil_ledger::BlockNo;

    fn identity(issuer: u8, issue_no: u64) -> HotIdentity {
        let mut hash = [0u8; 28];
        hash[0] = issuer;
        HotIdentity::from_parts(hash, issue_no)
    }

    fn view(block_no: u64, issuer: u8, issue_no: u64) -> TentativeHeaderView {
        TentativeHeaderView {
            block_no: BlockNo(block_no),
            identity: identity(issuer, issue_no),
        }
    }

    // -----------------------------------------------------------------------
    // TentativeHeaderState tests
    // -----------------------------------------------------------------------

    #[test]
    fn initial_state_allows_any_header() {
        let state = TentativeHeaderState::initial();
        let v = view(1, 1, 0);
        assert!(state.apply_tentative_header_view(&v).is_some());
    }

    #[test]
    fn same_issuer_same_block_no_rejected_after_trap() {
        let state = TentativeHeaderState::initial();
        let v1 = view(10, 1, 0);
        // First header passes and returns new state for if it became a trap.
        let trap_state = state.apply_tentative_header_view(&v1).unwrap();
        // Same issuer at same block_no → rejected.
        let v2 = view(10, 1, 0);
        assert!(trap_state.apply_tentative_header_view(&v2).is_none());
    }

    #[test]
    fn different_issuer_same_block_no_allowed() {
        let state = TentativeHeaderState::initial();
        let v1 = view(10, 1, 0);
        let trap_state = state.apply_tentative_header_view(&v1).unwrap();
        // Different issuer at same block_no → allowed.
        let v2 = view(10, 2, 0);
        assert!(trap_state.apply_tentative_header_view(&v2).is_some());
    }

    #[test]
    fn same_issuer_higher_issue_no_treated_as_different() {
        let state = TentativeHeaderState::initial();
        let v1 = view(10, 1, 0);
        let trap_state = state.apply_tentative_header_view(&v1).unwrap();
        // Same cold key but higher opcert counter → different HotIdentity.
        let v2 = view(10, 1, 1);
        assert!(trap_state.apply_tentative_header_view(&v2).is_some());
    }

    #[test]
    fn higher_block_no_always_resets() {
        let state = TentativeHeaderState::initial();
        let v1 = view(10, 1, 0);
        let trap_state = state.apply_tentative_header_view(&v1).unwrap();
        // Higher block_no resets — same issuer is allowed again.
        let v2 = view(11, 1, 0);
        let new_state = trap_state.apply_tentative_header_view(&v2).unwrap();
        // The same issuer at block 11 is now the only bad identity.
        assert_eq!(new_state.last_trap_block_no, Some(BlockNo(11)));
        assert_eq!(new_state.bad_identities.len(), 1);
    }

    #[test]
    fn lower_block_no_always_rejected() {
        let state = TentativeHeaderState::initial();
        let v1 = view(10, 1, 0);
        let trap_state = state.apply_tentative_header_view(&v1).unwrap();
        // Lower block_no → always rejected.
        let v2 = view(9, 2, 0);
        assert!(trap_state.apply_tentative_header_view(&v2).is_none());
    }

    #[test]
    fn multiple_issuers_tracked_at_same_block_no() {
        let state = TentativeHeaderState::initial();
        let v1 = view(10, 1, 0);
        let s1 = state.apply_tentative_header_view(&v1).unwrap();
        let v2 = view(10, 2, 0);
        let s2 = s1.apply_tentative_header_view(&v2).unwrap();
        // Both identities 1 and 2 are now "bad" at block 10.
        assert_eq!(s2.bad_identities.len(), 2);
        // A third issuer is still allowed.
        let v3 = view(10, 3, 0);
        assert!(s2.apply_tentative_header_view(&v3).is_some());
        // But issuers 1 and 2 are blocked.
        assert!(s2.apply_tentative_header_view(&view(10, 1, 0)).is_none());
        assert!(s2.apply_tentative_header_view(&view(10, 2, 0)).is_none());
    }

    #[test]
    fn subsequence_consistency() {
        // Per upstream requirement: any subsequence of valid view
        // applications must also be valid.
        let state = TentativeHeaderState::initial();
        let views = vec![
            view(10, 1, 0),
            view(11, 2, 0),
            view(12, 1, 0),
            view(12, 3, 0),
        ];

        // Full sequence.
        let mut s = state.clone();
        for v in &views {
            s = s.apply_tentative_header_view(v).unwrap();
        }

        // Subsequence: skip view[1].
        let mut s2 = state.clone();
        for v in [&views[0], &views[2], &views[3]] {
            s2 = s2.apply_tentative_header_view(v).unwrap();
        }

        // Subsequence: skip view[0] and view[2].
        let mut s3 = state.clone();
        for v in [&views[1], &views[3]] {
            s3 = s3.apply_tentative_header_view(v).unwrap();
        }
    }

    // -----------------------------------------------------------------------
    // TentativeState tests
    // -----------------------------------------------------------------------

    #[test]
    fn tentative_state_initial_has_no_header() {
        let ts = TentativeState::initial();
        assert!(!ts.has_tentative());
        assert!(ts.tentative().is_none());
    }

    #[test]
    fn clear_adopted_removes_header() {
        let mut ts = TentativeState::initial();
        // Simulate setting a tentative header directly.
        ts.tentative_header = Some(TentativeHeader {
            block_no: BlockNo(5),
            slot: yggdrasil_ledger::SlotNo(100),
            header_hash: yggdrasil_ledger::HeaderHash([0xAA; 32]),
            view: view(5, 1, 0),
            raw_header: vec![0xCA, 0xFE],
        });
        assert!(ts.has_tentative());
        let event = ts.clear_adopted().unwrap();
        assert!(!ts.has_tentative());
        assert!(matches!(
            event,
            PipeliningEvent::TentativeHeaderAdopted { .. }
        ));
    }

    #[test]
    fn clear_trap_records_bad_identity() {
        let mut ts = TentativeState::initial();
        let id = identity(1, 0);
        ts.tentative_header = Some(TentativeHeader {
            block_no: BlockNo(5),
            slot: yggdrasil_ledger::SlotNo(100),
            header_hash: yggdrasil_ledger::HeaderHash([0xBB; 32]),
            view: TentativeHeaderView {
                block_no: BlockNo(5),
                identity: id.clone(),
            },
            raw_header: vec![],
        });

        let event = ts.clear_trap().unwrap();
        assert!(matches!(event, PipeliningEvent::TrapTentativeHeader { .. }));
        assert!(!ts.has_tentative());
        // Criterion state updated: issuer 1 at block 5 is now "bad".
        assert!(ts.criterion_state.bad_identities.contains(&id));
    }

    #[test]
    fn clear_on_empty_returns_none() {
        let mut ts = TentativeState::initial();
        assert!(ts.clear_adopted().is_none());
        assert!(ts.clear_trap().is_none());
    }

    // -----------------------------------------------------------------------
    // PeerPipeliningState tests
    // -----------------------------------------------------------------------

    #[test]
    fn peer_state_allows_first_trap() {
        let mut ps = PeerPipeliningState::initial();
        let v = view(10, 1, 0);
        assert!(ps.check_peer_trap(&v));
    }

    #[test]
    fn peer_state_rejects_repeated_bad_identity() {
        let mut ps = PeerPipeliningState::initial();
        let v1 = view(10, 1, 0);
        assert!(ps.check_peer_trap(&v1));
        // Same issuer at same block → peer is misbehaving.
        let v2 = view(10, 1, 0);
        assert!(!ps.check_peer_trap(&v2));
    }

    #[test]
    fn peer_state_allows_different_issuer() {
        let mut ps = PeerPipeliningState::initial();
        assert!(ps.check_peer_trap(&view(10, 1, 0)));
        assert!(ps.check_peer_trap(&view(10, 2, 0)));
    }

    #[test]
    fn peer_state_allows_higher_block_no() {
        let mut ps = PeerPipeliningState::initial();
        assert!(ps.check_peer_trap(&view(10, 1, 0)));
        // Higher block resets — same issuer is OK again.
        assert!(ps.check_peer_trap(&view(11, 1, 0)));
    }

    // -----------------------------------------------------------------------
    // HotIdentity tests
    // -----------------------------------------------------------------------

    #[test]
    fn hot_identity_equality_considers_both_fields() {
        let a = HotIdentity::from_parts([1; 28], 0);
        let b = HotIdentity::from_parts([1; 28], 1);
        let c = HotIdentity::from_parts([2; 28], 0);
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_eq!(a, HotIdentity::from_parts([1; 28], 0));
    }

    #[test]
    fn hot_identity_from_vkey() {
        // Verify that HotIdentity::new hashes the key correctly.
        let vkey_bytes = [0x42u8; 32];
        let vkey = VerificationKey::from_bytes(vkey_bytes);
        let hi = HotIdentity::new(&vkey, 7);
        // The hash should be deterministic Blake2b-224.
        let expected_hash = hash_bytes_224(&vkey_bytes);
        assert_eq!(hi.issuer_hash, expected_hash.0);
        assert_eq!(hi.issue_no, 7);
    }

    // -----------------------------------------------------------------------
    // DiffusionPipeliningSupport tests
    // -----------------------------------------------------------------------

    #[test]
    fn pipelining_support_enum_variants() {
        assert_ne!(
            DiffusionPipeliningSupport::DiffusionPipeliningOff,
            DiffusionPipeliningSupport::DiffusionPipeliningOn,
        );
    }
}
