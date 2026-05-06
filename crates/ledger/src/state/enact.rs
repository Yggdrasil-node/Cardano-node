//! Conway ENACT rule — applies a ratified governance action.
//!
//! Mirrors upstream
//! [`Cardano.Ledger.Conway.Rules.Enact`](https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/Enact.hs)
//! and the `EnactState` record from
//! [`Cardano.Ledger.Conway.Governance`](https://github.com/IntersectMBO/cardano-ledger/blob/master/eras/conway/impl/src/Cardano/Ledger/Conway/Governance.hs).
//!
//! The [`EnactState`] record carries the currently-enacted governance lineage
//! (constitution, committee quorum, last-enacted action IDs per purpose group).
//! [`enact_gov_action`] is the entry point: it dispatches a single
//! [`crate::eras::conway::GovAction`] variant to the right state-mutation,
//! returning an [`EnactOutcome`] for tracing.
//!
//! Extracted from `state.rs` in R269 third slice as part of the strict 1:1
//! filename-mirror refactor — see `docs/operational-runs/2026-05-06-round-269c-state-enact-extraction.md`.

use super::{
    AccountingState, CommitteeState, ConwayGovActionPurpose, RewardAccounts,
    decode_optional_gov_action_id, encode_optional_gov_action_id,
};
use crate::types::{EpochNo, UnitInterval};
use crate::{CborDecode, CborEncode, Decoder, Encoder, LedgerError};

/// Enacted governance state tracking the current constitution, committee
/// quorum, and the most recently enacted action ID per governance purpose
/// group.
///
/// Upstream reference: `Cardano.Ledger.Conway.Governance.EnactState`.
///
/// The purpose groups mirror the upstream `GovRelation`:
/// * **PParamUpdate** — `ParameterChange` actions.
/// * **HardFork** — `HardForkInitiation` actions.
/// * **Committee** — `NoConfidence` and `UpdateCommittee` actions.
/// * **Constitution** — `NewConstitution` actions.
///
/// `TreasuryWithdrawals` and `InfoAction` have no lineage tracking.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnactState {
    /// The current enacted constitution.
    pub constitution: crate::eras::conway::Constitution,
    /// Committee quorum threshold (ratio of yes-votes needed).
    pub committee_quorum: UnitInterval,
    /// Whether a committee currently exists.
    ///
    /// After `NoConfidence`, upstream sets `ensCommitteeL = SNothing`,
    /// causing `committeeAccepted` to return `False` for all
    /// committee-requiring actions.  `UpdateCommittee` re-establishes
    /// the committee (`SJust`).
    pub has_committee: bool,
    /// Most recently enacted `ParameterChange` action ID.
    pub prev_pparams_update: Option<crate::eras::conway::GovActionId>,
    /// Most recently enacted `HardForkInitiation` action ID.
    pub prev_hard_fork: Option<crate::eras::conway::GovActionId>,
    /// Most recently enacted `NoConfidence` or `UpdateCommittee` action ID.
    pub prev_committee: Option<crate::eras::conway::GovActionId>,
    /// Most recently enacted `NewConstitution` action ID.
    pub prev_constitution: Option<crate::eras::conway::GovActionId>,
}

impl Default for EnactState {
    fn default() -> Self {
        Self {
            constitution: crate::eras::conway::Constitution {
                anchor: crate::types::Anchor {
                    url: String::new(),
                    data_hash: [0u8; 32],
                },
                guardrails_script_hash: None,
            },
            committee_quorum: UnitInterval {
                numerator: 0,
                denominator: 1,
            },
            has_committee: true,
            prev_pparams_update: None,
            prev_hard_fork: None,
            prev_committee: None,
            prev_constitution: None,
        }
    }
}

impl CborEncode for EnactState {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(7);
        self.constitution.encode_cbor(enc);
        self.committee_quorum.encode_cbor(enc);
        enc.bool(self.has_committee);
        encode_optional_gov_action_id(self.prev_pparams_update.as_ref(), enc);
        encode_optional_gov_action_id(self.prev_hard_fork.as_ref(), enc);
        encode_optional_gov_action_id(self.prev_committee.as_ref(), enc);
        encode_optional_gov_action_id(self.prev_constitution.as_ref(), enc);
    }
}

