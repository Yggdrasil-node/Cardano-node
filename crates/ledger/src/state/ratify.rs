//! Conway RATIFY rule — governance-action ratification tally engine.
//!
//! Mirrors upstream `Cardano.Ledger.Conway.Rules.Ratify` and
//! `Cardano.Ledger.Conway.Governance.DRepPulser`.
//!
//! The functions here tally stored votes for each voter role (constitutional
//! committee, DReps, stake-pool operators) against the per-action-type
//! thresholds in `PoolVotingThresholds` / `DRepVotingThresholds`. The combined
//! predicate [`ratify_action`] determines whether a governance action has
//! been accepted.
//!
//! Extracted from `state.rs` in R269 second slice as part of the strict 1:1
//! filename-mirror refactor — see `docs/operational-runs/2026-05-06-round-269b-state-ratify-extraction.md`.

use super::{
    CommitteeState, DrepState, GovernanceActionState, PoolState, StakeCredentials,
    conway_drep_parameter_change_threshold, conway_parameter_change_has_spo_security_vote_group,
};
use crate::protocol_params::{DRepVotingThresholds, PoolVotingThresholds};
use crate::stake::PoolStakeDistribution;
use crate::types::{DRep, EpochNo, PoolKeyHash, StakeCredential, UnitInterval};
use std::collections::BTreeMap;

/// Tally result for one voter role.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VoteTally {
    /// Weighted "yes" votes.
    pub yes: u64,
    /// Weighted "no" votes (explicit only — abstentions excluded).
    pub no: u64,
    /// Weighted "abstain" votes.
    pub abstain: u64,
    /// Total eligible voting weight (yes + no + abstain + non-voting).
    pub total: u64,
}

impl VoteTally {
    /// Whether the "yes" fraction of **non-abstaining** weight meets `threshold`.
    ///
    /// Upstream semantics: `yes / (total - abstain) >= threshold`.
    /// Avoids float arithmetic by cross-multiplying.
    pub fn meets_threshold(&self, threshold: &UnitInterval) -> bool {
        let active = self.total.saturating_sub(self.abstain);
        if active == 0 {
            // Upstream: `a %? b = if b == 0 then 0 else a % b`
            // (Cardano.Ledger.BaseTypes).  A zero ratio only meets a zero
            // threshold (`r == minBound` short-circuit in committeeAccepted,
            // dRepAccepted, spoAccepted).
            return threshold.numerator == 0;
        }
        // yes * denominator >= threshold_numerator * active
        (self.yes as u128) * (threshold.denominator as u128)
            >= (threshold.numerator as u128) * (active as u128)
    }
}

/// Counts the number of active (non-resigned, non-expired) committee members.
///
/// A member is active when:
/// - They have a registered hot credential (not resigned), **and**
/// - Their term has not expired (`current_epoch <= expiry`).
///
/// This matches the upstream `activeCommitteeSize` calculation inside
/// `votingCommitteeThresholdInternal`.
fn count_active_committee_members(committee_state: &CommitteeState, current_epoch: EpochNo) -> u64 {
    committee_state
        .iter()
        .filter(|(_, member)| {
            member.is_enacted_member() && !member.is_resigned() && !member.is_expired(current_epoch)
        })
        .count() as u64
}

