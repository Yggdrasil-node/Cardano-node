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

use std::collections::BTreeSet;

use yggdrasil_crypto::blake2b::hash_bytes_224;
use yggdrasil_crypto::ed25519::VerificationKey;
use yggdrasil_ledger::BlockNo;

use crate::header::HeaderBody;

// ---------------------------------------------------------------------------
// HotIdentity — issuer identity for pipelining decisions
// ---------------------------------------------------------------------------

/// Hot block-issuer identity for diffusion pipelining.
///
/// Uniquely identifies a block issuer using the hash of their cold key and
/// the operational certificate sequence number.  Even if an operational
/// certificate is compromised, the legitimate owner can issue a new one
/// with a higher counter, so their blocks will still be pipelineable.
///
/// Reference: `HotIdentity` in
/// `Ouroboros.Consensus.Shelley.Node.DiffusionPipelining`.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct HotIdentity {
    /// Blake2b-224 hash of the cold (block issuer) verification key.
    ///
    /// Upstream: `hiIssuer :: KeyHash BlockIssuer`.
    pub issuer_hash: [u8; 28],
    /// Operational certificate sequence number.
    ///
    /// Upstream: `hiIssueNo :: Word64`.
    pub issue_no: u64,
}

impl HotIdentity {
    /// Construct a `HotIdentity` from a cold verification key and an
    /// operational certificate sequence number.
    pub fn new(cold_vkey: &VerificationKey, opcert_sequence: u64) -> Self {
        let hash = hash_bytes_224(&cold_vkey.to_bytes());
        Self {
            issuer_hash: hash.0,
            issue_no: opcert_sequence,
        }
    }

    /// Construct a `HotIdentity` from a pre-computed issuer key hash and
    /// sequence number (useful when the hash is already available from a
    /// decoded header).
    pub fn from_parts(issuer_hash: [u8; 28], issue_no: u64) -> Self {
        Self {
            issuer_hash,
            issue_no,
        }
    }
}

// ---------------------------------------------------------------------------
// TentativeHeaderView — extracted per-header data for criterion check
// ---------------------------------------------------------------------------

/// View on a block header used to evaluate the pipelining criterion.
///
/// Reference: `ShelleyTentativeHeaderView` in
/// `Ouroboros.Consensus.Shelley.Node.DiffusionPipelining`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TentativeHeaderView {
    /// Block number of the header.
    pub block_no: BlockNo,
    /// Hot identity of the block issuer.
    pub identity: HotIdentity,
}

