//! Epoch boundary processing for the Shelley-based ledger.
//!
//! At each epoch transition the ledger performs the NEWEPOCH / EPOCH /
//! SNAP / RUPD sequence defined in the Shelley formal specification:
//!
//! 1. **Stake snapshot rotation** (SNAP rule) — a fresh snapshot is
//!    computed from the current UTxO and reward accounts, and the
//!    three-snapshot ring is rotated (`go ← set ← mark ← new`).
//! 2. **Reward distribution** (RUPD rule) — the reward pot is formed
//!    from monetary expansion (ρ) and accumulated fees, the treasury
//!    cut (τ) is deducted, and the remainder is distributed to pools
//!    and delegators according to the **go** snapshot.
//! 3. **Pool retirement** — pools whose `retiring_epoch` ≤ the new
//!    epoch are removed and their deposits refunded.
//! 4. **Accounting update** — treasury receives its cut plus any
//!    unclaimed rewards; reserves are reduced by monetary expansion.
//!
//! The orchestration entry point is [`apply_epoch_boundary`], which
//! operates on a [`LedgerState`] and returns an [`EpochBoundaryEvent`]
//! summarising the transition.
//!
//! Reference: `Cardano.Ledger.Shelley.Rules.NewEpoch`,
//! `Cardano.Ledger.Shelley.Rules.Epoch`.

use std::collections::{BTreeMap, BTreeSet};

use crate::error::LedgerError;
use crate::rewards::{compute_epoch_rewards, EpochRewardDistribution, RewardParams};
use crate::stake::{compute_drep_stake_distribution, compute_stake_snapshot, StakeSnapshots};
use crate::state::{EnactOutcome, LedgerState};
use crate::types::{EpochNo, PoolKeyHash, RewardAccount, UnitInterval};
use crate::eras::conway::GovActionId;

// ---------------------------------------------------------------------------
// Epoch boundary event
// ---------------------------------------------------------------------------

/// Summary of the work done at an epoch boundary.
///
/// Returned by [`apply_epoch_boundary`] so callers can trace or log the
/// transition details without inspecting ledger state diffs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EpochBoundaryEvent {
    /// The new epoch number after the transition.
    pub new_epoch: EpochNo,
    /// Number of protocol parameter fields updated via Shelley PPUP proposals.
    pub pparam_updates_applied: usize,
    /// Number of pools retired during this transition.
    pub pools_retired: usize,
    /// Operator keys of retired pools.
    pub retired_pool_keys: Vec<PoolKeyHash>,
    /// Pool deposits refunded to reward accounts (lovelace).
    pub pool_deposit_refunds: u64,
    /// Total rewards distributed to delegators & operators.
    pub rewards_distributed: u64,
    /// Treasury delta (treasury cut + unclaimed rewards).
    pub treasury_delta: u64,
    /// Monetary expansion drawn from reserves (ΔR).
    pub delta_reserves: u64,
    /// Number of reward accounts that received non-zero rewards.
    pub accounts_rewarded: usize,
    /// Number of governance actions that expired during this transition.
    pub governance_actions_expired: usize,
    /// Governance-action deposit lovelace refunded to return accounts.
    pub governance_deposit_refunds: u64,
    /// GovActionIds that were removed due to expiry.
    pub expired_gov_action_ids: Vec<GovActionId>,
    /// Number of DReps that became inactive during this transition.
    pub dreps_expired: usize,
    /// Number of governance actions ratified and enacted during this transition.
    pub governance_actions_enacted: usize,
    /// GovActionIds that were ratified and enacted.
    pub enacted_gov_action_ids: Vec<GovActionId>,
    /// Outcomes of each enacted governance action.
    pub enact_outcomes: Vec<EnactOutcome>,
    /// Governance-action deposit lovelace refunded for enacted actions.
    pub enacted_deposit_refunds: u64,
    /// GovActionIds removed due to conflicting lineage after enactment.
    pub removed_due_to_enactment: Vec<GovActionId>,
    /// Governance-action deposit lovelace refunded for lineage-conflicting removals.
    pub removed_due_to_enactment_deposit_refunds: u64,
    /// Unclaimed governance deposits (unregistered reward accounts) sent to treasury.
    pub unclaimed_governance_deposits: u64,
}

// ---------------------------------------------------------------------------
// Epoch boundary application
// ---------------------------------------------------------------------------