/// Tally constitutional-committee votes for a governance action.
///
/// Each non-resigned, non-expired committee member has equal weight (1).
/// Resigned members and members whose term has expired are excluded from
/// the total.
///
/// Upstream reference: `Cardano.Ledger.Conway.Rules.Ratify` —
/// `ccVotesSatisfied` filters `committeeMembers` by
/// `currentEpoch <= expirationEpoch` before tallying.
pub(crate) fn tally_committee_votes(
    action: &GovernanceActionState,
    committee_state: &CommitteeState,
    current_epoch: EpochNo,
) -> VoteTally {
    use crate::eras::conway::{Vote, Voter};

    let mut yes: u64 = 0;
    let mut no: u64 = 0;
    let mut abstain: u64 = 0;
    let mut eligible: u64 = 0;

    for (_cold_cred, member_state) in committee_state.iter() {
        // Non-enacted members (e.g., auto-registered via isPotentialFutureMember
        // or membership-cleared via NoConfidence) do not count.
        if !member_state.is_enacted_member() {
            continue;
        }
        // Resigned members do not count.
        if member_state.is_resigned() {
            continue;
        }
        // Expired members do not count (upstream: currentEpoch <= expirationEpoch).
        if member_state.is_expired(current_epoch) {
            continue;
        }
        eligible += 1;

        // Find whether this committee member voted.
        // Votes are keyed by Voter which carries HOT credential hashes
        // (Conway CDDL tags 0/1 = `committee_hot_credential`).  We must
        // look up the member's authorized hot credential and build the Voter
        // from that, not from the cold credential.
        //
        // Reference: `Cardano.Ledger.Conway.Rules.Ratify` — `ccVotesSatisfied`
        // iterates `committeeMembers`, resolves each cold credential to its
        // hot credential via `votingCommitteeCredentials`, and then looks up
        // the vote keyed by the hot credential.
        let hot_voter = member_state
            .hot_credential()
            .map(|hot_cred| match hot_cred {
                StakeCredential::AddrKeyHash(h) => Voter::CommitteeKeyHash(h),
                StakeCredential::ScriptHash(h) => Voter::CommitteeScript(h),
            });

        match hot_voter.and_then(|v| action.votes.get(&v)) {
            Some(Vote::Yes) => yes += 1,
            Some(Vote::No) => no += 1,
            Some(Vote::Abstain) => abstain += 1,
            None => {} // no hot credential or did not vote — counted in eligible but not tallied
        }
    }

    VoteTally {
        yes,
        no,
        abstain,
        total: eligible,
    }
}

/// Tally DRep votes for a governance action, weighted by delegated stake.
///
/// Only active DReps (not exceeding the `drep_activity` window) are
/// counted. Inactive DReps are excluded from both the vote tally and the
/// total eligible weight.
///
/// **`AlwaysAbstain`** delegated stake is excluded from the total,
/// effectively reducing the quorum denominator.
///
/// **`AlwaysNoConfidence`** delegated stake is always included in the
/// total.  When `count_no_confidence_as_yes` is true (i.e. for
/// `NoConfidence` and `UpdateCommittee`-in-state-of-no-confidence
/// actions), that stake is additionally counted as automatic "Yes"
/// votes.
///
/// `drep_delegated_stake` maps each `DRep` to the total lovelace
/// delegated to it. The caller is responsible for computing this from
/// the stake distribution (see `compute_drep_stake_distribution`).
///
/// Reference: `Cardano.Ledger.Conway.Rules.Ratify` — `dRepVotesSatisfied`.
pub(crate) fn tally_drep_votes(
    action: &GovernanceActionState,
    drep_state: &DrepState,
    drep_delegated_stake: &BTreeMap<DRep, u64>,
    current_epoch: EpochNo,
    drep_activity: u64,
    count_no_confidence_as_yes: bool,
) -> VoteTally {
    use crate::eras::conway::{Vote, Voter};

    let mut yes: u64 = 0;
    let mut no: u64 = 0;
    let mut abstain: u64 = 0;
    let mut total: u64 = 0;

    for (drep, stake) in drep_delegated_stake {
        match drep {
            DRep::AlwaysAbstain => {
                // Excluded from total — reduces quorum denominator.
                continue;
            }
            DRep::AlwaysNoConfidence => {
                // Always included in total.  Counted as automatic "Yes"
                // for NoConfidence/UpdateCommittee(no-confidence) actions.
                total = total.saturating_add(*stake);
                if count_no_confidence_as_yes {
                    yes = yes.saturating_add(*stake);
                }
                continue;
            }
            _ => {}
        }

        // Only active registered DReps count.
        let Some(reg) = drep_state.get(drep) else {
            continue;
        };
        // Check activity window.
        if reg
            .last_active_epoch
            .is_some_and(|e| e.0.saturating_add(drep_activity) < current_epoch.0)
        {
            continue; // inactive — excluded from quorum
        }

        total = total.saturating_add(*stake);

        // Find vote keyed by DRep voter tag. `AlwaysAbstain` /
        // `AlwaysNoConfidence` are already handled via `continue` in the
        // early match at the top of this loop so they cannot reach here
        // under current control-flow. `continue` (rather than
        // `unreachable!()`) keeps us defensive: if a future refactor
        // removes the early filter, we silently skip the variant instead
        // of panicking in production.
        let voter = match drep {
            DRep::KeyHash(h) => Voter::DRepKeyHash(*h),
            DRep::ScriptHash(h) => Voter::DRepScript(*h),
            DRep::AlwaysAbstain | DRep::AlwaysNoConfidence => continue,
        };

        match action.votes.get(&voter) {
            Some(Vote::Yes) => yes = yes.saturating_add(*stake),
            Some(Vote::No) => no = no.saturating_add(*stake),
            Some(Vote::Abstain) => abstain = abstain.saturating_add(*stake),
            None => {} // non-voting weight already in total
        }
    }

    VoteTally {
        yes,
        no,
        abstain,
        total,
    }
}