impl TentativeHeaderView {
    /// Extract a [`TentativeHeaderView`] from a consensus [`HeaderBody`].
    ///
    /// Upstream: `tentativeHeaderView` applied to a `ShelleyHeader`.
    pub fn from_header_body(hb: &HeaderBody) -> Self {
        Self {
            block_no: hb.block_number,
            identity: HotIdentity::new(
                &hb.issuer_vkey,
                hb.operational_cert.sequence_number,
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// TentativeHeaderState — stateful criterion tracking
// ---------------------------------------------------------------------------

/// State maintained to judge whether a header may be pipelined.
///
/// Records the block number of the most recent trap header and the set of
/// hot identities that produced trap headers at that block number.
///
/// Maintained in two contexts:
/// - **ChainSel**: tracks trap headers *we* announced to downstream peers.
/// - **Per-upstream-peer**: tracks trap headers a given peer sent to us.
///
/// Reference: `ShelleyTentativeHeaderState` in
/// `Ouroboros.Consensus.Shelley.Node.DiffusionPipelining`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TentativeHeaderState {
    /// Block number of the latest trap tentative header, or `None` if no
    /// trap header has been recorded.
    ///
    /// Upstream: `WithOrigin BlockNo` (first field).
    last_trap_block_no: Option<BlockNo>,
    /// Hot identities of issuers who produced trap headers at
    /// `last_trap_block_no`.
    ///
    /// Upstream: `Set (HotIdentity c)` (second field).
    bad_identities: BTreeSet<HotIdentity>,
}

impl TentativeHeaderState {
    /// Initial state: no trap headers recorded.
    ///
    /// Upstream: `initialTentativeHeaderState`.
    pub fn initial() -> Self {
        Self {
            last_trap_block_no: None,
            bad_identities: BTreeSet::new(),
        }
    }

    /// The block number of the last recorded trap header, if any.
    pub fn last_trap_block_no(&self) -> Option<BlockNo> {
        self.last_trap_block_no
    }

    /// The set of bad identities at the current trap block number.
    pub fn bad_identities(&self) -> &BTreeSet<HotIdentity> {
        &self.bad_identities
    }

    /// Evaluate the diffusion pipelining criterion for a header view.
    ///
    /// Returns `Some(new_state)` if the header **may** be pipelined.  The
    /// returned state is the state that should be adopted **if** the header
    /// later turns out to be a trap (body invalid).
    ///
    /// Returns `None` if the header **must not** be pipelined because it
    /// violates the criterion (same block number as the last trap, same
    /// issuer identity).
    ///
    /// # Criterion
    ///
    /// A header can be pipelined iff:
    /// - Its `block_no > last_trap_block_no` (strictly greater), **or**
    /// - `block_no == last_trap_block_no` and its issuer identity is
    ///   **not** in the bad-identity set.
    ///
    /// Reference: `applyTentativeHeaderView` in
    /// `Ouroboros.Consensus.Shelley.Node.DiffusionPipelining`.
    pub fn apply_tentative_header_view(
        &self,
        view: &TentativeHeaderView,
    ) -> Option<TentativeHeaderState> {
        match self.last_trap_block_no {
            Some(last_bno) => {
                match view.block_no.0.cmp(&last_bno.0) {
                    // Header is behind the last trap — reject.
                    std::cmp::Ordering::Less => None,
                    // Same block number — only allow if different identity.
                    std::cmp::Ordering::Equal => {
                        if self.bad_identities.contains(&view.identity) {
                            None
                        } else {
                            let mut new_bad = self.bad_identities.clone();
                            new_bad.insert(view.identity.clone());
                            Some(TentativeHeaderState {
                                last_trap_block_no: self.last_trap_block_no,
                                bad_identities: new_bad,
                            })
                        }
                    }
                    // Higher block number — always allow, reset bad set.
                    std::cmp::Ordering::Greater => {
                        let mut new_bad = BTreeSet::new();
                        new_bad.insert(view.identity.clone());
                        Some(TentativeHeaderState {
                            last_trap_block_no: Some(view.block_no),
                            bad_identities: new_bad,
                        })
                    }
                }
            }
            // No prior traps — always allow.
            None => {
                let mut new_bad = BTreeSet::new();
                new_bad.insert(view.identity.clone());
                Some(TentativeHeaderState {
                    last_trap_block_no: Some(view.block_no),
                    bad_identities: new_bad,
                })
            }
        }
    }

    /// Convenience: extract a [`TentativeHeaderView`] from a
    /// [`HeaderBody`] and apply the criterion in one step.
    ///
    /// Upstream: `updateTentativeHeaderState`.
    pub fn update_with_header_body(
        &self,
        hb: &HeaderBody,
    ) -> Option<TentativeHeaderState> {
        let view = TentativeHeaderView::from_header_body(hb);
        self.apply_tentative_header_view(&view)
    }
}

impl Default for TentativeHeaderState {
    fn default() -> Self {
        Self::initial()
    }
}

// ---------------------------------------------------------------------------
// DiffusionPipeliningSupport — feature gate
// ---------------------------------------------------------------------------

/// Whether diffusion pipelining is enabled for this node.
///
/// Reference: `DiffusionPipeliningSupport` in
/// `Ouroboros.Consensus.Node.ProtocolInfo`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiffusionPipeliningSupport {
    /// Pipelining disabled: ChainSync servers only serve the confirmed
    /// chain selection (no tentative tip).
    DiffusionPipeliningOff,
    /// Pipelining enabled: ChainSync servers may serve a tentative header
    /// at the tip before body validation completes.
    DiffusionPipeliningOn,
}

// ---------------------------------------------------------------------------
// TentativeState — combined tentative header + criterion state for ChainDb
// ---------------------------------------------------------------------------

/// Combined tentative header state maintained by ChainDb (or equivalent).
///
/// Holds the currently tentative header (if any) alongside the criterion
/// state.  The tentative header extends the current confirmed chain
/// selection and is served to ChainSync followers when pipelining is
/// enabled.
///
/// Invariants:
/// - When `tentative_header` is `Some`, the header fits on top of the
///   current confirmed chain tip and its body has not yet been validated.
/// - When `tentative_header` is `None`, the confirmed chain tip is the
///   authoritative tip for all followers.
///
/// Reference: `cdbTentativeState` + `cdbTentativeHeader` in
/// `Ouroboros.Consensus.Storage.ChainDB.Impl.Types`.
#[derive(Clone, Debug)]
pub struct TentativeState {
    /// The pipelining criterion state.
    pub criterion_state: TentativeHeaderState,
    /// The currently tentative header, if any.
    ///
    /// Stored as:
    /// - `block_no` and `slot` for identification
    /// - `header_hash` for point construction
    /// - `raw_header` so ChainSync servers can send it without re-encoding
    pub tentative_header: Option<TentativeHeader>,
}

/// A tentative header pending body validation.
#[derive(Clone, Debug)]
pub struct TentativeHeader {
    /// Block number of the tentative header.
    pub block_no: BlockNo,
    /// Slot of the tentative header.
    pub slot: yggdrasil_ledger::SlotNo,
    /// Header hash (for point construction and follower comparison).
    pub header_hash: yggdrasil_ledger::HeaderHash,
    /// The `TentativeHeaderView` extracted at pipelining time, used to
    /// update `TentativeHeaderState` if this becomes a trap header.
    pub view: TentativeHeaderView,
    /// Raw CBOR-encoded header bytes for ChainSync serving.
    pub raw_header: Vec<u8>,
}

/// Trace events emitted by the diffusion pipelining subsystem.
///
/// Reference: `TracePipeliningEvent` in
/// `Ouroboros.Consensus.Storage.ChainDB.Impl.Types`.
#[derive(Clone, Debug)]
pub enum PipeliningEvent {
    /// A tentative header was set (announced before body validation).
    ///
    /// Upstream: `SetTentativeHeader`.
    SetTentativeHeader {
        /// Block number of the tentative header.
        block_no: BlockNo,
        /// Slot of the tentative header.
        slot: yggdrasil_ledger::SlotNo,
        /// Header hash.
        header_hash: yggdrasil_ledger::HeaderHash,
    },
    /// A previously tentative header is no longer needed because the body
    /// was valid and the block is now part of the confirmed selection.
    ///
    /// Upstream: `OutdatedTentativeHeader` (cleared because adopted normally).
    TentativeHeaderAdopted {
        block_no: BlockNo,
        slot: yggdrasil_ledger::SlotNo,
        header_hash: yggdrasil_ledger::HeaderHash,
    },
    /// A tentative header's body turned out to be invalid (trap header).
    /// The announcement is rolled back.
    ///
    /// Upstream: `TrapTentativeHeader`.
    TrapTentativeHeader {
        block_no: BlockNo,
        slot: yggdrasil_ledger::SlotNo,
        header_hash: yggdrasil_ledger::HeaderHash,
    },
}

impl TentativeState {
    /// Create initial tentative state with no tentative header and clean
    /// criterion state.
    pub fn initial() -> Self {
        Self {
            criterion_state: TentativeHeaderState::initial(),
            tentative_header: None,
        }
    }

    /// Try to set a tentative header.
    ///
    /// Returns `Some(PipeliningEvent::SetTentativeHeader)` if the header
    /// passes the pipelining criterion and was set as tentative.  Returns
    /// `None` if the criterion was not met (header should not be pipelined).
    ///
    /// The caller should announce the header to ChainSync followers on
    /// `Some`.
    pub fn try_set_tentative(
        &mut self,
        header_body: &HeaderBody,
        slot: yggdrasil_ledger::SlotNo,
        header_hash: yggdrasil_ledger::HeaderHash,
        raw_header: Vec<u8>,
    ) -> Option<PipeliningEvent> {
        let view = TentativeHeaderView::from_header_body(header_body);
        // Check criterion — but don't update state yet.
        // State is only updated if this becomes a trap (body invalid).
        let _trap_state = self
            .criterion_state
            .apply_tentative_header_view(&view)?;

        let block_no = header_body.block_number;
        self.tentative_header = Some(TentativeHeader {
            block_no,
            slot,
            header_hash,
            view,
            raw_header,
        });

        Some(PipeliningEvent::SetTentativeHeader {
            block_no,
            slot,
            header_hash,
        })
    }

    /// Clear the tentative header because the body was valid and the block
    /// is now part of the confirmed chain selection.
    ///
    /// Returns the trace event, or `None` if there was no tentative header.
    pub fn clear_adopted(&mut self) -> Option<PipeliningEvent> {
        let th = self.tentative_header.take()?;
        Some(PipeliningEvent::TentativeHeaderAdopted {
            block_no: th.block_no,
            slot: th.slot,
            header_hash: th.header_hash,
        })
    }

    /// Clear the tentative header because the body was **invalid** (trap
    /// header).
    ///
    /// Updates the criterion state to record this trap, which may prevent
    /// future headers from the same issuer at the same block number from
    /// being pipelined.
    ///
    /// Returns the trace event, or `None` if there was no tentative header.
    pub fn clear_trap(&mut self) -> Option<PipeliningEvent> {
        let th = self.tentative_header.take()?;

        // Apply the view to record the trap in criterion state.
        if let Some(new_state) = self
            .criterion_state
            .apply_tentative_header_view(&th.view)
        {
            self.criterion_state = new_state;
        }

        Some(PipeliningEvent::TrapTentativeHeader {
            block_no: th.block_no,
            slot: th.slot,
            header_hash: th.header_hash,
        })
    }

    /// Whether there is currently a tentative header.
    pub fn has_tentative(&self) -> bool {
        self.tentative_header.is_some()
    }

    /// Access the current tentative header, if any.
    pub fn tentative(&self) -> Option<&TentativeHeader> {
        self.tentative_header.as_ref()
    }
}

impl Default for TentativeState {
    fn default() -> Self {
        Self::initial()
    }
}

// ---------------------------------------------------------------------------
// Per-peer pipelining validation
// ---------------------------------------------------------------------------

/// Per-upstream-peer pipelining state, used by BlockFetch clients to
/// validate that a peer adheres to the pipelining criterion.
///
/// Each BlockFetch upstream connection maintains its own
/// `TentativeHeaderState`.  When a block received from the peer has an
/// invalid body, we check whether the peer's behavior is consistent with
/// the criterion.  If not, we disconnect.
///
/// Reference: BlockFetch punishment logic in
/// `Ouroboros.Consensus.MiniProtocol.BlockFetch.ClientInterface`.
#[derive(Clone, Debug)]
pub struct PeerPipeliningState {
    state: TentativeHeaderState,
}

impl PeerPipeliningState {
    /// Create initial per-peer state.
    pub fn initial() -> Self {
        Self {
            state: TentativeHeaderState::initial(),
        }
    }

    /// Check whether a peer's invalid block (trap header) is consistent
    /// with the pipelining criterion.
    ///
    /// Returns `true` if the peer's behavior is allowed and we should
    /// record the trap in the per-peer state.
    ///
    /// Returns `false` if the peer violated the criterion and should be
    /// disconnected.
    pub fn check_peer_trap(&mut self, view: &TentativeHeaderView) -> bool {
        match self.state.apply_tentative_header_view(view) {
            Some(new_state) => {
                self.state = new_state;
                true
            }
            None => false,
        }
    }
}

impl Default for PeerPipeliningState {
    fn default() -> Self {
        Self::initial()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert!(matches!(
            event,
            PipeliningEvent::TrapTentativeHeader { .. }
        ));
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