impl CborDecode for EnactState {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 6 && len != 7 {
            return Err(LedgerError::CborInvalidLength {
                expected: 7,
                actual: len as usize,
            });
        }
        let constitution = crate::eras::conway::Constitution::decode_cbor(dec)?;
        let committee_quorum = UnitInterval::decode_cbor(dec)?;
        let has_committee = if len >= 7 { dec.bool()? } else { true };
        let prev_pparams_update = decode_optional_gov_action_id(dec)?;
        let prev_hard_fork = decode_optional_gov_action_id(dec)?;
        let prev_committee = decode_optional_gov_action_id(dec)?;
        let prev_constitution = decode_optional_gov_action_id(dec)?;
        Ok(Self {
            constitution,
            committee_quorum,
            has_committee,
            prev_pparams_update,
            prev_hard_fork,
            prev_committee,
            prev_constitution,
        })
    }
}

impl EnactState {
    /// Creates a default `EnactState` with empty constitution and no lineage.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the currently enacted constitution.
    pub fn constitution(&self) -> &crate::eras::conway::Constitution {
        &self.constitution
    }

    /// Returns the current committee quorum threshold.
    pub fn committee_quorum(&self) -> &UnitInterval {
        &self.committee_quorum
    }

    /// Returns the most recently enacted action ID for each purpose group.
    pub fn prev_pparams_update(&self) -> Option<&crate::eras::conway::GovActionId> {
        self.prev_pparams_update.as_ref()
    }

    pub fn prev_hard_fork(&self) -> Option<&crate::eras::conway::GovActionId> {
        self.prev_hard_fork.as_ref()
    }

    pub fn prev_committee(&self) -> Option<&crate::eras::conway::GovActionId> {
        self.prev_committee.as_ref()
    }

    pub fn prev_constitution(&self) -> Option<&crate::eras::conway::GovActionId> {
        self.prev_constitution.as_ref()
    }

    /// Returns the enacted root for the given governance purpose group.
    ///
    /// This is used during Conway proposal validation to check whether a
    /// proposal's `prev_action_id` correctly references the most recently
    /// enacted action of its purpose family.
    ///
    /// Upstream reference: `Cardano.Ledger.Conway.Governance.prevGovActionIds`.
    pub(crate) fn enacted_root(
        &self,
        purpose: ConwayGovActionPurpose,
    ) -> Option<&crate::eras::conway::GovActionId> {
        match purpose {
            ConwayGovActionPurpose::ParameterChange => self.prev_pparams_update.as_ref(),
            ConwayGovActionPurpose::HardFork => self.prev_hard_fork.as_ref(),
            ConwayGovActionPurpose::Committee => self.prev_committee.as_ref(),
            ConwayGovActionPurpose::Constitution => self.prev_constitution.as_ref(),
            // TreasuryWithdrawals and Info have no lineage.
            ConwayGovActionPurpose::TreasuryWithdrawals | ConwayGovActionPurpose::Info => None,
        }
    }
}

/// Outcome of enacting a single governance action.
///
/// Callers inspect this to determine what side-effects to apply to
/// `LedgerState` (committee, treasury, protocol params, etc.).
///
/// Upstream reference: `Cardano.Ledger.Conway.Rules.Enact`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EnactOutcome {
    /// No on-chain effect (InfoAction).
    NoEffect,
    /// The constitution was updated.
    ConstitutionUpdated,
    /// All committee members were removed (no-confidence motion).
    CommitteeRemoved,
    /// Committee membership was updated and quorum changed.
    CommitteeUpdated {
        members_removed: usize,
        members_added: usize,
    },
    /// A hard fork was enacted — the protocol version was updated.
    HardForkEnacted { new_version: (u64, u64) },
    /// Treasury withdrawals were enacted — lovelace credited to reward
    /// accounts from the treasury.
    TreasuryWithdrawn { total_withdrawn: u64 },
    /// A parameter change was enacted and applied to protocol parameters.
    ParameterChangeRecorded,
}

/// Enacts a single ratified governance action, updating the `EnactState`
/// lineage and applying side-effects to the mutable ledger components.
///
/// This function implements the Conway `ENACT` rule for each governance
/// action variant. Side-effects are applied directly to the provided
/// mutable references so callers do not need to interpret the outcome
/// for state updates — the `EnactOutcome` is purely informational.
///
/// # Parameters
///
/// * `enact` — Enacted governance state (constitution, quorum, lineage).
/// * `action_id` — The `GovActionId` of the action being enacted.
/// * `action` — The `GovAction` body to enact.
/// * `committee` — Mutable committee-member state.
/// * `protocol_params` — Mutable protocol parameters (for hard-fork version).
/// * `reward_accounts` — Mutable reward-account balances (for treasury withdrawal).
/// * `accounting` — Mutable treasury/reserves accounting.
///
/// Upstream reference: `Cardano.Ledger.Conway.Rules.Enact`.
pub fn enact_gov_action(
    enact: &mut EnactState,
    action_id: crate::eras::conway::GovActionId,
    action: &crate::eras::conway::GovAction,
    committee: &mut CommitteeState,
    protocol_params: &mut Option<crate::protocol_params::ProtocolParameters>,
    reward_accounts: &mut RewardAccounts,
    accounting: &mut AccountingState,
) -> EnactOutcome {
    enact_gov_action_at_epoch(
        enact,
        EpochNo(0),
        action_id,
        action,
        committee,
        protocol_params,
        reward_accounts,
        accounting,
    )
}