/// Default vote for a stake pool that did not vote explicitly.
///
/// Reference: `Cardano.Ledger.Conway.Governance.DefaultVote`,
/// `Cardano.Ledger.Conway.Governance.defaultStakePoolVote`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DefaultVote {
    /// Pool reward account delegates to a DRep key/script or is undelegated.
    No,
    /// Pool reward account delegates to `DRepAlwaysAbstain`.
    Abstain,
    /// Pool reward account delegates to `DRepAlwaysNoConfidence`.
    NoConfidence,
}

/// Determine the default SPO vote from the pool's reward-account DRep delegation.
///
/// Upstream: `defaultStakePoolVote poolId poolParams accounts`
/// 1. Look up the pool's `PoolParams` → `reward_account` → extract credential.
/// 2. Look up that credential in stake credentials → `delegated_drep`.
/// 3. Map `AlwaysAbstain → DefaultAbstain`, `AlwaysNoConfidence → DefaultNoConfidence`,
///    everything else (including undelegated) → `DefaultNo`.
pub(crate) fn default_stake_pool_vote(
    pool_hash: &PoolKeyHash,
    pool_state: &PoolState,
    stake_credentials: &StakeCredentials,
) -> DefaultVote {
    let pool = match pool_state.get(pool_hash) {
        Some(p) => p,
        None => return DefaultVote::No,
    };
    let cred = &pool.params().reward_account.credential;
    let drep = match stake_credentials.get(cred) {
        Some(state) => state.delegated_drep(),
        None => return DefaultVote::No,
    };
    match drep {
        Some(crate::types::DRep::AlwaysAbstain) => DefaultVote::Abstain,
        Some(crate::types::DRep::AlwaysNoConfidence) => DefaultVote::NoConfidence,
        _ => DefaultVote::No,
    }
}