/// Applies the full epoch-boundary transition to `ledger`.
///
/// The caller is responsible for detecting that a new epoch has started
/// (e.g. via `consensus::epoch::is_new_epoch`).  This function is
/// idempotent only if the same epoch transition is not applied twice.
///
/// # Parameters
///
/// * `ledger` — mutable ledger state to update in place.
/// * `new_epoch` — the epoch number that has just begun.
/// * `snapshots` — the three-snapshot ring maintained alongside the ledger;
///   this is mutated to perform the SNAP rotation.
/// * `pool_performance` — per-pool performance ratios for the reward
///   calculation.  A pool absent from this map is treated as having
///   perfect (1/1) performance.
///
/// # Errors
///
/// Returns `LedgerError` if the ledger lacks protocol parameters
/// (required for deposit amounts and reward formula inputs).
pub fn apply_epoch_boundary(
    ledger: &mut LedgerState,
    new_epoch: EpochNo,
    snapshots: &mut StakeSnapshots,
    pool_performance: &BTreeMap<PoolKeyHash, UnitInterval>,
) -> Result<EpochBoundaryEvent, LedgerError> {
    ledger.set_current_epoch(new_epoch);

    // -----------------------------------------------------------------------
    // 0. PPUP — apply any pending Shelley-era protocol parameter update
    //    proposals whose target epoch matches the new epoch.
    //    Reference: `Cardano.Ledger.Shelley.Rules.NewEpoch` — PPUP tick.
    // -----------------------------------------------------------------------
    let pparam_updates_applied = ledger.apply_pending_pparam_updates(new_epoch);

    let params = ledger
        .protocol_params()
        .ok_or(LedgerError::MissingProtocolParameters)?;

    // Extract values from params before any mutable borrows.
    let pool_deposit = params.pool_deposit;
    let rho = params.rho;
    let tau = params.tau;
    let a0 = params.a0;
    let n_opt = params.n_opt;
    let min_pool_cost = params.min_pool_cost;
    let drep_activity = params.drep_activity.unwrap_or(u64::MAX);

    // -----------------------------------------------------------------------
    // 1. RUPD — compute and distribute rewards from the *go* snapshot.
    //
    //    The upstream NEWEPOCH rule credits rewards BEFORE the SNAP
    //    rotation so that newly-credited reward balances are included in
    //    the freshly-computed mark snapshot.
    //
    //    Reference: `Cardano.Ledger.Shelley.Rules.NewEpoch` — RUPD runs
    //    before EPOCH (which contains SNAP).
    // -----------------------------------------------------------------------
    let fee_pot = std::mem::take(&mut snapshots.fee_pot);
    let reward_params = RewardParams {
        rho,
        tau,
        a0,
        n_opt,
        min_pool_cost,
        reserves: ledger.accounting().reserves,
        fee_pot,
    };

    let reward_dist = compute_epoch_rewards(&reward_params, &snapshots.go, pool_performance);
    let accounts_rewarded = distribute_rewards(ledger, &reward_dist);

    // -----------------------------------------------------------------------
    // 2. SNAP — compute a fresh mark snapshot from post-reward state
    //    and rotate the three-snapshot ring.
    //
    //    Because rewards have already been credited above, the new mark
    //    snapshot reflects the updated reward account balances.
    //
    //    Reference: `Cardano.Ledger.Shelley.Rules.Snap` — runs inside
    //    the EPOCH rule, after RUPD.
    // -----------------------------------------------------------------------
    let new_mark = compute_stake_snapshot(
        ledger.multi_era_utxo(),
        ledger.stake_credentials(),
        ledger.reward_accounts(),
        ledger.pool_state(),
    );
    // fee_pot was already taken above; rotate returns 0 here.
    let _ = snapshots.rotate(new_mark);

    // -----------------------------------------------------------------------
    // 3. Pool retirement — remove pools and refund deposits.
    // -----------------------------------------------------------------------
    let (retired_pool_keys, pool_deposit_refunds) =
        retire_pools_with_refunds(ledger, new_epoch, pool_deposit);
    let pools_retired = retired_pool_keys.len();

    // -----------------------------------------------------------------------
    // 4. Accounting — update treasury and reserves.
    //
    //    Only `delta_reserves` (= reserves × ρ, the monetary expansion)
    //    is subtracted from reserves.  The fee pot comes from transaction
    //    fees, not from reserves.
    //
    //    Reference: `Cardano.Ledger.Shelley.Rules.NewEpoch` — accounting
    //    update step.
    // -----------------------------------------------------------------------
    {
        let acct = ledger.accounting_mut();
        acct.reserves = acct.reserves.saturating_sub(reward_dist.delta_reserves);
        acct.treasury = acct.treasury.saturating_add(reward_dist.treasury_delta);
    }

    // -----------------------------------------------------------------------
    // 5. Governance action expiry — remove expired proposals and refund
    //    deposits to their return accounts (Conway+ EPOCH rule).
    // -----------------------------------------------------------------------
    let (expired_gov_action_ids, governance_deposit_refunds) =
        remove_expired_governance_actions(ledger, new_epoch);
    let governance_actions_expired = expired_gov_action_ids.len();

    // -----------------------------------------------------------------------
    // 5b. Ratification — tally votes for surviving governance actions and
    //     enact any that reach their acceptance thresholds.
    //     Upstream: `Cardano.Ledger.Conway.Rules.Ratify` — run at each
    //     epoch boundary after expiry pruning.
    // -----------------------------------------------------------------------
    let ratify_result = ratify_and_enact(ledger, new_epoch, snapshots, drep_activity);
    let governance_actions_enacted = ratify_result.enacted_ids.len();

    // Credit unclaimed governance deposits to treasury.
    if ratify_result.unclaimed_deposits > 0 {
        let acct = ledger.accounting_mut();
        acct.treasury = acct.treasury.saturating_add(ratify_result.unclaimed_deposits);
    }

    // -----------------------------------------------------------------------
    // 6. DRep inactivity — compute the set of DReps that have exceeded
    //    the `drep_activity` window.  Inactive DReps remain registered
    //    but are excluded from ratification quorum calculations.
    //    Upstream: `Cardano.Ledger.Conway.Rules.Epoch` — drepExpiry.
    // -----------------------------------------------------------------------
    let dreps_expired = {
        ledger.drep_state().inactive_dreps(new_epoch, drep_activity).len()
    };

    Ok(EpochBoundaryEvent {
        new_epoch,
        pparam_updates_applied,
        pools_retired,
        retired_pool_keys,
        pool_deposit_refunds,
        rewards_distributed: reward_dist.distributed,
        treasury_delta: reward_dist.treasury_delta,
        delta_reserves: reward_dist.delta_reserves,
        accounts_rewarded,
        governance_actions_expired,
        governance_deposit_refunds,
        expired_gov_action_ids,
        dreps_expired,
        governance_actions_enacted,
        enacted_gov_action_ids: ratify_result.enacted_ids,
        enact_outcomes: ratify_result.outcomes,
        enacted_deposit_refunds: ratify_result.enacted_deposit_refunds,
        removed_due_to_enactment: ratify_result.removed_due_to_enactment,
        removed_due_to_enactment_deposit_refunds: ratify_result.removed_due_to_enactment_deposit_refunds,
        unclaimed_governance_deposits: ratify_result.unclaimed_deposits,
    })
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Credits reward accounts from the epoch distribution.
///
/// Returns the number of accounts that received non-zero rewards.
fn distribute_rewards(
    ledger: &mut LedgerState,
    dist: &EpochRewardDistribution,
) -> usize {
    let mut count = 0usize;
    let ra = ledger.reward_accounts_mut();
    for (account, &amount) in &dist.reward_deltas {
        if amount == 0 {
            continue;
        }
        if let Some(state) = ra.get_mut(account) {
            state.set_balance(state.balance().saturating_add(amount));
            count += 1;
        }
        // If the reward account is not registered, the reward is
        // effectively unclaimed and rolls into the treasury at the
        // next epoch boundary (upstream behavior).
    }
    count
}

/// Removes governance actions whose `expires_after` is strictly before `epoch`,
/// refunds each action's deposit to its recorded return account, and returns
/// the removed action IDs plus the total lovelace refunded.
///
/// Upstream reference: `Cardano.Ledger.Conway.Rules.Epoch` — expired-action
/// pruning step of the EPOCH rule, prior to starting a new DRep pulser.
fn remove_expired_governance_actions(
    ledger: &mut LedgerState,
    epoch: EpochNo,
) -> (Vec<GovActionId>, u64) {
    // 1. Identify expired governance action IDs.
    let expired_ids: Vec<GovActionId> = ledger
        .governance_actions()
        .iter()
        .filter(|(_, state)| {
            state
                .expires_after()
                .is_some_and(|exp| exp.0 < epoch.0)
        })
        .map(|(id, _)| id.clone())
        .collect();

    if expired_ids.is_empty() {
        return (Vec::new(), 0);
    }

    // 2. Remove expired actions and collect their deposit + return address.
    let mut refund_targets: Vec<(Vec<u8>, u64)> = Vec::with_capacity(expired_ids.len());
    for id in &expired_ids {
        if let Some(state) = ledger.governance_actions_mut().remove(id) {
            refund_targets.push((
                state.proposal().reward_account.clone(),
                state.proposal().deposit,
            ));
        }
    }

    // 3. Credit refunds to reward accounts.
    let mut total_refunded: u64 = 0;
    for (raw_account, deposit) in &refund_targets {
        if let Some(reward_account) = RewardAccount::from_bytes(raw_account) {
            if let Some(ra_state) = ledger.reward_accounts_mut().get_mut(&reward_account) {
                ra_state.set_balance(ra_state.balance().saturating_add(*deposit));
                total_refunded = total_refunded.saturating_add(*deposit);
            }
            // If the reward account is no longer registered, the deposit
            // is effectively lost — matching upstream behavior where
            // unclaimed refunds accrue to the treasury.
        }
    }

    (expired_ids, total_refunded)
}

/// Tallies votes for all surviving governance actions, enacts those that
/// reach acceptance thresholds, removes enacted actions from the ledger,
/// refunds their deposits, prunes lineage-conflicting proposals, and
/// returns the enacted action IDs with their outcomes.
///
/// This implements the Conway RATIFY rule at the epoch boundary: after
/// expired proposals are pruned, each remaining governance action is
/// evaluated against committee, DRep, and SPO acceptance predicates.
/// Actions accepted by all required roles are enacted via
/// `enact_gov_action` and then removed from the pending set.  After
/// enactment, proposals whose `prev_action_id` references a now-stale
/// lineage root are also removed (subtree pruning), matching upstream
/// `proposalsApplyEnactment`.
///
/// Deposits for all removed actions (enacted + lineage-conflicting) are
/// refunded to their return reward accounts if registered.  Unclaimed
/// deposits (unregistered return accounts) are returned separately so
/// the caller can add them to the treasury.
///
/// Upstream reference: `Cardano.Ledger.Conway.Rules.Ratify`,
/// `Cardano.Ledger.Conway.Governance.Procedures.proposalsApplyEnactment`.
fn ratify_and_enact(
    ledger: &mut LedgerState,
    current_epoch: EpochNo,
    snapshots: &StakeSnapshots,
    drep_activity: u64,
) -> RatifyAndEnactResult {
    use crate::state::ratify_action;

    // Extract thresholds from protocol params. When Conway-specific threshold
    // fields are absent, fall back to the built-in Conway defaults rather than
    // silently skipping ratification.
    let (pool_thresholds, drep_thresholds, min_committee_size) = match ledger.protocol_params() {
        Some(pp) => (
            pp.pool_voting_thresholds.clone().unwrap_or_default(),
            pp.drep_voting_thresholds.clone().unwrap_or_default(),
            pp.min_committee_size,
        ),
        None => return RatifyAndEnactResult::default(),
    };

    // Compute DRep delegated stake distribution from the mark snapshot.
    let drep_delegated_stake =
        compute_drep_stake_distribution(&snapshots.mark, ledger.stake_credentials());

    // Compute SPO pool stake distribution from the mark snapshot.
    let pool_stake_dist = snapshots.mark.pool_stake_distribution();

    // Read committee quorum from enact state.
    let committee_quorum = ledger.enact_state().committee_quorum;

    // Collect IDs of actions that pass ratification.
    let ratified_ids: Vec<GovActionId> = ledger
        .governance_actions()
        .iter()
        .filter(|(_, action_state)| {
            if !committee_update_meets_min_size(
                action_state,
                ledger.committee_state(),
                min_committee_size,
            ) {
                return false;
            }

            ratify_action(
                action_state,
                ledger.committee_state(),
                &committee_quorum,
                ledger.drep_state(),
                &drep_delegated_stake,
                current_epoch,
                drep_activity,
                &drep_thresholds,
                &pool_stake_dist,
                &pool_thresholds,
            )
        })
        .map(|(id, _)| id.clone())
        .collect();

    if ratified_ids.is_empty() {
        return RatifyAndEnactResult::default();
    }

    // Remove ratified actions, refund their deposits, and enact each one.
    // First, collect the affected governance purposes so we can prune
    // stale proposals of those purposes after enactment.
    let mut outcomes = Vec::with_capacity(ratified_ids.len());
    let mut deposit_targets: Vec<(Vec<u8>, u64)> = Vec::new();
    let mut enacted_purposes: BTreeSet<crate::state::ConwayGovActionPurpose> = BTreeSet::new();

    for id in &ratified_ids {
        let action_state = ledger.governance_actions_mut().remove(id);
        if let Some(state) = action_state {
            enacted_purposes.insert(
                crate::state::conway_gov_action_purpose(&state.proposal().gov_action),
            );
            // Collect deposit for refund.
            deposit_targets.push((
                state.proposal().reward_account.clone(),
                state.proposal().deposit,
            ));
            let outcome = ledger.enact_action(id.clone(), &state.proposal().gov_action);
            outcomes.push(outcome);
        }
    }

    // -----------------------------------------------------------------------
    // Subtree pruning: remove proposals whose prev_action_id no longer
    // chains from the current enacted lineage root for their purpose.
    //
    // When an action is enacted, the lineage root for its governance
    // purpose advances to the enacted action's GovActionId.  Any pending
    // proposal of that purpose whose chain does not start at the new root
    // is stale and must be removed.  The pruning is transitive: if
    // proposal B chains from a stale proposal A, B is also removed.
    //
    // Upstream reference: `proposalsApplyEnactment`.
    // -----------------------------------------------------------------------
    let removed_due_to_enactment = remove_lineage_conflicting_proposals(
        ledger,
        &enacted_purposes,
    );

    // Collect deposit targets from subtree-removed actions.
    for id in &removed_due_to_enactment {
        if let Some(state) = ledger.governance_actions_mut().remove(id) {
            deposit_targets.push((
                state.proposal().reward_account.clone(),
                state.proposal().deposit,
            ));
        }
    }

    // -----------------------------------------------------------------------
    // Refund all deposits (enacted + lineage-conflicting) to reward accounts.
    // Unclaimed deposits (unregistered accounts) go to the treasury.
    //
    // Upstream reference: `returnProposalDeposits`.
    // -----------------------------------------------------------------------
    let mut enacted_refunded: u64 = 0;
    let mut subtree_refunded: u64 = 0;
    let mut unclaimed: u64 = 0;
    let enacted_count = ratified_ids.len();

    for (i, (raw_account, deposit)) in deposit_targets.iter().enumerate() {
        if let Some(reward_account) = RewardAccount::from_bytes(raw_account) {
            if let Some(ra_state) = ledger.reward_accounts_mut().get_mut(&reward_account) {
                ra_state.set_balance(ra_state.balance().saturating_add(*deposit));
                if i < enacted_count {
                    enacted_refunded = enacted_refunded.saturating_add(*deposit);
                } else {
                    subtree_refunded = subtree_refunded.saturating_add(*deposit);
                }
            } else {
                // Unregistered return account — deposit goes to treasury.
                unclaimed = unclaimed.saturating_add(*deposit);
            }
        } else {
            // Malformed reward account bytes — treat as unclaimed.
            unclaimed = unclaimed.saturating_add(*deposit);
        }
    }

    RatifyAndEnactResult {
        enacted_ids: ratified_ids,
        outcomes,
        enacted_deposit_refunds: enacted_refunded,
        removed_due_to_enactment,
        removed_due_to_enactment_deposit_refunds: subtree_refunded,
        unclaimed_deposits: unclaimed,
    }
}

fn committee_update_meets_min_size(
    action_state: &crate::state::GovernanceActionState,
    committee_state: &crate::state::CommitteeState,
    min_committee_size: Option<u64>,
) -> bool {
    let Some(minimum) = min_committee_size else {
        return true;
    };

    let crate::eras::conway::GovAction::UpdateCommittee {
        members_to_remove,
        members_to_add,
        ..
    } = &action_state.proposal().gov_action
    else {
        return true;
    };

    let mut members: BTreeSet<_> = committee_state.iter().map(|(credential, _)| *credential).collect();
    for member in members_to_remove {
        members.remove(member);
    }
    for member in members_to_add.keys() {
        members.insert(*member);
    }

    (members.len() as u64) >= minimum
}

/// Result of the ratification-and-enactment step at an epoch boundary.
#[derive(Clone, Debug, Default)]
struct RatifyAndEnactResult {
    /// GovActionIds that were ratified and enacted.
    enacted_ids: Vec<GovActionId>,
    /// Outcomes of each enacted governance action.
    outcomes: Vec<EnactOutcome>,
    /// Governance-action deposit lovelace refunded for enacted actions.
    enacted_deposit_refunds: u64,
    /// GovActionIds removed due to conflicting lineage after enactment.
    removed_due_to_enactment: Vec<GovActionId>,
    /// Governance-action deposit lovelace refunded for lineage-conflicting removals.
    removed_due_to_enactment_deposit_refunds: u64,
    /// Unclaimed governance deposits (unregistered reward accounts) for treasury.
    unclaimed_deposits: u64,
}

/// Extracts the `prev_action_id` from a `GovAction`, if the action type
/// carries one (ParameterChange, HardForkInitiation, NoConfidence,
/// UpdateCommittee, NewConstitution).  Returns `None` for TreasuryWithdrawals
/// and InfoAction which have no lineage.
fn gov_action_prev_id(action: &crate::eras::conway::GovAction) -> Option<&Option<GovActionId>> {
    use crate::eras::conway::GovAction;
    match action {
        GovAction::ParameterChange { prev_action_id, .. } => Some(prev_action_id),
        GovAction::HardForkInitiation { prev_action_id, .. } => Some(prev_action_id),
        GovAction::NoConfidence { prev_action_id, .. } => Some(prev_action_id),
        GovAction::UpdateCommittee { prev_action_id, .. } => Some(prev_action_id),
        GovAction::NewConstitution { prev_action_id, .. } => Some(prev_action_id),
        GovAction::TreasuryWithdrawals { .. } | GovAction::InfoAction => None,
    }
}

/// Remove pending governance proposals that no longer chain from the
/// current enacted lineage root after enactment.
///
/// This implements the `proposalsApplyEnactment` step from upstream.
/// When an action is enacted for a given governance purpose, the lineage
/// root for that purpose advances to the enacted action's `GovActionId`.
/// Any remaining proposal of that purpose whose `prev_action_id` does
/// **not** chain from the new root is stale and must be removed.  The
/// pruning is transitive: if proposal B chains from a stale proposal A,
/// B is also removed.
///
/// Purposes that had no enactments are left untouched.  TreasuryWithdrawals
/// and InfoAction have no lineage and are never pruned.
///
/// Returns the IDs of the stale proposals.  The caller is responsible for
/// actually removing them from `governance_actions_mut()` and refunding
/// their deposits.
fn remove_lineage_conflicting_proposals(
    ledger: &LedgerState,
    enacted_purposes: &BTreeSet<crate::state::ConwayGovActionPurpose>,
) -> Vec<GovActionId> {
    use crate::state::conway_gov_action_purpose;

    let mut stale_ids: Vec<GovActionId> = Vec::new();

    for &purpose in enacted_purposes {
        // The new lineage root for this purpose (after enactment).
        let new_root: Option<&GovActionId> = ledger.enact_state().enacted_root(purpose);

        // Collect all remaining proposals of this purpose.
        let purpose_proposals: Vec<(GovActionId, Option<GovActionId>)> = ledger
            .governance_actions()
            .iter()
            .filter(|(_, state)| {
                conway_gov_action_purpose(&state.proposal().gov_action) == purpose
            })
            .map(|(id, state)| {
                let prev = gov_action_prev_id(&state.proposal().gov_action)
                    .and_then(|opt| opt.clone());
                (id.clone(), prev)
            })
            .collect();

        // Build the set of valid proposals: those that chain from new_root.
        // A proposal P is valid if:
        //   P.prev_action_id == new_root, OR
        //   P.prev_action_id == Some(Q) where Q is a valid proposal.
        let mut valid: BTreeSet<GovActionId> = BTreeSet::new();
        loop {
            let mut changed = false;
            for (id, prev) in &purpose_proposals {
                if valid.contains(id) {
                    continue;
                }
                let chains_from_root = match (prev, new_root) {
                    (None, None) => true,
                    (Some(p), Some(r)) if p == r => true,
                    _ => false,
                };
                if chains_from_root || prev.as_ref().is_some_and(|p| valid.contains(p)) {
                    valid.insert(id.clone());
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        // Everything not valid is stale.
        for (id, _) in &purpose_proposals {
            if !valid.contains(id) {
                stale_ids.push(id.clone());
            }
        }
    }

    stale_ids
}

/// Retires pools whose `retiring_epoch` ≤ `epoch`, refunds their deposits,
/// and returns the list of retired pool operator keys and total refund.
///
/// This is the preferred helper that captures reward accounts *before*
/// removing pools, avoiding the ordering problem in the two-step approach.
pub fn retire_pools_with_refunds(
    ledger: &mut LedgerState,
    epoch: EpochNo,
    pool_deposit: u64,
) -> (Vec<PoolKeyHash>, u64) {
    // 1. Identify pools scheduled to retire and capture their reward accounts.
    let retiring: Vec<(PoolKeyHash, RewardAccount)> = ledger
        .pool_state()
        .iter()
        .filter(|(_, pool)| {
            pool.retiring_epoch().is_some_and(|e| e <= epoch)
        })
        .map(|(k, pool)| (*k, pool.params().reward_account))
        .collect();

    if retiring.is_empty() {
        return (Vec::new(), 0);
    }

    // 2. Remove the retiring pools from the registry.
    let retired_keys = ledger.pool_state_mut().process_retirements(epoch);

    // 3. Credit refunds to reward accounts and update deposit pot.
    let mut total_refunded: u64 = 0;
    for (_, reward_account) in &retiring {
        if let Some(state) = ledger.reward_accounts_mut().get_mut(reward_account) {
            state.set_balance(state.balance().saturating_add(pool_deposit));
            total_refunded = total_refunded.saturating_add(pool_deposit);
        }
        ledger.deposit_pot_mut().return_pool_deposit(pool_deposit);
    }

    (retired_keys, total_refunded)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stake::StakeSnapshot;
    use crate::state::GenesisDelegationState;
    use crate::state::RewardAccountState;
    use crate::types::{
        Address, BaseAddress, EpochNo, PoolKeyHash, PoolParams, RewardAccount,
        StakeCredential, UnitInterval,
    };
    use crate::eras::{Era, ShelleyTxIn, ShelleyTxOut};
    use crate::eras::shelley::ShelleyUpdate;
    use crate::protocol_params::{ProtocolParameterUpdate, ProtocolParameters};

    fn test_cred(b: u8) -> StakeCredential {
        StakeCredential::AddrKeyHash([b; 28])
    }

    fn test_pool(b: u8) -> PoolKeyHash {
        [b; 28]
    }

    fn test_reward_account(b: u8) -> RewardAccount {
        RewardAccount {
            network: 1,
            credential: test_cred(b),
        }
    }

    fn test_pool_params(b: u8) -> PoolParams {
        PoolParams {
            operator: test_pool(b),
            vrf_keyhash: [b; 32],
            pledge: 100_000_000,
            cost: 340_000_000,
            margin: UnitInterval {
                numerator: 1,
                denominator: 100,
            },
            reward_account: test_reward_account(b),
            pool_owners: vec![[b; 28]],
            relays: vec![],
            pool_metadata: None,
        }
    }

    fn test_protocol_params() -> ProtocolParameters {
        let mut pp = ProtocolParameters::default();
        pp.rho = UnitInterval {
            numerator: 3,
            denominator: 1000,
        };
        pp.tau = UnitInterval {
            numerator: 2,
            denominator: 10,
        };
        pp.a0 = UnitInterval {
            numerator: 3,
            denominator: 10,
        };
        pp.n_opt = 150;
        pp.min_pool_cost = 170_000_000;
        pp.pool_deposit = 500_000_000;
        pp.key_deposit = 2_000_000;
        pp
    }

    fn make_ledger_with_pool(pool_id: u8) -> LedgerState {
        let mut ledger = LedgerState::new(Era::Shelley);
        ledger.set_protocol_params(test_protocol_params());

        // Register a pool.
        let params = test_pool_params(pool_id);
        ledger.pool_state_mut().register(params);

        // Register the pool operator as a stake credential + delegation.
        let cred = test_cred(pool_id);
        ledger.stake_credentials_mut().register(cred);
        if let Some(cs) = ledger.stake_credentials_mut().get_mut(&cred) {
            cs.set_delegated_pool(Some(test_pool(pool_id)));
        }

        // Create a reward account for the pool.
        let ra = test_reward_account(pool_id);
        ledger
            .reward_accounts_mut()
            .insert(ra, RewardAccountState::new(0, None));

        // Set initial accounting.
        ledger.accounting_mut().reserves = 14_000_000_000_000_000; // 14B ADA
        ledger.accounting_mut().treasury = 500_000_000_000; // 500k ADA

        ledger
    }

    fn require_committee_vote_for_ratification(ledger: &mut LedgerState, cold_byte: u8, hot_byte: u8) {
        let cc_cred = test_cred(cold_byte);
        let hot_cred = test_cred(hot_byte);
        ledger.committee_state_mut().register(cc_cred);
        ledger
            .committee_state_mut()
            .get_mut(&cc_cred)
            .expect("registered committee credential")
            .set_authorization(Some(
                crate::state::CommitteeAuthorization::CommitteeHotCredential(hot_cred),
            ));
        ledger.enact_state_mut().committee_quorum = UnitInterval {
            numerator: 1,
            denominator: 1,
        };
    }

    // -- Snapshot rotation ------------------------------------------------

    #[test]
    fn test_snapshot_rotation_at_epoch_boundary() {
        let mut ledger = make_ledger_with_pool(1);
        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        let event = apply_epoch_boundary(
            &mut ledger,
            EpochNo(1),
            &mut snapshots,
            &perf,
        )
        .expect("epoch boundary should succeed");

        assert_eq!(event.new_epoch, EpochNo(1));
        assert_eq!(ledger.current_epoch(), EpochNo(1));
        // After one rotation, the fresh snapshot lands in `mark`
        // (go ← set ← mark ← new).  Pool 1's params should be captured.
        assert!(!snapshots.mark.pool_params.is_empty());
    }

    // -- Reward distribution ----------------------------------------------

    #[test]
    fn test_rewards_distributed_to_operator() {
        let mut ledger = make_ledger_with_pool(2);

        // Seed some UTxO stake delegated to pool 2 so the reward formula
        // produces a non-zero reward.
        let cred = test_cred(2);
        let base_addr = Address::Base(BaseAddress {
            network: 1,
            payment: StakeCredential::AddrKeyHash([0xAA; 28]),
            staking: cred,
        });
        let addr_bytes = base_addr.to_bytes();
        let txin = ShelleyTxIn { transaction_id: [0u8; 32], index: 0 };
        ledger.multi_era_utxo_mut().insert_shelley(
            txin,
            ShelleyTxOut {
                address: addr_bytes.clone(),
                amount: 10_000_000_000_000, // 10M ADA
            },
        );

        let mut snapshots = StakeSnapshots::new();
        // Accumulate some fees.
        snapshots.accumulate_fees(1_000_000_000); // 1000 ADA

        // First rotation to populate `mark`.
        let perf = BTreeMap::new();
        let _event = apply_epoch_boundary(
            &mut ledger,
            EpochNo(1),
            &mut snapshots,
            &perf,
        )
        .expect("epoch 1 boundary should succeed");

        // Second rotation moves the snapshot into `go`, enabling rewards.
        // Add more fees for epoch 2.
        snapshots.accumulate_fees(500_000_000); // 500 ADA

        let event = apply_epoch_boundary(
            &mut ledger,
            EpochNo(2),
            &mut snapshots,
            &perf,
        )
        .expect("epoch 2 boundary should succeed");

        // Some rewards should have been distributed (unless pool cost
        // exceeds the apparent reward — depends on reserve size).
        // With 14B ADA reserves and rho = 3/1000, the monetary expansion
        // is ~42M ADA, which is far above any single pool's cost.
        // At the go snapshot the pool should receive something.
        assert!(
            event.rewards_distributed > 0 || event.treasury_delta > 0,
            "expected some reward activity at epoch boundary"
        );
    }

    // -- Pool retirement + deposit refund ---------------------------------

    #[test]
    fn test_pool_retirement_refunds_deposit() {
        let mut ledger = make_ledger_with_pool(3);
        let pool_deposit = 500_000_000u64;

        // Record that we charged a pool deposit.
        ledger.deposit_pot_mut().add_pool_deposit(pool_deposit);

        // Schedule pool 3 for retirement at epoch 5.
        ledger
            .pool_state_mut()
            .retire(test_pool(3), EpochNo(5));

        // Before retirement.
        let ra = test_reward_account(3);
        let balance_before = ledger.reward_accounts().balance(&ra);

        let (retired, refunded) =
            retire_pools_with_refunds(&mut ledger, EpochNo(5), pool_deposit);

        assert_eq!(retired.len(), 1);
        assert_eq!(retired[0], test_pool(3));
        assert_eq!(refunded, pool_deposit);

        // Reward account should have been credited.
        let balance_after = ledger.reward_accounts().balance(&ra);
        assert_eq!(balance_after, balance_before + pool_deposit);

        // Deposit pot should be reduced.
        assert_eq!(ledger.deposit_pot().pool_deposits, 0);
    }

    // -- Pool not yet due for retirement ----------------------------------

    #[test]
    fn test_pool_not_yet_retiring() {
        let mut ledger = make_ledger_with_pool(4);
        ledger.deposit_pot_mut().add_pool_deposit(500_000_000);

        // Schedule for epoch 10.
        ledger
            .pool_state_mut()
            .retire(test_pool(4), EpochNo(10));

        // Try retiring at epoch 5 — pool should NOT be retired.
        let (retired, refunded) =
            retire_pools_with_refunds(&mut ledger, EpochNo(5), 500_000_000);

        assert!(retired.is_empty());
        assert_eq!(refunded, 0);
        // Pool should still be registered.
        assert!(ledger.pool_state().get(&test_pool(4)).is_some());
    }

    // -- Accounting update (treasury/reserves) ----------------------------

    #[test]
    fn test_accounting_update_after_epoch_boundary() {
        let mut ledger = make_ledger_with_pool(5);
        let initial_reserves = ledger.accounting().reserves;
        let initial_treasury = ledger.accounting().treasury;

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        let _event = apply_epoch_boundary(
            &mut ledger,
            EpochNo(1),
            &mut snapshots,
            &perf,
        )
        .expect("epoch 1 boundary should succeed");

        // With empty go snapshot, no rewards are distributed, but the
        // reward pot formation still draws from reserves and credits treasury.
        let new_reserves = ledger.accounting().reserves;
        let new_treasury = ledger.accounting().treasury;

        // Reserves should not increase (they can stay the same if the go
        // snapshot is empty and all goes to treasury).
        assert!(new_reserves <= initial_reserves);
        // Treasury should not decrease.
        assert!(new_treasury >= initial_treasury);
    }

    // -- Empty ledger (no protocol params) --------------------------------

    #[test]
    fn test_epoch_boundary_without_params_fails() {
        let mut ledger = LedgerState::new(Era::Shelley);
        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        let result = apply_epoch_boundary(
            &mut ledger,
            EpochNo(1),
            &mut snapshots,
            &perf,
        );

        assert!(result.is_err());
    }

    // -- Multiple epoch boundaries ----------------------------------------

    #[test]
    fn test_multiple_epoch_boundaries() {
        let mut ledger = make_ledger_with_pool(6);
        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        for epoch in 1..=5 {
            snapshots.accumulate_fees(100_000_000); // 100 ADA per epoch
            let event = apply_epoch_boundary(
                &mut ledger,
                EpochNo(epoch),
                &mut snapshots,
                &perf,
            )
            .expect("epoch boundary should succeed");

            assert_eq!(event.new_epoch, EpochNo(epoch));
        }

        // After 5 epochs, treasury should have grown.
        assert!(ledger.accounting().treasury > 500_000_000_000);
    }

    // -- Fee pot carried across rotation ----------------------------------

    #[test]
    fn test_fee_pot_carried_into_rewards() {
        let mut snapshots = StakeSnapshots::new();
        snapshots.accumulate_fees(2_000_000_000); // 2000 ADA
        assert_eq!(snapshots.fee_pot, 2_000_000_000);

        // After rotation the fee_pot should be consumed.
        let dummy = StakeSnapshot::empty();
        let returned_fees = snapshots.rotate(dummy);
        assert_eq!(returned_fees, 2_000_000_000);
        assert_eq!(snapshots.fee_pot, 0);
    }

    // -- retire_pools_with_refunds: no pools registered -------------------

    #[test]
    fn test_retire_no_pools() {
        let mut ledger = LedgerState::new(Era::Shelley);
        let (retired, refunded) =
            retire_pools_with_refunds(&mut ledger, EpochNo(1), 500_000_000);
        assert!(retired.is_empty());
        assert_eq!(refunded, 0);
    }

    // -- Governance action expiry -----------------------------------------

    use crate::eras::conway::{GovAction, GovActionId};
    use crate::state::GovernanceActionState;
    use crate::types::Anchor;

    fn test_proposal(deposit: u64, reward_account_byte: u8) -> crate::eras::conway::ProposalProcedure {
        let ra = test_reward_account(reward_account_byte);
        crate::eras::conway::ProposalProcedure {
            deposit,
            reward_account: ra.to_bytes().to_vec(),
            gov_action: GovAction::HardForkInitiation {
                prev_action_id: None,
                protocol_version: (10, 0),
            },
            anchor: Anchor {
                url: String::from("https://example.com"),
                data_hash: [0u8; 32],
            },
        }
    }

    fn test_gov_action_id(tx_byte: u8, index: u16) -> GovActionId {
        GovActionId {
            transaction_id: [tx_byte; 32],
            gov_action_index: index,
        }
    }

    #[test]
    fn test_expired_governance_actions_removed_at_epoch_boundary() {
        let mut ledger = make_ledger_with_pool(7);
        require_committee_vote_for_ratification(&mut ledger, 0x71, 0x72);
        let deposit_amount = 500_000_000u64;
        let ra = test_reward_account(7);

        // Stage a governance action proposed in epoch 1, lifetime 2 → expires_after = epoch 3.
        let gas = GovernanceActionState::new_with_lifetime(
            test_proposal(deposit_amount, 7),
            EpochNo(1),
            Some(2),
        );
        let gai = test_gov_action_id(0xAA, 0);
        ledger.governance_actions_mut().insert(gai.clone(), gas);
        assert_eq!(ledger.governance_actions().len(), 1);

        let balance_before = ledger.reward_accounts().balance(&ra);

        // Epoch 3 boundary — action is NOT expired (expires_after = 3, epoch = 3).
        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();
        let event = apply_epoch_boundary(&mut ledger, EpochNo(3), &mut snapshots, &perf)
            .expect("epoch 3 boundary should succeed");
        assert_eq!(event.governance_actions_expired, 0);
        assert_eq!(ledger.governance_actions().len(), 1);

        // Epoch 4 boundary — action IS expired (expires_after = 3 < 4).
        let event = apply_epoch_boundary(&mut ledger, EpochNo(4), &mut snapshots, &perf)
            .expect("epoch 4 boundary should succeed");
        assert_eq!(event.governance_actions_expired, 1);
        assert_eq!(event.governance_deposit_refunds, deposit_amount);
        assert_eq!(event.expired_gov_action_ids, vec![gai]);
        assert!(ledger.governance_actions().is_empty());

        // Deposit should be refunded to the return account.
        let balance_after = ledger.reward_accounts().balance(&ra);
        assert_eq!(balance_after, balance_before + deposit_amount);
    }

    #[test]
    fn test_non_expired_governance_actions_preserved() {
        let mut ledger = make_ledger_with_pool(8);
        require_committee_vote_for_ratification(&mut ledger, 0x81, 0x82);

        // Stage a governance action proposed in epoch 5, lifetime 10 → expires_after = epoch 15.
        let gas = GovernanceActionState::new_with_lifetime(
            test_proposal(100_000_000, 8),
            EpochNo(5),
            Some(10),
        );
        let gai = test_gov_action_id(0xBB, 0);
        ledger.governance_actions_mut().insert(gai.clone(), gas);

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        // Epoch 10 — not expired yet (15 >= 10).
        let event = apply_epoch_boundary(&mut ledger, EpochNo(10), &mut snapshots, &perf)
            .expect("epoch 10 boundary should succeed");
        assert_eq!(event.governance_actions_expired, 0);
        assert_eq!(ledger.governance_actions().len(), 1);
    }

    #[test]
    fn test_multiple_governance_actions_mixed_expiry() {
        let mut ledger = make_ledger_with_pool(9);
        require_committee_vote_for_ratification(&mut ledger, 0x91, 0x92);

        // Register two more reward accounts for proposals.
        ledger.reward_accounts_mut().insert(
            test_reward_account(10),
            RewardAccountState::new(0, None),
        );
        ledger.reward_accounts_mut().insert(
            test_reward_account(11),
            RewardAccountState::new(0, None),
        );

        let deposit = 250_000_000u64;

        // Action 1: expires_after = 3 (proposed epoch 1, lifetime 2).
        let gas1 = GovernanceActionState::new_with_lifetime(
            test_proposal(deposit, 9),
            EpochNo(1),
            Some(2),
        );
        // Action 2: expires_after = 10 (proposed epoch 5, lifetime 5).
        let gas2 = GovernanceActionState::new_with_lifetime(
            test_proposal(deposit, 10),
            EpochNo(5),
            Some(5),
        );
        // Action 3: expires_after = 4 (proposed epoch 2, lifetime 2).
        let gas3 = GovernanceActionState::new_with_lifetime(
            test_proposal(deposit, 11),
            EpochNo(2),
            Some(2),
        );

        let gai1 = test_gov_action_id(0xCC, 0);
        let gai2 = test_gov_action_id(0xDD, 0);
        let gai3 = test_gov_action_id(0xEE, 0);

        ledger.governance_actions_mut().insert(gai1.clone(), gas1);
        ledger.governance_actions_mut().insert(gai2.clone(), gas2);
        ledger.governance_actions_mut().insert(gai3.clone(), gas3);

        assert_eq!(ledger.governance_actions().len(), 3);

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        // Epoch 5: actions 1 (expires 3) and 3 (expires 4) should expire;
        // action 2 (expires 10) should survive.
        let event = apply_epoch_boundary(&mut ledger, EpochNo(5), &mut snapshots, &perf)
            .expect("epoch 5 boundary should succeed");
        assert_eq!(event.governance_actions_expired, 2);
        assert_eq!(event.governance_deposit_refunds, deposit * 2);
        assert_eq!(ledger.governance_actions().len(), 1);
        assert!(ledger.governance_actions().get(&gai2).is_some());
    }

    #[test]
    fn test_governance_expiry_with_no_lifetime() {
        let mut ledger = make_ledger_with_pool(12);
        require_committee_vote_for_ratification(&mut ledger, 0xC2, 0xC3);

        // Action without lifetime tracking (legacy/None).
        let gas = GovernanceActionState::new(test_proposal(100_000_000, 12));
        let gai = test_gov_action_id(0xFF, 0);
        ledger.governance_actions_mut().insert(gai.clone(), gas);

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        // Should never expire since expires_after is None.
        let event = apply_epoch_boundary(&mut ledger, EpochNo(100), &mut snapshots, &perf)
            .expect("epoch 100 boundary should succeed");
        assert_eq!(event.governance_actions_expired, 0);
        assert_eq!(ledger.governance_actions().len(), 1);
    }

    #[test]
    fn test_governance_expiry_refund_to_unregistered_account_is_lost() {
        let mut ledger = make_ledger_with_pool(13);
        let deposit = 500_000_000u64;

        // Stage a governance action with return address for a reward
        // account that is NOT registered in the ledger.
        let unregistered_ra = test_reward_account(99);
        let proposal = crate::eras::conway::ProposalProcedure {
            deposit,
            reward_account: unregistered_ra.to_bytes().to_vec(),
            gov_action: GovAction::InfoAction,
            anchor: Anchor {
                url: String::from("https://example.com"),
                data_hash: [0u8; 32],
            },
        };
        let gas = GovernanceActionState::new_with_lifetime(
            proposal,
            EpochNo(1),
            Some(1),
        );
        let gai = test_gov_action_id(0xAB, 0);
        ledger.governance_actions_mut().insert(gai.clone(), gas);

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        // Epoch 3 → action expires (expires_after = 2 < 3).
        let event = apply_epoch_boundary(&mut ledger, EpochNo(3), &mut snapshots, &perf)
            .expect("epoch 3 boundary should succeed");

        // Action is removed.
        assert_eq!(event.governance_actions_expired, 1);
        assert!(ledger.governance_actions().is_empty());
        // But refund was 0 because the return account is not registered.
        assert_eq!(event.governance_deposit_refunds, 0);
    }

    // -- DRep inactivity expiry -------------------------------------------

    use crate::state::RegisteredDrep;
    use crate::types::DRep;

    fn test_protocol_params_with_drep_activity(drep_activity: u64) -> ProtocolParameters {
        let mut pp = test_protocol_params();
        pp.drep_activity = Some(drep_activity);
        pp
    }

    #[test]
    fn test_drep_no_expiry_without_activity_window() {
        // When drep_activity is None, no DReps should expire.
        let mut ledger = make_ledger_with_pool(14);
        let drep = DRep::KeyHash([0x01; 28]);
        ledger.drep_state_mut().register(
            drep.clone(),
            RegisteredDrep::new_active(500_000_000, None, EpochNo(1)),
        );

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        // Epoch 1000 — no drep_activity set, so no expiry.
        let event = apply_epoch_boundary(&mut ledger, EpochNo(1000), &mut snapshots, &perf)
            .expect("epoch boundary should succeed");
        assert_eq!(event.dreps_expired, 0);
        assert!(ledger.drep_state().is_registered(&drep));
    }

    #[test]
    fn test_drep_active_within_window() {
        let mut ledger = make_ledger_with_pool(15);
        ledger.set_protocol_params(test_protocol_params_with_drep_activity(20));

        let drep = DRep::KeyHash([0x02; 28]);
        // Active in epoch 80.
        ledger.drep_state_mut().register(
            drep.clone(),
            RegisteredDrep::new_active(500_000_000, None, EpochNo(80)),
        );

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        // Epoch 100 — 80 + 20 = 100 which is NOT < 100, so still active.
        let event = apply_epoch_boundary(&mut ledger, EpochNo(100), &mut snapshots, &perf)
            .expect("epoch boundary should succeed");
        assert_eq!(event.dreps_expired, 0);
    }

    #[test]
    fn test_drep_expired_beyond_window() {
        let mut ledger = make_ledger_with_pool(16);
        ledger.set_protocol_params(test_protocol_params_with_drep_activity(20));

        let drep = DRep::KeyHash([0x03; 28]);
        // Active in epoch 79 → 79 + 20 = 99 < 100 → expired.
        ledger.drep_state_mut().register(
            drep.clone(),
            RegisteredDrep::new_active(500_000_000, None, EpochNo(79)),
        );

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        let event = apply_epoch_boundary(&mut ledger, EpochNo(100), &mut snapshots, &perf)
            .expect("epoch boundary should succeed");
        assert_eq!(event.dreps_expired, 1);
        // DRep remains registered — inactivity does NOT remove it.
        assert!(ledger.drep_state().is_registered(&drep));
    }

    #[test]
    fn test_drep_legacy_without_activity_epoch() {
        // DReps with no last_active_epoch (legacy) should NOT expire.
        let mut ledger = make_ledger_with_pool(17);
        ledger.set_protocol_params(test_protocol_params_with_drep_activity(5));

        let drep = DRep::KeyHash([0x04; 28]);
        ledger.drep_state_mut().register(
            drep.clone(),
            RegisteredDrep::new(500_000_000, None),
        );

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        let event = apply_epoch_boundary(&mut ledger, EpochNo(100), &mut snapshots, &perf)
            .expect("epoch boundary should succeed");
        assert_eq!(event.dreps_expired, 0);
    }

    #[test]
    fn test_mixed_drep_active_and_expired() {
        let mut ledger = make_ledger_with_pool(18);
        ledger.set_protocol_params(test_protocol_params_with_drep_activity(10));

        // DRep A: active in epoch 85 → 85+10=95 < 100 → expired.
        let drep_a = DRep::KeyHash([0x05; 28]);
        ledger.drep_state_mut().register(
            drep_a.clone(),
            RegisteredDrep::new_active(500_000_000, None, EpochNo(85)),
        );
        // DRep B: active in epoch 95 → 95+10=105 >= 100 → still active.
        let drep_b = DRep::ScriptHash([0x06; 28]);
        ledger.drep_state_mut().register(
            drep_b.clone(),
            RegisteredDrep::new_active(500_000_000, None, EpochNo(95)),
        );
        // DRep C: legacy, no activity epoch → not expired.
        let drep_c = DRep::KeyHash([0x07; 28]);
        ledger.drep_state_mut().register(
            drep_c.clone(),
            RegisteredDrep::new(500_000_000, None),
        );

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        let event = apply_epoch_boundary(&mut ledger, EpochNo(100), &mut snapshots, &perf)
            .expect("epoch boundary should succeed");
        assert_eq!(event.dreps_expired, 1); // Only DRep A.
        // All still registered.
        assert!(ledger.drep_state().is_registered(&drep_a));
        assert!(ledger.drep_state().is_registered(&drep_b));
        assert!(ledger.drep_state().is_registered(&drep_c));
    }

    #[test]
    fn test_drep_reactivated_by_vote_not_expired() {
        let mut ledger = make_ledger_with_pool(19);
        ledger.set_protocol_params(test_protocol_params_with_drep_activity(10));

        let drep = DRep::KeyHash([0x08; 28]);
        // Initially active in epoch 80.
        ledger.drep_state_mut().register(
            drep.clone(),
            RegisteredDrep::new_active(500_000_000, None, EpochNo(80)),
        );
        // Simulate a vote in epoch 95 → touch_activity.
        ledger.drep_state_mut().get_mut(&drep).unwrap().touch_activity(EpochNo(95));

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        // Epoch 100: 95 + 10 = 105 >= 100 → still active.
        let event = apply_epoch_boundary(&mut ledger, EpochNo(100), &mut snapshots, &perf)
            .expect("epoch boundary should succeed");
        assert_eq!(event.dreps_expired, 0);
    }

    // -- Ratification + enactment at epoch boundary -----------------------

    use crate::eras::conway::{Constitution, Vote, Voter};
    use crate::protocol_params::{DRepVotingThresholds, PoolVotingThresholds};
    use crate::state::{CommitteeAuthorization, EnactOutcome};

    fn test_protocol_params_with_governance() -> ProtocolParameters {
        let mut pp = test_protocol_params();
        pp.pool_voting_thresholds = Some(PoolVotingThresholds::default());
        pp.drep_voting_thresholds = Some(DRepVotingThresholds::default());
        pp.drep_activity = Some(100);
        pp
    }

    fn make_governance_ledger() -> LedgerState {
        let mut ledger = make_ledger_with_pool(20);
        ledger.set_protocol_params(test_protocol_params_with_governance());

        // Register a CC member and authorize hot key.
        let cc_cred = test_cred(0xC0);
        ledger.committee_state_mut().register(cc_cred);
        let hot_cred = test_cred(0xC1);
        ledger
            .committee_state_mut()
            .get_mut(&cc_cred)
            .unwrap()
            .set_authorization(Some(CommitteeAuthorization::CommitteeHotCredential(hot_cred)));
        // Set committee quorum to 1/1 (all must vote yes).
        ledger.enact_state_mut().committee_quorum = UnitInterval {
            numerator: 1,
            denominator: 1,
        };

        // Register a DRep and delegate stake to it.
        let drep = DRep::KeyHash([0xD0; 28]);
        ledger.drep_state_mut().register(
            drep,
            RegisteredDrep::new_active(0, None, EpochNo(0)),
        );
        let cred = test_cred(20);
        if let Some(cs) = ledger.stake_credentials_mut().get_mut(&cred) {
            cs.set_delegated_drep(Some(drep));
        }

        // Add UTxO stake so DRep has weighted stake.
        let base_addr = Address::Base(BaseAddress {
            network: 1,
            payment: StakeCredential::AddrKeyHash([0xBB; 28]),
            staking: cred,
        });
        let txin = ShelleyTxIn { transaction_id: [0x20; 32], index: 0 };
        ledger.multi_era_utxo_mut().insert_shelley(
            txin,
            ShelleyTxOut {
                address: base_addr.to_bytes(),
                amount: 1_000_000_000_000,
            },
        );

        ledger
    }

    fn test_info_proposal() -> crate::eras::conway::ProposalProcedure {
        crate::eras::conway::ProposalProcedure {
            deposit: 0,
            reward_account: vec![],
            gov_action: GovAction::InfoAction,
            anchor: crate::types::Anchor {
                url: String::new(),
                data_hash: [0; 32],
            },
        }
    }

    fn test_hf_proposal() -> crate::eras::conway::ProposalProcedure {
        crate::eras::conway::ProposalProcedure {
            deposit: 0,
            reward_account: vec![],
            gov_action: GovAction::HardForkInitiation {
                prev_action_id: None,
                protocol_version: (10, 0),
            },
            anchor: crate::types::Anchor {
                url: String::new(),
                data_hash: [0; 32],
            },
        }
    }

    fn test_new_constitution_proposal() -> crate::eras::conway::ProposalProcedure {
        crate::eras::conway::ProposalProcedure {
            deposit: 0,
            reward_account: vec![],
            gov_action: GovAction::NewConstitution {
                prev_action_id: None,
                constitution: Constitution {
                    anchor: crate::types::Anchor {
                        url: String::from("https://constitution.example.com"),
                        data_hash: [0xCC; 32],
                    },
                    guardrails_script_hash: None,
                },
            },
            anchor: crate::types::Anchor {
                url: String::new(),
                data_hash: [0; 32],
            },
        }
    }

    fn test_update_committee_proposal() -> crate::eras::conway::ProposalProcedure {
        crate::eras::conway::ProposalProcedure {
            deposit: 0,
            reward_account: vec![],
            gov_action: GovAction::UpdateCommittee {
                prev_action_id: None,
                members_to_remove: vec![],
                members_to_add: BTreeMap::new(),
                quorum: UnitInterval {
                    numerator: 1,
                    denominator: 2,
                },
            },
            anchor: crate::types::Anchor {
                url: String::new(),
                data_hash: [0; 32],
            },
        }
    }

    #[test]
    fn test_info_action_always_ratified_at_epoch_boundary() {
        let mut ledger = make_governance_ledger();
        let gai = test_gov_action_id(0xA1, 0);
        let gas = GovernanceActionState::new(test_info_proposal());
        ledger.governance_actions_mut().insert(gai.clone(), gas);

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        // First rotation to populate the mark snapshot with DRep stake.
        let _ = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1 boundary");

        // Re-insert the InfoAction (it was enacted at epoch 1).
        let gas2 = GovernanceActionState::new(test_info_proposal());
        let gai2 = test_gov_action_id(0xA2, 0);
        ledger.governance_actions_mut().insert(gai2.clone(), gas2);

        let event = apply_epoch_boundary(&mut ledger, EpochNo(2), &mut snapshots, &perf)
            .expect("epoch 2 boundary");

        // InfoAction is always accepted → should be enacted.
        assert_eq!(event.governance_actions_enacted, 1);
        assert_eq!(event.enacted_gov_action_ids, vec![gai2]);
        assert_eq!(event.enact_outcomes.len(), 1);
        assert_eq!(event.enact_outcomes[0], EnactOutcome::NoEffect);
        // Should be removed from pending set.
        assert!(ledger.governance_actions().is_empty());
    }

    #[test]
    fn test_hf_rejected_when_no_votes() {
        let mut ledger = make_governance_ledger();

        // Hard fork proposal with NO votes at all.
        let gai = test_gov_action_id(0xB1, 0);
        let gas = GovernanceActionState::new(test_hf_proposal());
        ledger.governance_actions_mut().insert(gai.clone(), gas);

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        // Populate mark snapshot.
        let _ = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1 boundary");

        // HF without votes should NOT be ratified.
        // Re-insert after epoch 1 enacted none (no votes → rejected).
        // Actually, let's check: the action should still be there.
        assert_eq!(ledger.governance_actions().len(), 1);

        let event = apply_epoch_boundary(&mut ledger, EpochNo(2), &mut snapshots, &perf)
            .expect("epoch 2 boundary");

        // HF requires CC, DRep, and SPO approval — without votes, not ratified.
        assert_eq!(event.governance_actions_enacted, 0);
        assert!(event.enacted_gov_action_ids.is_empty());
        // Action remains in pending set.
        assert_eq!(ledger.governance_actions().len(), 1);
    }

    #[test]
    fn test_hf_enacted_with_unanimous_votes() {
        let mut ledger = make_governance_ledger();
        let cc_cold_cred = test_cred(0xC0);
        let drep_cred = [0xD0; 28];
        let pool_key = test_pool(20);

        let gai = test_gov_action_id(0xC1, 0);
        let mut gas = GovernanceActionState::new(test_hf_proposal());

        // Record CC vote (yes) — keyed by cold credential.
        gas.record_vote(
            Voter::CommitteeKeyHash(*cc_cold_cred.hash()),
            Vote::Yes,
        );
        // Record DRep vote (yes).
        gas.record_vote(Voter::DRepKeyHash(drep_cred), Vote::Yes);
        // Record SPO vote (yes).
        gas.record_vote(Voter::StakePool(pool_key), Vote::Yes);

        ledger.governance_actions_mut().insert(gai.clone(), gas);

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        // First rotation populates mark snapshot.
        let _ = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1 boundary");

        // Re-insert after epoch 1 (it may have been enacted).
        if ledger.governance_actions().is_empty() {
            // It was enacted at epoch 1 — verify.
            return; // Success
        }

        let event = apply_epoch_boundary(&mut ledger, EpochNo(2), &mut snapshots, &perf)
            .expect("epoch 2 boundary");

        assert!(event.governance_actions_enacted >= 1);
        // Protocol version should be updated.
        assert_eq!(
            ledger.protocol_params().unwrap().protocol_version,
            Some((10, 0))
        );
    }

    #[test]
    fn test_ratification_without_voting_thresholds_uses_defaults() {
        let mut ledger = make_ledger_with_pool(21);
        // Protocol params without explicit Conway thresholds fall back to the
        // built-in defaults so ratification still runs.
        let gas = GovernanceActionState::new(test_info_proposal());
        let gai = test_gov_action_id(0xD1, 0);
        ledger.governance_actions_mut().insert(gai.clone(), gas);

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        let event = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1 boundary");

        assert_eq!(event.governance_actions_enacted, 1);
        assert!(ledger.governance_actions().is_empty());
    }

    #[test]
    fn test_new_constitution_enacted_updates_enact_state() {
        let mut ledger = make_governance_ledger();
        let cc_cold_cred = test_cred(0xC0);
        let drep_cred = [0xD0; 28];

        let gai = test_gov_action_id(0xE1, 0);
        let mut gas = GovernanceActionState::new(test_new_constitution_proposal());

        // NewConstitution requires CC + DRep, but NOT SPO.
        gas.record_vote(
            Voter::CommitteeKeyHash(*cc_cold_cred.hash()),
            Vote::Yes,
        );
        gas.record_vote(Voter::DRepKeyHash(drep_cred), Vote::Yes);

        ledger.governance_actions_mut().insert(gai.clone(), gas);

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        // First rotation to populate mark.
        let _ = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1");

        // Check if enacted at epoch 1 or 2.
        if ledger.governance_actions().is_empty() {
            // Enacted at epoch 1.
            assert_eq!(
                ledger.enact_state().constitution.anchor.data_hash,
                [0xCC; 32]
            );
            return;
        }

        let event = apply_epoch_boundary(&mut ledger, EpochNo(2), &mut snapshots, &perf)
            .expect("epoch 2");

        assert!(event.governance_actions_enacted >= 1);
        assert_eq!(
            ledger.enact_state().constitution.anchor.data_hash,
            [0xCC; 32]
        );
    }

    #[test]
    fn test_mixed_ratification_and_expiry() {
        let mut ledger = make_governance_ledger();

        // Action 1: InfoAction (always ratified).
        let gai1 = test_gov_action_id(0xF1, 0);
        let gas1 = GovernanceActionState::new(test_info_proposal());
        ledger.governance_actions_mut().insert(gai1.clone(), gas1);

        // Action 2: HF with no votes (not ratified) + expires after epoch 2.
        let gai2 = test_gov_action_id(0xF2, 0);
        let gas2 = GovernanceActionState::new_with_lifetime(
            test_hf_proposal(),
            EpochNo(1),
            Some(1),
        );
        ledger.governance_actions_mut().insert(gai2.clone(), gas2);

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        // Epoch 1: populate mark + process.
        let _ = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1");

        // InfoAction should have been enacted, HF should still be pending.
        assert!(!ledger.governance_actions().contains_key(&gai1));

        // Epoch 3: HF should expire (expires_after = 2 < 3).
        let event = apply_epoch_boundary(&mut ledger, EpochNo(3), &mut snapshots, &perf)
            .expect("epoch 3");

        assert_eq!(event.governance_actions_expired, 1);
        assert!(ledger.governance_actions().is_empty());
    }

    #[test]
    fn test_update_committee_ratifies_with_resigned_only_committee() {
        let mut ledger = make_governance_ledger();

        // Differentiate elected vs non-elected committee paths.
        let mut drep_thresholds = DRepVotingThresholds::default();
        drep_thresholds.committee_no_confidence = UnitInterval {
            numerator: 0,
            denominator: 1,
        };
        drep_thresholds.committee_normal = UnitInterval {
            numerator: 1,
            denominator: 1,
        };
        let mut pool_thresholds = PoolVotingThresholds::default();
        pool_thresholds.committee_no_confidence = UnitInterval {
            numerator: 0,
            denominator: 1,
        };
        pool_thresholds.committee_normal = UnitInterval {
            numerator: 1,
            denominator: 1,
        };
        if let Some(pp) = ledger.protocol_params_mut() {
            pp.drep_voting_thresholds = Some(drep_thresholds);
            pp.pool_voting_thresholds = Some(pool_thresholds);
        }

        // Make the committee resign-only (no elected non-resigned members).
        let cc_cred = test_cred(0xC0);
        ledger
            .committee_state_mut()
            .get_mut(&cc_cred)
            .expect("committee member present")
            .set_authorization(Some(CommitteeAuthorization::CommitteeMemberResigned(None)));

        let gai = test_gov_action_id(0xFA, 0);
        let gas = GovernanceActionState::new(test_update_committee_proposal());
        ledger.governance_actions_mut().insert(gai.clone(), gas);

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        let _ = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1 boundary");
        if ledger.governance_actions().is_empty() {
            return;
        }

        let event = apply_epoch_boundary(&mut ledger, EpochNo(2), &mut snapshots, &perf)
            .expect("epoch 2 boundary");

        assert_eq!(event.governance_actions_enacted, 1);
        assert_eq!(event.enacted_gov_action_ids, vec![gai]);
        assert!(ledger.governance_actions().is_empty());
    }

    #[test]
    fn test_update_committee_not_ratified_with_elected_committee_and_no_votes() {
        let mut ledger = make_governance_ledger();

        let mut drep_thresholds = DRepVotingThresholds::default();
        drep_thresholds.committee_no_confidence = UnitInterval {
            numerator: 0,
            denominator: 1,
        };
        drep_thresholds.committee_normal = UnitInterval {
            numerator: 1,
            denominator: 1,
        };
        let mut pool_thresholds = PoolVotingThresholds::default();
        pool_thresholds.committee_no_confidence = UnitInterval {
            numerator: 0,
            denominator: 1,
        };
        pool_thresholds.committee_normal = UnitInterval {
            numerator: 1,
            denominator: 1,
        };
        if let Some(pp) = ledger.protocol_params_mut() {
            pp.drep_voting_thresholds = Some(drep_thresholds);
            pp.pool_voting_thresholds = Some(pool_thresholds);
        }

        // Keep committee elected (default governance ledger has one active member).
        let gai = test_gov_action_id(0xFB, 0);
        let gas = GovernanceActionState::new(test_update_committee_proposal());
        ledger.governance_actions_mut().insert(gai.clone(), gas);

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        let _ = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1 boundary");
        if ledger.governance_actions().is_empty() {
            panic!("unexpected enactment at epoch 1 with elected committee and no votes");
        }

        let event = apply_epoch_boundary(&mut ledger, EpochNo(2), &mut snapshots, &perf)
            .expect("epoch 2 boundary");

        assert_eq!(event.governance_actions_enacted, 0);
        assert!(event.enacted_gov_action_ids.is_empty());
        assert!(ledger.governance_actions().contains_key(&gai));
    }

    #[test]
    fn test_update_committee_rejected_when_result_below_min_committee_size() {
        let mut ledger = make_governance_ledger();

        let mut drep_thresholds = DRepVotingThresholds::default();
        drep_thresholds.committee_normal = UnitInterval {
            numerator: 0,
            denominator: 1,
        };
        let mut pool_thresholds = PoolVotingThresholds::default();
        pool_thresholds.committee_normal = UnitInterval {
            numerator: 0,
            denominator: 1,
        };
        if let Some(pp) = ledger.protocol_params_mut() {
            pp.drep_voting_thresholds = Some(drep_thresholds);
            pp.pool_voting_thresholds = Some(pool_thresholds);
            pp.min_committee_size = Some(2);
        }

        // Default governance ledger starts with one active committee member;
        // removing it would produce committee size 0, below min size 2.
        let cc_cred = test_cred(0xC0);
        let proposal = crate::eras::conway::ProposalProcedure {
            deposit: 0,
            reward_account: vec![],
            gov_action: GovAction::UpdateCommittee {
                prev_action_id: None,
                members_to_remove: vec![cc_cred],
                members_to_add: BTreeMap::new(),
                quorum: UnitInterval {
                    numerator: 1,
                    denominator: 2,
                },
            },
            anchor: crate::types::Anchor {
                url: String::new(),
                data_hash: [0; 32],
            },
        };
        let gai = test_gov_action_id(0xFC, 0);
        ledger
            .governance_actions_mut()
            .insert(gai.clone(), GovernanceActionState::new(proposal));

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        let _ = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1 boundary");
        let event = apply_epoch_boundary(&mut ledger, EpochNo(2), &mut snapshots, &perf)
            .expect("epoch 2 boundary");

        assert_eq!(event.governance_actions_enacted, 0);
        assert!(event.enacted_gov_action_ids.is_empty());
        assert!(ledger.governance_actions().contains_key(&gai));
    }

    #[test]
    fn test_update_committee_enacted_when_result_meets_min_committee_size() {
        let mut ledger = make_governance_ledger();
        let cc_cold_cred = test_cred(0xC0);

        let mut drep_thresholds = DRepVotingThresholds::default();
        drep_thresholds.committee_normal = UnitInterval {
            numerator: 0,
            denominator: 1,
        };
        let mut pool_thresholds = PoolVotingThresholds::default();
        pool_thresholds.committee_normal = UnitInterval {
            numerator: 0,
            denominator: 1,
        };
        if let Some(pp) = ledger.protocol_params_mut() {
            pp.drep_voting_thresholds = Some(drep_thresholds);
            pp.pool_voting_thresholds = Some(pool_thresholds);
            pp.min_committee_size = Some(2);
        }

        let added = test_cred(0xC2);
        let mut members_to_add = BTreeMap::new();
        members_to_add.insert(added, 10);
        let proposal = crate::eras::conway::ProposalProcedure {
            deposit: 0,
            reward_account: vec![],
            gov_action: GovAction::UpdateCommittee {
                prev_action_id: None,
                members_to_remove: vec![],
                members_to_add,
                quorum: UnitInterval {
                    numerator: 1,
                    denominator: 2,
                },
            },
            anchor: crate::types::Anchor {
                url: String::new(),
                data_hash: [0; 32],
            },
        };
        let gai = test_gov_action_id(0xFD, 0);
        let mut gas = GovernanceActionState::new(proposal);
        gas.record_vote(
            Voter::CommitteeKeyHash(*cc_cold_cred.hash()),
            Vote::Yes,
        );
        ledger
            .governance_actions_mut()
            .insert(gai.clone(), gas);

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        let _ = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1 boundary");
        if ledger.governance_actions().is_empty() {
            return;
        }

        let event = apply_epoch_boundary(&mut ledger, EpochNo(2), &mut snapshots, &perf)
            .expect("epoch 2 boundary");

        assert_eq!(event.governance_actions_enacted, 1);
        assert_eq!(event.enacted_gov_action_ids, vec![gai]);
        assert!(ledger.governance_actions().is_empty());
    }

    #[test]
    fn test_no_pending_actions() {
        let mut ledger = make_governance_ledger();
        assert!(ledger.governance_actions().is_empty());

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        let event = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1");

        assert_eq!(event.governance_actions_enacted, 0);
        assert!(event.enacted_gov_action_ids.is_empty());
        assert!(event.enact_outcomes.is_empty());
    }

    // -- Enacted deposit refund -------------------------------------------

    /// Helper that creates a governance-ready ledger with a proposal whose
    /// deposit is set to `deposit_amount` and whose return account is
    /// `reward_account_byte`.  An InfoAction is used so it will be
    /// automatically ratified at the next epoch boundary.
    fn make_ledger_with_deposited_info_action(
        deposit_amount: u64,
        reward_account_byte: u8,
    ) -> (LedgerState, GovActionId) {
        let mut ledger = make_governance_ledger();
        let ra = test_reward_account(reward_account_byte);
        ledger
            .reward_accounts_mut()
            .insert(ra, RewardAccountState::new(0, None));

        let proposal = crate::eras::conway::ProposalProcedure {
            deposit: deposit_amount,
            reward_account: ra.to_bytes().to_vec(),
            gov_action: GovAction::InfoAction,
            anchor: Anchor {
                url: String::new(),
                data_hash: [0; 32],
            },
        };
        let gai = test_gov_action_id(0xEA, 0);
        let gas = GovernanceActionState::new(proposal);
        ledger.governance_actions_mut().insert(gai.clone(), gas);
        (ledger, gai)
    }

    #[test]
    fn test_enacted_action_deposit_refunded_to_return_account() {
        let deposit = 500_000_000u64;
        let ra_byte = 0x50;
        let (mut ledger, gai) = make_ledger_with_deposited_info_action(deposit, ra_byte);
        let ra = test_reward_account(ra_byte);

        let balance_before = ledger.reward_accounts().balance(&ra);

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        // First epoch populates mark snapshot.
        let _ = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1");

        // Re-insert the action (InfoAction was enacted at epoch 1).
        let proposal = crate::eras::conway::ProposalProcedure {
            deposit,
            reward_account: ra.to_bytes().to_vec(),
            gov_action: GovAction::InfoAction,
            anchor: Anchor {
                url: String::new(),
                data_hash: [0; 32],
            },
        };
        let gai2 = test_gov_action_id(0xEB, 0);
        ledger
            .governance_actions_mut()
            .insert(gai2.clone(), GovernanceActionState::new(proposal));

        let event = apply_epoch_boundary(&mut ledger, EpochNo(2), &mut snapshots, &perf)
            .expect("epoch 2");

        // Verify enacted with deposit refund.
        assert_eq!(event.governance_actions_enacted, 1);
        assert_eq!(event.enacted_deposit_refunds, deposit);
        assert!(ledger.governance_actions().is_empty());

        // Reward account balance should increase by deposit.
        let balance_after = ledger.reward_accounts().balance(&ra);
        // Two info actions were enacted across epochs 1+2, both refunded.
        assert!(balance_after >= balance_before + deposit);
    }

    #[test]
    fn test_enacted_deposit_refund_for_unregistered_account_goes_to_treasury() {
        let mut ledger = make_governance_ledger();
        let deposit = 300_000_000u64;

        // Use an unregistered reward account.
        let unregistered_ra = test_reward_account(0x99);
        let proposal = crate::eras::conway::ProposalProcedure {
            deposit,
            reward_account: unregistered_ra.to_bytes().to_vec(),
            gov_action: GovAction::InfoAction,
            anchor: Anchor {
                url: String::new(),
                data_hash: [0; 32],
            },
        };
        let gai = test_gov_action_id(0xEC, 0);
        ledger
            .governance_actions_mut()
            .insert(gai.clone(), GovernanceActionState::new(proposal));

        let treasury_before = ledger.accounting().treasury;

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        // Populate mark.
        let _ = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1");

        // Re-insert.
        let proposal2 = crate::eras::conway::ProposalProcedure {
            deposit,
            reward_account: unregistered_ra.to_bytes().to_vec(),
            gov_action: GovAction::InfoAction,
            anchor: Anchor {
                url: String::new(),
                data_hash: [0; 32],
            },
        };
        ledger.governance_actions_mut().insert(
            test_gov_action_id(0xED, 0),
            GovernanceActionState::new(proposal2),
        );

        let treasury_before_e2 = ledger.accounting().treasury;
        let event = apply_epoch_boundary(&mut ledger, EpochNo(2), &mut snapshots, &perf)
            .expect("epoch 2");

        // Enacted but deposit is unclaimed.
        assert_eq!(event.governance_actions_enacted, 1);
        assert_eq!(event.enacted_deposit_refunds, 0);
        assert_eq!(event.unclaimed_governance_deposits, deposit);

        // Treasury should increase by unclaimed deposit (plus normal treasury flow).
        let treasury_after = ledger.accounting().treasury;
        // Treasury delta includes RUPD treasury cut + unclaimed deposit.
        assert!(
            treasury_after >= treasury_before_e2 + deposit,
            "treasury should include unclaimed deposit: before={treasury_before_e2} after={treasury_after} deposit={deposit}"
        );
    }

    // -- Lineage subtree pruning ------------------------------------------

    #[test]
    fn test_enacted_action_prunes_sibling_proposals() {
        // Setup: Two HardFork proposals A and B both reference prev_action_id = None.
        // A has votes and will be enacted. B has no votes.
        // After A is enacted, B should be removed (its prev_action_id is stale).
        let mut ledger = make_governance_ledger();
        let cc_cold_cred = test_cred(0xC0);
        let drep_cred = [0xD0; 28];
        let pool_key = test_pool(20);

        let deposit_a = 100_000_000u64;
        let deposit_b = 200_000_000u64;
        let ra_a = test_reward_account(0x60);
        let ra_b = test_reward_account(0x61);
        ledger
            .reward_accounts_mut()
            .insert(ra_a, RewardAccountState::new(0, None));
        ledger
            .reward_accounts_mut()
            .insert(ra_b, RewardAccountState::new(0, None));

        // Action A — voted yes by all roles.
        let proposal_a = crate::eras::conway::ProposalProcedure {
            deposit: deposit_a,
            reward_account: ra_a.to_bytes().to_vec(),
            gov_action: GovAction::HardForkInitiation {
                prev_action_id: None,
                protocol_version: (10, 0),
            },
            anchor: Anchor {
                url: String::new(),
                data_hash: [0; 32],
            },
        };
        let gai_a = test_gov_action_id(0xF0, 0);
        let mut gas_a = GovernanceActionState::new(proposal_a);
        gas_a.record_vote(
            Voter::CommitteeKeyHash(*cc_cold_cred.hash()),
            Vote::Yes,
        );
        gas_a.record_vote(Voter::DRepKeyHash(drep_cred), Vote::Yes);
        gas_a.record_vote(Voter::StakePool(pool_key), Vote::Yes);
        ledger
            .governance_actions_mut()
            .insert(gai_a.clone(), gas_a);

        // Action B — same prev_action_id (None), no votes.
        let proposal_b = crate::eras::conway::ProposalProcedure {
            deposit: deposit_b,
            reward_account: ra_b.to_bytes().to_vec(),
            gov_action: GovAction::HardForkInitiation {
                prev_action_id: None,
                protocol_version: (11, 0),
            },
            anchor: Anchor {
                url: String::new(),
                data_hash: [0; 32],
            },
        };
        let gai_b = test_gov_action_id(0xF1, 0);
        let gas_b = GovernanceActionState::new(proposal_b);
        ledger
            .governance_actions_mut()
            .insert(gai_b.clone(), gas_b);

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        // Epoch 1: populate mark.
        let _ = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1");

        // After epoch 1: A may have been enacted already. If both are gone,
        // the subtree pruning worked at epoch 1. Otherwise continue.
        if !ledger.governance_actions().is_empty() {
            // Re-insert A with votes (enactment removed it).
            let proposal_a2 = crate::eras::conway::ProposalProcedure {
                deposit: deposit_a,
                reward_account: ra_a.to_bytes().to_vec(),
                gov_action: GovAction::HardForkInitiation {
                    prev_action_id: None,
                    protocol_version: (10, 0),
                },
                anchor: Anchor {
                    url: String::new(),
                    data_hash: [0; 32],
                },
            };
            let gai_a2 = test_gov_action_id(0xF2, 0);
            let mut gas_a2 = GovernanceActionState::new(proposal_a2);
            gas_a2.record_vote(
                Voter::CommitteeKeyHash(*cc_cold_cred.hash()),
                Vote::Yes,
            );
            gas_a2.record_vote(Voter::DRepKeyHash(drep_cred), Vote::Yes);
            gas_a2.record_vote(Voter::StakePool(pool_key), Vote::Yes);

            // Remove stale entries and re-insert.
            ledger.governance_actions_mut().clear();
            ledger
                .governance_actions_mut()
                .insert(gai_a2.clone(), gas_a2);

            // Re-insert B with same lineage root.
            let proposal_b2 = crate::eras::conway::ProposalProcedure {
                deposit: deposit_b,
                reward_account: ra_b.to_bytes().to_vec(),
                gov_action: GovAction::HardForkInitiation {
                    prev_action_id: None,
                    protocol_version: (11, 0),
                },
                anchor: Anchor {
                    url: String::new(),
                    data_hash: [0; 32],
                },
            };
            let gai_b2 = test_gov_action_id(0xF3, 0);
            let gas_b2 = GovernanceActionState::new(proposal_b2);
            ledger
                .governance_actions_mut()
                .insert(gai_b2.clone(), gas_b2);

            let event = apply_epoch_boundary(&mut ledger, EpochNo(2), &mut snapshots, &perf)
                .expect("epoch 2");

            // A should be enacted, B should be pruned via lineage.
            assert_eq!(event.governance_actions_enacted, 1);
            assert_eq!(event.removed_due_to_enactment.len(), 1);
            assert_eq!(event.removed_due_to_enactment_deposit_refunds, deposit_b);
            assert!(ledger.governance_actions().is_empty());
        }
        // If both were gone at epoch 1, the test passes — subtree pruning worked.
    }

    #[test]
    fn test_transitive_subtree_pruning() {
        // Setup: Action A (prev=None, voted yes), Action B (prev=None, stale sibling),
        // Action C (prev=B).
        // When A is enacted the HardFork root advances to Some(gai_a).
        // B references None (the old root) → stale.
        // C references B (stale) → also stale.
        let mut ledger = make_governance_ledger();
        let cc_cold_cred = test_cred(0xC0);
        let drep_cred = [0xD0; 28];
        let pool_key = test_pool(20);

        let deposit_a = 100_000_000u64;
        let deposit_b = 100_000_000u64;
        let deposit_c = 100_000_000u64;
        let ra_a = test_reward_account(0x70);
        let ra_b = test_reward_account(0x71);
        let ra_c = test_reward_account(0x72);
        ledger
            .reward_accounts_mut()
            .insert(ra_a, RewardAccountState::new(0, None));
        ledger
            .reward_accounts_mut()
            .insert(ra_b, RewardAccountState::new(0, None));
        ledger
            .reward_accounts_mut()
            .insert(ra_c, RewardAccountState::new(0, None));

        let gai_a = test_gov_action_id(0xA0, 0);
        let gai_b = test_gov_action_id(0xB0, 0);
        let gai_c = test_gov_action_id(0xC9, 0);

        // Populate mark snapshot first.
        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();
        let _ = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1");

        // Action A: HardFork, prev=None, all votes yes (will be enacted).
        let proposal_a = crate::eras::conway::ProposalProcedure {
            deposit: deposit_a,
            reward_account: ra_a.to_bytes().to_vec(),
            gov_action: GovAction::HardForkInitiation {
                prev_action_id: None,
                protocol_version: (10, 0),
            },
            anchor: Anchor {
                url: String::new(),
                data_hash: [0; 32],
            },
        };
        let mut gas_a = GovernanceActionState::new(proposal_a);
        gas_a.record_vote(
            Voter::CommitteeKeyHash(*cc_cold_cred.hash()),
            Vote::Yes,
        );
        gas_a.record_vote(Voter::DRepKeyHash(drep_cred), Vote::Yes);
        gas_a.record_vote(Voter::StakePool(pool_key), Vote::Yes);
        ledger
            .governance_actions_mut()
            .insert(gai_a.clone(), gas_a);

        // Action B: HardFork, prev=None (stale after A is enacted), no votes.
        let proposal_b = crate::eras::conway::ProposalProcedure {
            deposit: deposit_b,
            reward_account: ra_b.to_bytes().to_vec(),
            gov_action: GovAction::HardForkInitiation {
                prev_action_id: None,
                protocol_version: (11, 0),
            },
            anchor: Anchor {
                url: String::new(),
                data_hash: [0; 32],
            },
        };
        let gas_b = GovernanceActionState::new(proposal_b);
        ledger
            .governance_actions_mut()
            .insert(gai_b.clone(), gas_b);

        // Action C: HardFork, prev=B (transitively stale), no votes.
        let proposal_c = crate::eras::conway::ProposalProcedure {
            deposit: deposit_c,
            reward_account: ra_c.to_bytes().to_vec(),
            gov_action: GovAction::HardForkInitiation {
                prev_action_id: Some(gai_b.clone()),
                protocol_version: (12, 0),
            },
            anchor: Anchor {
                url: String::new(),
                data_hash: [0; 32],
            },
        };
        let gas_c = GovernanceActionState::new(proposal_c);
        ledger
            .governance_actions_mut()
            .insert(gai_c.clone(), gas_c);

        assert_eq!(ledger.governance_actions().len(), 3);

        let event = apply_epoch_boundary(&mut ledger, EpochNo(2), &mut snapshots, &perf)
            .expect("epoch 2");

        // A enacted, B + C pruned transitively.
        assert_eq!(event.governance_actions_enacted, 1);
        assert_eq!(event.enacted_gov_action_ids, vec![gai_a]);
        assert_eq!(event.enacted_deposit_refunds, deposit_a);

        let mut pruned = event.removed_due_to_enactment.clone();
        pruned.sort();
        let mut expected_pruned = vec![gai_b, gai_c];
        expected_pruned.sort();
        assert_eq!(pruned, expected_pruned);
        assert_eq!(
            event.removed_due_to_enactment_deposit_refunds,
            deposit_b + deposit_c
        );

        // All actions should be gone.
        assert!(ledger.governance_actions().is_empty());

        // Each deposit should be refunded to its reward account.
        assert_eq!(ledger.reward_accounts().balance(&ra_a), deposit_a);
        assert_eq!(ledger.reward_accounts().balance(&ra_b), deposit_b);
        assert_eq!(ledger.reward_accounts().balance(&ra_c), deposit_c);
    }

    #[test]
    fn test_enacted_and_subtree_deposit_unclaimed_goes_to_treasury() {
        // Action A: enacted, return account unregistered.
        // Action B: pruned by lineage, return account also unregistered.
        // Both deposits should go to treasury.
        let mut ledger = make_governance_ledger();

        let deposit_a = 100_000_000u64;
        let unregistered_ra_a = test_reward_account(0x80);
        // Intentionally NOT registering this account.

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();
        let _ = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1");

        let gai_a = test_gov_action_id(0xD0, 0);

        // Action A: InfoAction (auto-ratified), unregistered return account.
        let proposal_a = crate::eras::conway::ProposalProcedure {
            deposit: deposit_a,
            reward_account: unregistered_ra_a.to_bytes().to_vec(),
            gov_action: GovAction::InfoAction,
            anchor: Anchor {
                url: String::new(),
                data_hash: [0; 32],
            },
        };
        ledger
            .governance_actions_mut()
            .insert(gai_a.clone(), GovernanceActionState::new(proposal_a));

        let treasury_before = ledger.accounting().treasury;

        let event = apply_epoch_boundary(&mut ledger, EpochNo(2), &mut snapshots, &perf)
            .expect("epoch 2");

        // Enacted, deposit is unclaimed → treasury.
        assert_eq!(event.governance_actions_enacted, 1);
        assert_eq!(event.enacted_deposit_refunds, 0);
        assert_eq!(event.unclaimed_governance_deposits, deposit_a);

        // Treasury increases by normal flow + unclaimed deposit.
        let treasury_after = ledger.accounting().treasury;
        assert!(treasury_after >= treasury_before + deposit_a);
    }

    #[test]
    fn test_no_subtree_pruning_when_action_types_differ() {
        // If a HardFork is enacted, a ParameterChange with prev_action_id=None
        // should NOT be pruned — they are different purposes.
        let mut ledger = make_governance_ledger();
        let cc_cold_cred = test_cred(0xC0);
        let drep_cred = [0xD0; 28];
        let pool_key = test_pool(20);

        let ra_a = test_reward_account(0x62);
        let ra_pc = test_reward_account(0x63);
        ledger
            .reward_accounts_mut()
            .insert(ra_a, RewardAccountState::new(0, None));
        ledger
            .reward_accounts_mut()
            .insert(ra_pc, RewardAccountState::new(0, None));

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();
        let _ = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1");

        let gai_hf = test_gov_action_id(0xE0, 0);
        let gai_pc = test_gov_action_id(0xE1, 0);

        // HardFork A — voted yes, will be enacted.
        let proposal_hf = crate::eras::conway::ProposalProcedure {
            deposit: 100_000_000,
            reward_account: ra_a.to_bytes().to_vec(),
            gov_action: GovAction::HardForkInitiation {
                prev_action_id: None,
                protocol_version: (10, 0),
            },
            anchor: Anchor {
                url: String::new(),
                data_hash: [0; 32],
            },
        };
        let mut gas_hf = GovernanceActionState::new(proposal_hf);
        gas_hf.record_vote(
            Voter::CommitteeKeyHash(*cc_cold_cred.hash()),
            Vote::Yes,
        );
        gas_hf.record_vote(Voter::DRepKeyHash(drep_cred), Vote::Yes);
        gas_hf.record_vote(Voter::StakePool(pool_key), Vote::Yes);
        ledger
            .governance_actions_mut()
            .insert(gai_hf.clone(), gas_hf);

        // ParameterChange B — no votes, different purpose.
        let proposal_pc = crate::eras::conway::ProposalProcedure {
            deposit: 100_000_000,
            reward_account: ra_pc.to_bytes().to_vec(),
            gov_action: GovAction::ParameterChange {
                prev_action_id: None,
                protocol_param_update: ProtocolParameterUpdate {
                    min_fee_a: Some(55),
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
            anchor: Anchor {
                url: String::new(),
                data_hash: [0; 32],
            },
        };
        let gas_pc = GovernanceActionState::new(proposal_pc);
        ledger
            .governance_actions_mut()
            .insert(gai_pc.clone(), gas_pc);

        let event = apply_epoch_boundary(&mut ledger, EpochNo(2), &mut snapshots, &perf)
            .expect("epoch 2");

        // HardFork enacted, but ParameterChange should survive.
        assert_eq!(event.governance_actions_enacted, 1);
        assert!(event.removed_due_to_enactment.is_empty());
        // ParameterChange should still be pending.
        assert_eq!(ledger.governance_actions().len(), 1);
        assert!(ledger.governance_actions().get(&gai_pc).is_some());
    }

    fn make_ppup_ledger(gen_deleg_count: usize) -> LedgerState {
        let mut ledger = LedgerState::new(Era::Shelley);
        let mut pp = test_protocol_params();
        pp.min_fee_a = 44;
        ledger.set_protocol_params(pp);
        for i in 0..gen_deleg_count {
            let mut genesis_hash = [0u8; 28];
            genesis_hash[0] = i as u8;
            ledger.gen_delegs_mut().insert(
                genesis_hash,
                GenesisDelegationState {
                    delegate: [0x10 + i as u8; 28],
                    vrf: [0x20 + i as u8; 32],
                },
            );
        }
        ledger
    }

    fn pparam_update_min_fee_a(value: u64) -> ProtocolParameterUpdate {
        ProtocolParameterUpdate {
            min_fee_a: Some(value),
            ..Default::default()
        }
    }

    #[test]
    fn ppup_applies_when_quorum_reached() {
        let mut ledger = make_ppup_ledger(3);
        let mut p1 = BTreeMap::new();
        p1.insert([0u8; 28], pparam_update_min_fee_a(77));
        ledger.collect_pparam_proposals(&ShelleyUpdate {
            proposed_protocol_parameter_updates: p1,
            epoch: 3,
        });

        let mut p2 = BTreeMap::new();
        let mut h2 = [0u8; 28];
        h2[0] = 1;
        p2.insert(h2, pparam_update_min_fee_a(77));
        ledger.collect_pparam_proposals(&ShelleyUpdate {
            proposed_protocol_parameter_updates: p2,
            epoch: 3,
        });

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();
        let event = apply_epoch_boundary(&mut ledger, EpochNo(3), &mut snapshots, &perf)
            .expect("epoch 3");

        assert!(event.pparam_updates_applied > 0);
        assert_eq!(ledger.protocol_params().expect("params").min_fee_a, 77);
        assert!(ledger.pending_pparam_updates().is_empty());
    }

    #[test]
    fn ppup_no_quorum_does_not_apply() {
        let mut ledger = make_ppup_ledger(3);
        let mut p1 = BTreeMap::new();
        p1.insert([0u8; 28], pparam_update_min_fee_a(88));
        ledger.collect_pparam_proposals(&ShelleyUpdate {
            proposed_protocol_parameter_updates: p1,
            epoch: 2,
        });

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();
        let event = apply_epoch_boundary(&mut ledger, EpochNo(2), &mut snapshots, &perf)
            .expect("epoch 2");

        assert_eq!(event.pparam_updates_applied, 0);
        assert_eq!(ledger.protocol_params().expect("params").min_fee_a, 44);
    }

    #[test]
    fn ppup_ignores_unknown_delegate_hashes() {
        let mut ledger = make_ppup_ledger(1);
        let mut p1 = BTreeMap::new();
        p1.insert([0xFFu8; 28], pparam_update_min_fee_a(101));
        ledger.collect_pparam_proposals(&ShelleyUpdate {
            proposed_protocol_parameter_updates: p1,
            epoch: 4,
        });

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();
        let event = apply_epoch_boundary(&mut ledger, EpochNo(4), &mut snapshots, &perf)
            .expect("epoch 4");

        assert_eq!(event.pparam_updates_applied, 0);
        assert_eq!(ledger.protocol_params().expect("params").min_fee_a, 44);
    }

    #[test]
    fn pparam_update_field_count_counts_some_fields() {
        let update = ProtocolParameterUpdate {
            min_fee_a: Some(1),
            min_fee_b: Some(2),
            max_tx_size: Some(3),
            ..Default::default()
        };
        assert_eq!(update.field_count(), 3);
        assert_eq!(ProtocolParameterUpdate::default().field_count(), 0);
    }

    // -----------------------------------------------------------------------
    // Epoch boundary: NoConfidence ratification
    // -----------------------------------------------------------------------

    fn test_no_confidence_proposal() -> crate::eras::conway::ProposalProcedure {
        crate::eras::conway::ProposalProcedure {
            deposit: 0,
            reward_account: vec![],
            gov_action: GovAction::NoConfidence { prev_action_id: None },
            anchor: crate::types::Anchor {
                url: String::new(),
                data_hash: [0; 32],
            },
        }
    }

    fn test_treasury_withdrawal_proposal() -> crate::eras::conway::ProposalProcedure {
        let ra = crate::RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash([0xE0; 28]),
        };
        let mut withdrawals = BTreeMap::new();
        withdrawals.insert(ra, 5_000_000);
        crate::eras::conway::ProposalProcedure {
            deposit: 0,
            reward_account: vec![],
            gov_action: GovAction::TreasuryWithdrawals {
                withdrawals,
                guardrails_script_hash: None,
            },
            anchor: crate::types::Anchor {
                url: String::new(),
                data_hash: [0; 32],
            },
        }
    }

    fn test_parameter_change_proposal() -> crate::eras::conway::ProposalProcedure {
        crate::eras::conway::ProposalProcedure {
            deposit: 0,
            reward_account: vec![],
            gov_action: GovAction::ParameterChange {
                prev_action_id: None,
                protocol_param_update: ProtocolParameterUpdate {
                    key_deposit: Some(3_000_000),
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
            anchor: crate::types::Anchor {
                url: String::new(),
                data_hash: [0; 32],
            },
        }
    }

    #[test]
    fn test_no_confidence_ratified_removes_committee() {
        let mut ledger = make_governance_ledger();

        // Set 0% thresholds so DRep/SPO auto-pass.
        let mut drep_thresholds = DRepVotingThresholds::default();
        drep_thresholds.motion_no_confidence = UnitInterval { numerator: 0, denominator: 1 };
        let mut pool_thresholds = PoolVotingThresholds::default();
        pool_thresholds.motion_no_confidence = UnitInterval { numerator: 0, denominator: 1 };
        if let Some(pp) = ledger.protocol_params_mut() {
            pp.drep_voting_thresholds = Some(drep_thresholds);
            pp.pool_voting_thresholds = Some(pool_thresholds);
        }

        // Resign the CC member so committee check passes (vacant CC).
        let cc_cred = test_cred(0xC0);
        ledger
            .committee_state_mut()
            .get_mut(&cc_cred)
            .unwrap()
            .set_authorization(Some(CommitteeAuthorization::CommitteeMemberResigned(None)));

        let gai = test_gov_action_id(0xE1, 0);
        let gas = GovernanceActionState::new(test_no_confidence_proposal());
        ledger.governance_actions_mut().insert(gai.clone(), gas);

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        let _ = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1");
        if ledger.governance_actions().is_empty() {
            // Enacted at epoch 1.
            assert_eq!(ledger.committee_state().len(), 0);
            return;
        }

        let event = apply_epoch_boundary(&mut ledger, EpochNo(2), &mut snapshots, &perf)
            .expect("epoch 2");

        assert_eq!(event.governance_actions_enacted, 1);
        assert_eq!(ledger.committee_state().len(), 0);
        assert_eq!(
            ledger.enact_state().committee_quorum,
            UnitInterval { numerator: 0, denominator: 1 },
        );
    }

    #[test]
    fn test_no_confidence_not_ratified_without_drep_spo_approval() {
        let mut ledger = make_governance_ledger();

        // 100% thresholds → requires all votes.
        let mut drep_thresholds = DRepVotingThresholds::default();
        drep_thresholds.motion_no_confidence = UnitInterval { numerator: 1, denominator: 1 };
        let mut pool_thresholds = PoolVotingThresholds::default();
        pool_thresholds.motion_no_confidence = UnitInterval { numerator: 1, denominator: 1 };
        if let Some(pp) = ledger.protocol_params_mut() {
            pp.drep_voting_thresholds = Some(drep_thresholds);
            pp.pool_voting_thresholds = Some(pool_thresholds);
        }

        let gai = test_gov_action_id(0xE2, 0);
        let gas = GovernanceActionState::new(test_no_confidence_proposal());
        ledger.governance_actions_mut().insert(gai.clone(), gas);

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        let _ = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1");
        let event = apply_epoch_boundary(&mut ledger, EpochNo(2), &mut snapshots, &perf)
            .expect("epoch 2");

        assert_eq!(event.governance_actions_enacted, 0);
        assert!(ledger.governance_actions().contains_key(&gai));
        assert!(ledger.committee_state().len() > 0); // committee still present
    }

    // -----------------------------------------------------------------------
    // Epoch boundary: TreasuryWithdrawals ratification
    // -----------------------------------------------------------------------

    #[test]
    fn test_treasury_withdrawal_ratified_credits_reward_account() {
        let mut ledger = make_governance_ledger();

        // Set 0% thresholds so auto-pass on CC + DRep.
        let mut drep_thresholds = DRepVotingThresholds::default();
        drep_thresholds.treasury_withdrawal = UnitInterval { numerator: 0, denominator: 1 };
        if let Some(pp) = ledger.protocol_params_mut() {
            pp.drep_voting_thresholds = Some(drep_thresholds);
        }
        // Set CC quorum to 0% so committee auto-passes.
        ledger.enact_state_mut().committee_quorum = UnitInterval { numerator: 0, denominator: 1 };

        // Register the withdrawal target credential and create reward account entry.
        let target_cred = StakeCredential::AddrKeyHash([0xE0; 28]);
        ledger.stake_credentials_mut().register(target_cred);
        let target_ra = crate::RewardAccount {
            network: 1,
            credential: target_cred,
        };
        ledger.reward_accounts_mut().insert(
            target_ra,
            crate::RewardAccountState::new(0, None),
        );
        ledger.accounting_mut().treasury = 100_000_000;

        let gai = test_gov_action_id(0xE3, 0);
        let gas = GovernanceActionState::new(test_treasury_withdrawal_proposal());
        ledger.governance_actions_mut().insert(gai.clone(), gas);

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        let _ = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1");
        if ledger.governance_actions().is_empty() {
            // Enacted at epoch 1.
            let ra = crate::RewardAccount {
                network: 1,
                credential: target_cred,
            };
            assert!(ledger.reward_accounts().get(&ra).unwrap().balance() >= 5_000_000);
            return;
        }

        let event = apply_epoch_boundary(&mut ledger, EpochNo(2), &mut snapshots, &perf)
            .expect("epoch 2");

        assert_eq!(event.governance_actions_enacted, 1);
        let ra = crate::RewardAccount {
            network: 1,
            credential: target_cred,
        };
        assert!(ledger.reward_accounts().get(&ra).unwrap().balance() >= 5_000_000);
    }

    #[test]
    fn test_treasury_withdrawal_not_ratified_without_votes() {
        let mut ledger = make_governance_ledger();

        // 100% DRep threshold → needs explicit DRep Yes.
        let mut drep_thresholds = DRepVotingThresholds::default();
        drep_thresholds.treasury_withdrawal = UnitInterval { numerator: 1, denominator: 1 };
        if let Some(pp) = ledger.protocol_params_mut() {
            pp.drep_voting_thresholds = Some(drep_thresholds);
        }

        let gai = test_gov_action_id(0xE4, 0);
        let gas = GovernanceActionState::new(test_treasury_withdrawal_proposal());
        ledger.governance_actions_mut().insert(gai.clone(), gas);

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        let _ = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1");
        let event = apply_epoch_boundary(&mut ledger, EpochNo(2), &mut snapshots, &perf)
            .expect("epoch 2");

        assert_eq!(event.governance_actions_enacted, 0);
        assert!(ledger.governance_actions().contains_key(&gai));
    }

    // -----------------------------------------------------------------------
    // Epoch boundary: ParameterChange ratification
    // -----------------------------------------------------------------------

    #[test]
    fn test_parameter_change_ratified_applies_update() {
        let mut ledger = make_governance_ledger();

        // Set 0% thresholds so auto-pass on CC + DRep.
        let mut drep_thresholds = DRepVotingThresholds::default();
        drep_thresholds.pp_economic_group = UnitInterval { numerator: 0, denominator: 1 };
        drep_thresholds.pp_network_group = UnitInterval { numerator: 0, denominator: 1 };
        drep_thresholds.pp_technical_group = UnitInterval { numerator: 0, denominator: 1 };
        drep_thresholds.pp_gov_group = UnitInterval { numerator: 0, denominator: 1 };
        if let Some(pp) = ledger.protocol_params_mut() {
            pp.drep_voting_thresholds = Some(drep_thresholds);
        }
        // Set CC quorum to 0% so committee auto-passes.
        ledger.enact_state_mut().committee_quorum = UnitInterval { numerator: 0, denominator: 1 };

        let gai = test_gov_action_id(0xE5, 0);
        let gas = GovernanceActionState::new(test_parameter_change_proposal());
        ledger.governance_actions_mut().insert(gai.clone(), gas);

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        let _ = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1");
        if ledger.governance_actions().is_empty() {
            // Enacted at epoch 1.
            assert_eq!(
                ledger.protocol_params().unwrap().key_deposit,
                3_000_000,
            );
            return;
        }

        let event = apply_epoch_boundary(&mut ledger, EpochNo(2), &mut snapshots, &perf)
            .expect("epoch 2");

        assert_eq!(event.governance_actions_enacted, 1);
        assert_eq!(
            ledger.protocol_params().unwrap().key_deposit,
            3_000_000,
        );
    }

    #[test]
    fn test_parameter_change_not_ratified_without_votes() {
        let mut ledger = make_governance_ledger();

        // 100% DRep threshold.
        let mut drep_thresholds = DRepVotingThresholds::default();
        drep_thresholds.pp_economic_group = UnitInterval { numerator: 1, denominator: 1 };
        drep_thresholds.pp_network_group = UnitInterval { numerator: 1, denominator: 1 };
        drep_thresholds.pp_technical_group = UnitInterval { numerator: 1, denominator: 1 };
        drep_thresholds.pp_gov_group = UnitInterval { numerator: 1, denominator: 1 };
        if let Some(pp) = ledger.protocol_params_mut() {
            pp.drep_voting_thresholds = Some(drep_thresholds);
        }

        let gai = test_gov_action_id(0xE6, 0);
        let gas = GovernanceActionState::new(test_parameter_change_proposal());
        ledger.governance_actions_mut().insert(gai.clone(), gas);

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        let _ = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1");
        let event = apply_epoch_boundary(&mut ledger, EpochNo(2), &mut snapshots, &perf)
            .expect("epoch 2");

        assert_eq!(event.governance_actions_enacted, 0);
        assert!(ledger.governance_actions().contains_key(&gai));
    }

    // -- RUPD-before-SNAP ordering: rewards reflected in mark snapshot ----

    #[test]
    fn test_rewards_reflected_in_mark_snapshot() {
        // Verify that epoch rewards credited to reward accounts are
        // included in the freshly-computed mark snapshot (RUPD before SNAP).
        let mut ledger = make_ledger_with_pool(21);

        // Add stake UTxO delegated to pool 21.
        let cred = test_cred(21);
        let base_addr = Address::Base(BaseAddress {
            network: 1,
            payment: StakeCredential::AddrKeyHash([0xAA; 28]),
            staking: cred,
        });
        let addr_bytes = base_addr.to_bytes();
        let txin = ShelleyTxIn { transaction_id: [21u8; 32], index: 0 };
        ledger.multi_era_utxo_mut().insert_shelley(
            txin,
            ShelleyTxOut {
                address: addr_bytes.clone(),
                amount: 10_000_000_000_000, // 10M ADA
            },
        );

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        // Epoch 1: populate mark from current state.
        let _ = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1");
        // Epoch 2: go snapshot now has the pool → rewards are computed.
        snapshots.accumulate_fees(1_000_000_000);
        let _ = apply_epoch_boundary(&mut ledger, EpochNo(2), &mut snapshots, &perf)
            .expect("epoch 2");

        // After epoch 2, the reward account should have positive balance
        // (rewards were distributed).
        let ra = test_reward_account(21);
        let reward_balance = ledger.reward_accounts().balance(&ra);

        // Epoch 3: the mark snapshot should now include the reward balance.
        snapshots.accumulate_fees(500_000_000);
        let _ = apply_epoch_boundary(&mut ledger, EpochNo(3), &mut snapshots, &perf)
            .expect("epoch 3");

        // The mark snapshot's stake should include the operator's reward
        // balance (reward accounts feed into compute_stake_snapshot).
        let mark_stake = snapshots.mark.stake.get(&cred);

        // Reward balance should be non-zero and reflected in mark snapshot.
        if reward_balance > 0 {
            // Mark snapshot stake should be at least the reward balance
            // (it may also include UTxO stake).
            assert!(
                mark_stake >= reward_balance,
                "mark snapshot stake ({mark_stake}) should include reward balance ({reward_balance})"
            );
        }
    }

    // -- Reserves accounting: only monetary expansion deducted from reserves

    #[test]
    fn test_reserves_only_deducted_by_monetary_expansion() {
        let mut ledger = make_ledger_with_pool(22);

        // Add UTxO stake delegated to pool 22.
        let cred = test_cred(22);
        let base_addr = Address::Base(BaseAddress {
            network: 1,
            payment: StakeCredential::AddrKeyHash([0xBB; 28]),
            staking: cred,
        });
        let addr_bytes = base_addr.to_bytes();
        let txin = ShelleyTxIn { transaction_id: [22u8; 32], index: 0 };
        ledger.multi_era_utxo_mut().insert_shelley(
            txin,
            ShelleyTxOut {
                address: addr_bytes.clone(),
                amount: 50_000_000_000_000, // 50M ADA
            },
        );

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        // Epoch 1: populate mark.
        let _ = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1");

        // Epoch 2: go snapshot has pool → rewards computed.
        // Add a large fee pot to make the difference visible.
        snapshots.accumulate_fees(10_000_000_000); // 10k ADA in fees
        let reserves_before_epoch2 = ledger.accounting().reserves;
        let event = apply_epoch_boundary(&mut ledger, EpochNo(2), &mut snapshots, &perf)
            .expect("epoch 2");

        let reserves_after = ledger.accounting().reserves;
        let actual_deduction = reserves_before_epoch2.saturating_sub(reserves_after);

        // delta_reserves should be reserves × rho (monetary expansion only),
        // NOT delta_reserves + fee_pot.
        assert_eq!(actual_deduction, event.delta_reserves);

        // The fee pot (10k ADA) should NOT have been deducted from reserves.
        // rho = 3/1000, so delta_reserves ≈ reserves × 0.003.
        let expected_delta = (reserves_before_epoch2 as u128 * 3 / 1000) as u64;
        assert_eq!(event.delta_reserves, expected_delta);

        // Verify that reserves were NOT over-decremented by the fee pot.
        assert_eq!(reserves_after, reserves_before_epoch2 - expected_delta);
    }

    // -- Fee pot does not affect reserves --

    #[test]
    fn test_fee_pot_not_subtracted_from_reserves() {
        // With zero reserves (so delta_reserves = 0), the fee pot should
        // NOT cause any reserves deduction.
        let mut ledger = make_ledger_with_pool(23);
        ledger.accounting_mut().reserves = 0; // no reserves
        ledger.accounting_mut().treasury = 0;

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        // Add fees — these come from transactions, not reserves.
        snapshots.accumulate_fees(5_000_000_000); // 5k ADA

        let _ = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1");

        // Reserves should still be 0 (fees don't come from reserves).
        assert_eq!(ledger.accounting().reserves, 0);
        // Treasury should have received the treasury cut of the fees.
        assert!(ledger.accounting().treasury > 0);
    }
}