pub(super) fn enact_gov_action_at_epoch(
    enact: &mut EnactState,
    _current_epoch: EpochNo,
    action_id: crate::eras::conway::GovActionId,
    action: &crate::eras::conway::GovAction,
    committee: &mut CommitteeState,
    protocol_params: &mut Option<crate::protocol_params::ProtocolParameters>,
    reward_accounts: &mut RewardAccounts,
    accounting: &mut AccountingState,
) -> EnactOutcome {
    use crate::eras::conway::GovAction;

    match action {
        GovAction::InfoAction => EnactOutcome::NoEffect,

        GovAction::NewConstitution { constitution, .. } => {
            enact.constitution = constitution.clone();
            enact.prev_constitution = Some(action_id);
            EnactOutcome::ConstitutionUpdated
        }

        GovAction::NoConfidence { .. } => {
            // Upstream sets `ensCommittee = SNothing` which removes all
            // members from committeeMembers, but csCommitteeCreds (authorization
            // and resignation state) is preserved in VState.
            // In our combined model, we clear expires_at (membership) while
            // preserving authorization/resignation state.
            let count = committee.len();
            committee.clear_all_membership();
            enact.committee_quorum = UnitInterval {
                numerator: 0,
                denominator: 1,
            };
            enact.has_committee = false;
            enact.prev_committee = Some(action_id);
            let _ = count; // suppress unused; count is informational
            EnactOutcome::CommitteeRemoved
        }

        GovAction::UpdateCommittee {
            members_to_remove,
            members_to_add,
            quorum,
            ..
        } => {
            let mut removed = 0usize;
            for cred in members_to_remove {
                // Upstream: removes from committeeMembers only — does not
                // touch csCommitteeCreds (authorization/resignation state).
                if committee
                    .get(cred)
                    .is_some_and(|m| m.expires_at().is_some())
                {
                    committee.clear_membership(cred);
                    removed += 1;
                }
            }
            let mut added = 0usize;
            for (cred, term_epoch) in members_to_add {
                // Register the new member with no hot-key authorization
                // but with a term expiry epoch (upstream committeeMembers).
                if committee.register_with_term(*cred, *term_epoch) {
                    added += 1;
                }
            }
            enact.committee_quorum = *quorum;
            enact.has_committee = true;
            enact.prev_committee = Some(action_id);
            EnactOutcome::CommitteeUpdated {
                members_removed: removed,
                members_added: added,
            }
        }

        GovAction::HardForkInitiation {
            protocol_version, ..
        } => {
            let params = protocol_params.get_or_insert_with(Default::default);
            params.protocol_version = Some(*protocol_version);
            enact.prev_hard_fork = Some(action_id);
            EnactOutcome::HardForkEnacted {
                new_version: *protocol_version,
            }
        }

        GovAction::TreasuryWithdrawals { withdrawals, .. } => {
            let mut total = 0u64;
            for (ra, &amount) in withdrawals {
                if amount == 0 {
                    continue;
                }
                if let Some(ra_state) = reward_accounts.get_mut(ra) {
                    // Only credit registered reward accounts.
                    ra_state.set_balance(ra_state.balance().saturating_add(amount));
                    accounting.treasury = accounting.treasury.saturating_sub(amount);
                    total = total.saturating_add(amount);
                }
                // Unregistered reward accounts: withdrawal is lost (matching
                // upstream behavior where uncredited amounts remain in treasury).
            }
            EnactOutcome::TreasuryWithdrawn {
                total_withdrawn: total,
            }
        }

        GovAction::ParameterChange {
            protocol_param_update,
            ..
        } => {
            let params = protocol_params.get_or_insert_with(Default::default);
            params.apply_update(protocol_param_update);
            enact.prev_pparams_update = Some(action_id);
            EnactOutcome::ParameterChangeRecorded
        }
    }
}