/// Tally stake-pool operator (SPO) votes for a governance action, weighted
/// by delegated pool stake.
///
/// Reference: `Cardano.Ledger.Conway.Rules.Ratify` — `spoVotesSatisfied`.
pub(crate) fn tally_spo_votes(
    action: &GovernanceActionState,
    pool_stake_dist: &PoolStakeDistribution,
    is_bootstrap_phase: bool,
    pool_state: &PoolState,
    stake_credentials: &StakeCredentials,
) -> VoteTally {
    use crate::eras::conway::{Vote, Voter};

    let is_hard_fork = matches!(
        &action.proposal.gov_action,
        crate::eras::conway::GovAction::HardForkInitiation { .. }
    );
    let is_no_confidence = matches!(
        &action.proposal.gov_action,
        crate::eras::conway::GovAction::NoConfidence { .. }
    );

    let mut yes: u64 = 0;
    let mut no: u64 = 0;
    let mut abstain: u64 = 0;

    for (pool_hash, &pool_stake) in pool_stake_dist.iter() {
        let voter = Voter::StakePool(*pool_hash);
        match action.votes.get(&voter) {
            Some(Vote::Yes) => yes = yes.saturating_add(pool_stake),
            Some(Vote::No) => no = no.saturating_add(pool_stake),
            Some(Vote::Abstain) => abstain = abstain.saturating_add(pool_stake),
            None => {
                // Upstream spoAcceptedRatio:
                // - HardForkInitiation: non-voting → implicit No (always)
                // - Bootstrap phase: non-voting → implicit Abstain
                // - Post-bootstrap: uses defaultStakePoolVote
                //
                // Reference: Cardano.Ledger.Conway.Governance.defaultStakePoolVote
                if is_hard_fork {
                    // Non-voting on HardFork is always implicit No (not counted
                    // as yes or abstain, falls through to total denominator).
                } else if is_bootstrap_phase {
                    abstain = abstain.saturating_add(pool_stake);
                } else {
                    // Post-bootstrap: derive default vote from pool's reward
                    // account DRep delegation.
                    match default_stake_pool_vote(pool_hash, pool_state, stake_credentials) {
                        DefaultVote::Abstain => {
                            abstain = abstain.saturating_add(pool_stake);
                        }
                        DefaultVote::NoConfidence => {
                            if is_no_confidence {
                                yes = yes.saturating_add(pool_stake);
                            }
                            // else: implicit No (only counted in total)
                        }
                        DefaultVote::No => {
                            // implicit No (only counted in total)
                        }
                    }
                }
            }
        }
    }

    VoteTally {
        yes,
        no,
        abstain,
        total: pool_stake_dist.total_active_stake(),
    }
}

/// Look up the required DRep voting threshold for a governance action type.
///
/// Returns `None` for action types where DRep votes are not required
/// (InfoAction — always accepted, never enacted).
pub(crate) fn drep_threshold_for_action(
    action: &crate::eras::conway::GovAction,
    has_committee: bool,
    thresholds: &DRepVotingThresholds,
) -> Option<UnitInterval> {
    match action {
        crate::eras::conway::GovAction::ParameterChange {
            protocol_param_update,
            ..
        } => conway_drep_parameter_change_threshold(protocol_param_update, thresholds),
        crate::eras::conway::GovAction::HardForkInitiation { .. } => {
            Some(thresholds.hard_fork_initiation)
        }
        crate::eras::conway::GovAction::NoConfidence { .. } => {
            Some(thresholds.motion_no_confidence)
        }
        crate::eras::conway::GovAction::UpdateCommittee { .. } => {
            // Upstream: `isElectedCommittee = isSJust (ensCommitteeL)`.
            // When no committee exists (post-NoConfidence), use no-confidence
            // threshold.
            Some(if has_committee {
                thresholds.committee_normal
            } else {
                thresholds.committee_no_confidence
            })
        }
        crate::eras::conway::GovAction::NewConstitution { .. } => {
            Some(thresholds.update_to_constitution)
        }
        crate::eras::conway::GovAction::TreasuryWithdrawals { .. } => {
            Some(thresholds.treasury_withdrawal)
        }
        crate::eras::conway::GovAction::InfoAction => None,
    }
}

/// Look up the required SPO voting threshold for a governance action.
///
/// Returns `None` for actions where SPO votes are not required.
pub(crate) fn spo_threshold_for_action(
    action: &crate::eras::conway::GovAction,
    has_committee: bool,
    thresholds: &PoolVotingThresholds,
) -> Option<UnitInterval> {
    match action {
        crate::eras::conway::GovAction::ParameterChange {
            protocol_param_update,
            ..
        } => conway_parameter_change_has_spo_security_vote_group(protocol_param_update)
            .then_some(thresholds.pp_security_group),
        crate::eras::conway::GovAction::HardForkInitiation { .. } => {
            Some(thresholds.hard_fork_initiation)
        }
        crate::eras::conway::GovAction::NoConfidence { .. } => {
            Some(thresholds.motion_no_confidence)
        }
        crate::eras::conway::GovAction::UpdateCommittee { .. } => {
            // Upstream: `isElectedCommittee = isSJust (ensCommitteeL)`.
            Some(if has_committee {
                thresholds.committee_normal
            } else {
                thresholds.committee_no_confidence
            })
        }
        crate::eras::conway::GovAction::NewConstitution { .. }
        | crate::eras::conway::GovAction::TreasuryWithdrawals { .. }
        | crate::eras::conway::GovAction::InfoAction => None,
    }
}

