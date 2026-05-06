//! `GovernanceActionState` — stored Conway governance-action state.
//!
//! Mirrors upstream
//! [`Cardano.Ledger.Conway.Governance::GovActionState`](https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/conway/impl/src/Cardano/Ledger/Conway/Governance.hs)
//! reduced to the fields yggdrasil currently inspects: the submitted
//! `ProposalProcedure`, votes keyed by `Voter`, and the optional
//! proposed/expires epoch lifetime.
//!
//! Used by the RATIFY rule (`state/ratify.rs`) to tally CC / DRep / SPO
//! votes against per-action thresholds, and by the EPOCH rule
//! (`epoch_boundary.rs`) to expire actions whose `expires_after` epoch has
//! passed.
//!
//! Extracted from `state.rs` in R269 tenth slice as part of the strict 1:1
//! filename-mirror refactor — see
//! `docs/operational-runs/2026-05-06-round-269j-state-governance-action-state-extraction.md`.

use super::{decode_optional_epoch_no, encode_optional_epoch_no};
use crate::types::EpochNo;
use crate::{CborDecode, CborEncode, Decoder, Encoder, LedgerError};
use std::collections::BTreeMap;

/// Stored Conway governance action state visible from the ledger.
///
/// This is a reduced local analogue of the upstream Conway `GovActionState`.
/// It preserves the submitted proposal body plus the currently recorded votes
/// keyed by Conway `Voter`, which is enough for proposal lookup and vote
/// replacement semantics in this ledger slice.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GovernanceActionState {
    pub(super) proposal: crate::eras::conway::ProposalProcedure,
    pub(super) votes: BTreeMap<crate::eras::conway::Voter, crate::eras::conway::Vote>,
    pub(super) proposed_in: Option<EpochNo>,
    pub(super) expires_after: Option<EpochNo>,
}

impl CborEncode for GovernanceActionState {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(4);
        self.proposal.encode_cbor(enc);
        enc.map(self.votes.len() as u64);
        for (voter, vote) in &self.votes {
            voter.encode_cbor(enc);
            vote.encode_cbor(enc);
        }
        encode_optional_epoch_no(self.proposed_in, enc);
        encode_optional_epoch_no(self.expires_after, enc);
    }
}

impl CborDecode for GovernanceActionState {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 2 && len != 4 {
            return Err(LedgerError::CborInvalidLength {
                expected: 4,
                actual: len as usize,
            });
        }

        let proposal = crate::eras::conway::ProposalProcedure::decode_cbor(dec)?;
        let votes_len = dec.map()?;
        let mut votes = BTreeMap::new();
        for _ in 0..votes_len {
            let voter = crate::eras::conway::Voter::decode_cbor(dec)?;
            let vote = crate::eras::conway::Vote::decode_cbor(dec)?;
            votes.insert(voter, vote);
        }

        let proposed_in = if len == 4 {
            decode_optional_epoch_no(dec)?
        } else {
            None
        };
        let expires_after = if len == 4 {
            decode_optional_epoch_no(dec)?
        } else {
            None
        };

        Ok(Self {
            proposal,
            votes,
            proposed_in,
            expires_after,
        })
    }
}

impl GovernanceActionState {
    /// Creates stored governance action state for a newly submitted proposal.
    pub fn new(proposal: crate::eras::conway::ProposalProcedure) -> Self {
        Self {
            proposal,
            votes: BTreeMap::new(),
            proposed_in: None,
            expires_after: None,
        }
    }

    pub(crate) fn new_with_lifetime(
        proposal: crate::eras::conway::ProposalProcedure,
        proposed_in: EpochNo,
        gov_action_lifetime: Option<u64>,
    ) -> Self {
        Self {
            proposal,
            votes: BTreeMap::new(),
            proposed_in: Some(proposed_in),
            expires_after: gov_action_lifetime
                .map(|lifetime| EpochNo(proposed_in.0.saturating_add(lifetime))),
        }
    }

    /// Returns the submitted proposal procedure.
    pub fn proposal(&self) -> &crate::eras::conway::ProposalProcedure {
        &self.proposal
    }

    /// Returns the recorded votes keyed by voter.
    pub fn votes(&self) -> &BTreeMap<crate::eras::conway::Voter, crate::eras::conway::Vote> {
        &self.votes
    }

    /// Returns the epoch in which the proposal was introduced, when tracked.
    pub fn proposed_in(&self) -> Option<EpochNo> {
        self.proposed_in
    }

    /// Returns the last epoch in which votes are accepted, when tracked.
    pub fn expires_after(&self) -> Option<EpochNo> {
        self.expires_after
    }

    /// Records a vote from `voter`, replacing any previous vote.
    pub fn record_vote(
        &mut self,
        voter: crate::eras::conway::Voter,
        vote: crate::eras::conway::Vote,
    ) {
        self.votes.insert(voter, vote);
    }
}
