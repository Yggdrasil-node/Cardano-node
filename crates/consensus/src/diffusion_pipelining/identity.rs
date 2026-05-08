//! Per-pool tentative-header tracking for diffusion pipelining.
//!
//! Mirrors upstream `Ouroboros.Consensus.Shelley.Node.DiffusionPipelining`
//! — `HotIdentity`, `TentativeHeaderView`, and the per-pool
//! `TentativeHeaderState` ring used to detect "trap headers" (the same
//! issuer pipelining a header at the same block number twice).
//!
//! Three public types:
//!
//! - `HotIdentity` — block issuer identity (cold-key hash + opcert seq).
//! - `TentativeHeaderView` — projection of the per-block-number state
//!   used by the safety criterion.
//! - `TentativeHeaderState` — the per-pool ring tracking which block
//!   numbers have been pipelined (and which were trap headers).
//!
//! Extracted from `diffusion_pipelining.rs` in R273f (Phase γ §R273
//! sixth slice).

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
            identity: HotIdentity::new(&hb.issuer_vkey, hb.operational_cert.sequence_number),
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
    pub(super) last_trap_block_no: Option<BlockNo>,
    /// Hot identities of issuers who produced trap headers at
    /// `last_trap_block_no`.
    ///
    /// Upstream: `Set (HotIdentity c)` (second field).
    pub(super) bad_identities: BTreeSet<HotIdentity>,
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
    pub fn update_with_header_body(&self, hb: &HeaderBody) -> Option<TentativeHeaderState> {
        let view = TentativeHeaderView::from_header_body(hb);
        self.apply_tentative_header_view(&view)
    }
}

impl Default for TentativeHeaderState {
    fn default() -> Self {
        Self::initial()
    }
}