/// Determines whether a governance action is accepted by the
/// constitutional committee.
///
/// The committee must meet a quorum (`committee_quorum` threshold)
/// with equal-weight per-member votes.
///
/// Upstream `votingCommitteeThresholdInternal` logic determines per-action
/// voting semantics:
/// - `NoConfidence` and `UpdateCommittee`: committee vote is not required
///   (`NoVotingAllowed` → always passes, threshold 0).
/// - `InfoAction`: no voting threshold available (`NoVotingThreshold` →
///   committee never accepts, matching upstream behavior where InfoAction
///   proposals are never ratified via committee vote).
/// - For all other actions (NewConstitution, HardForkInitiation,
///   ParameterChange, TreasuryWithdrawals): if the number of active
///   (non-resigned, non-expired) committee members is below
///   `min_committee_size` and we are **not** in bootstrap phase, the
///   committee never accepts (upstream: too-small committee treated as
///   absent).
///
/// Reference: `Cardano.Ledger.Conway.Governance.Internal` —
/// `votingCommitteeThresholdInternal`, `committeeAccepted`.
pub(crate) fn accepted_by_committee(
    action: &GovernanceActionState,
    committee_state: &CommitteeState,
    committee_quorum: &UnitInterval,
    current_epoch: EpochNo,
    min_committee_size: u64,
    is_bootstrap_phase: bool,
    has_committee: bool,
) -> bool {
    use crate::eras::conway::GovAction;

    match &action.proposal.gov_action {
        // NoVotingAllowed → threshold 0 → always passes.
        GovAction::NoConfidence { .. } | GovAction::UpdateCommittee { .. } => true,

        // NoVotingThreshold → SNothing → always fails.
        GovAction::InfoAction => false,

        // All other actions use the committee quorum threshold,
        // but only if a committee currently exists and is large enough.
        _ => {
            if !has_committee {
                // Upstream: ensCommitteeL == SNothing → NoVotingThreshold
                // → committeeAccepted returns False.
                return false;
            }
            if !is_bootstrap_phase {
                let active = count_active_committee_members(committee_state, current_epoch);
                if active < min_committee_size {
                    return false;
                }
            }
            let tally = tally_committee_votes(action, committee_state, current_epoch);
            tally.meets_threshold(committee_quorum)
        }
    }
}

/// Determines whether a governance action is accepted by DReps.
///
/// Returns `true` when:
/// - The action type does not require DRep approval, or
/// - The stake-weighted DRep tally meets the per-type threshold.
///
/// For `NoConfidence` and `UpdateCommittee`-in-state-of-no-confidence
/// actions, stake delegated to `AlwaysNoConfidence` is counted as
/// automatic "Yes" votes.
///
/// Reference: `Cardano.Ledger.Conway.Rules.Ratify` — `dRepVotesSatisfied`.
pub(crate) fn accepted_by_dreps(
    action: &GovernanceActionState,
    has_committee: bool,
    drep_state: &DrepState,
    drep_delegated_stake: &BTreeMap<DRep, u64>,
    current_epoch: EpochNo,
    drep_activity: u64,
    thresholds: &DRepVotingThresholds,
) -> bool {
    let Some(threshold) =
        drep_threshold_for_action(&action.proposal.gov_action, has_committee, thresholds)
    else {
        return true; // no DRep vote required for this action type
    };

    // AlwaysNoConfidence stake counts as "Yes" only for NoConfidence actions.
    //
    // Upstream reference: `dRepAcceptedRatio` in
    // `Cardano.Ledger.Conway.Rules.Ratify`:
    //   DRepAlwaysNoConfidence ->
    //     case govAction of
    //       NoConfidence _ -> (yes + stake, tot + stake)
    //       _              -> (yes, tot + stake)
    let count_no_confidence_as_yes = matches!(
        &action.proposal.gov_action,
        crate::eras::conway::GovAction::NoConfidence { .. }
    );

    let tally = tally_drep_votes(
        action,
        drep_state,
        drep_delegated_stake,
        current_epoch,
        drep_activity,
        count_no_confidence_as_yes,
    );
    tally.meets_threshold(&threshold)
}

/// Determines whether a governance action is accepted by stake-pool
/// operators.
///
/// Returns `true` when:
/// - The action type does not require SPO approval, or
/// - The stake-weighted SPO tally meets the per-type threshold.
pub(crate) fn accepted_by_spo(
    action: &GovernanceActionState,
    has_committee: bool,
    pool_stake_dist: &PoolStakeDistribution,
    thresholds: &PoolVotingThresholds,
    is_bootstrap_phase: bool,
    pool_state: &PoolState,
    stake_credentials: &StakeCredentials,
) -> bool {
    let Some(threshold) =
        spo_threshold_for_action(&action.proposal.gov_action, has_committee, thresholds)
    else {
        return true; // no SPO vote required for this action type
    };
    let tally = tally_spo_votes(
        action,
        pool_stake_dist,
        is_bootstrap_phase,
        pool_state,
        stake_credentials,
    );
    tally.meets_threshold(&threshold)
}

/// Combined ratification predicate: checks whether a governance action is
/// accepted by **all** required voter roles (CC + DRep + SPO).
///
/// This implements the core of the Conway RATIFY rule acceptance test.
/// InfoAction proposals are always accepted (they have no side effects).
///
/// Reference: `Cardano.Ledger.Conway.Rules.Ratify` — `ratifyTransition`.
pub(crate) fn ratify_action(
    action: &GovernanceActionState,
    committee_state: &CommitteeState,
    committee_quorum: &UnitInterval,
    drep_state: &DrepState,
    drep_delegated_stake: &BTreeMap<DRep, u64>,
    current_epoch: EpochNo,
    drep_activity: u64,
    drep_thresholds: &DRepVotingThresholds,
    pool_stake_dist: &PoolStakeDistribution,
    pool_thresholds: &PoolVotingThresholds,
    min_committee_size: u64,
    is_bootstrap_phase: bool,
    has_committee: bool,
    pool_state: &PoolState,
    stake_credentials: &StakeCredentials,
) -> bool {
    // Upstream: during Conway bootstrap phase (PV 9), all DRep thresholds are
    // zeroed (`def` = minBound for every field).  With zero thresholds the
    // `r == minBound` short-circuit in `dRepAccepted` makes every non-Info
    // action pass the DRep gate automatically.
    //
    // Reference: `votingDRepThresholdInternal` in
    // `Cardano.Ledger.Conway.Governance.Internal`:
    //   | hardforkConwayBootstrapPhase (pp ^. ppProtocolVersionL) = def
    let zero_drep = DRepVotingThresholds {
        motion_no_confidence: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        committee_normal: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        committee_no_confidence: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        update_to_constitution: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        hard_fork_initiation: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        pp_network_group: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        pp_economic_group: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        pp_technical_group: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        pp_gov_group: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
        treasury_withdrawal: UnitInterval {
            numerator: 0,
            denominator: 1,
        },
    };
    let effective_drep_thresholds = if is_bootstrap_phase {
        &zero_drep
    } else {
        drep_thresholds
    };

    accepted_by_committee(
        action,
        committee_state,
        committee_quorum,
        current_epoch,
        min_committee_size,
        is_bootstrap_phase,
        has_committee,
    ) && accepted_by_dreps(
        action,
        has_committee,
        drep_state,
        drep_delegated_stake,
        current_epoch,
        drep_activity,
        effective_drep_thresholds,
    ) && accepted_by_spo(
        action,
        has_committee,
        pool_stake_dist,
        pool_thresholds,
        is_bootstrap_phase,
        pool_state,
        stake_credentials,
    )
}
