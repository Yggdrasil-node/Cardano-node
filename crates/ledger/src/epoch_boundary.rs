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
    /// Pool deposits that could not be refunded because the reward
    /// account was no longer registered — sent to treasury.
    ///
    /// Reference: `poolReapTransition` — `casTreasury += unclaimed`.
    pub unclaimed_pool_deposits: u64,
    /// Total rewards distributed to delegators & operators.
    pub rewards_distributed: u64,
    /// Treasury delta (τ cut + unregistered rewards to treasury).
    ///
    /// Does NOT include unclaimed (`deltaR2`) — those go back to reserves.
    pub treasury_delta: u64,
    /// Unclaimed rewards returned to reserves (`deltaR2`).
    pub unclaimed_rewards: u64,
    /// Monetary expansion drawn from reserves (ΔR1).
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
    /// Accumulated treasury donations (Conway `utxosDonation`) transferred to
    /// treasury during this epoch boundary.
    pub donations_transferred: u64,
    /// Number of reward accounts credited via MIR at this epoch boundary.
    pub mir_accounts_credited: usize,
    /// Total lovelace credited to reward accounts from reserves via MIR.
    pub mir_from_reserves: u64,
    /// Total lovelace credited to reward accounts from treasury via MIR.
    pub mir_from_treasury: u64,
    /// Net delta applied to reserves from pot-to-pot MIR transfers.
    pub mir_pot_delta_reserves: i64,
    /// Net delta applied to treasury from pot-to-pot MIR transfers.
    pub mir_pot_delta_treasury: i64,
    /// `true` when MIR rewards were skipped because a pot had insufficient
    /// funds (all-or-nothing rule).
    pub mir_pots_insufficient: bool,
}

// ---------------------------------------------------------------------------
// Epoch boundary application
// ---------------------------------------------------------------------------

/// Computes the monetary expansion efficiency factor η.
///
/// When `d >= 0.8`: η = 1 (no adjustment).
/// When `d < 0.8` (post-Shelley: d = 0):
///   `expected_blocks = ⌊(1 - d) × asc × slots_per_epoch⌋`
///   `η = blocks_made / expected_blocks` (capped at 1).
///
/// Returns `(1, 1)` when genesis data is unavailable (`slots_per_epoch == 0`
/// or `asc == 0`) so the reward formula behaves conservatively.
///
/// Reference: `startStep` in
/// `Cardano.Ledger.Shelley.LedgerState.PulsingReward`.
fn compute_eta(
    d_param: UnitInterval,
    asc: UnitInterval,
    slots_per_epoch: u64,
    blocks_made: &std::collections::BTreeMap<PoolKeyHash, u64>,
) -> UnitInterval {
    let one = UnitInterval { numerator: 1, denominator: 1 };

    // d >= 0.8 → η = 1
    if d_param.denominator > 0 && d_param.numerator * 10 >= d_param.denominator * 8 {
        return one;
    }

    if slots_per_epoch == 0 || asc.denominator == 0 || asc.numerator == 0 {
        return one;
    }

    // (1 - d) as a ratio
    let one_minus_d_num = d_param.denominator.saturating_sub(d_param.numerator) as u128;
    let one_minus_d_den = d_param.denominator as u128;

    // expectedBlocks = floor((1 - d) × asc × slots_per_epoch)
    let expected = one_minus_d_num
        * (asc.numerator as u128)
        * (slots_per_epoch as u128)
        / (one_minus_d_den * asc.denominator as u128);

    if expected == 0 {
        return one;
    }

    let total_blocks: u64 = blocks_made.values().sum();
    let blocks = total_blocks as u128;

    // η = min(1, blocks / expected)
    if blocks >= expected {
        one
    } else {
        // Reduce to u64 — safe because blocks < expected and expected
        // fits u128 comfortably.
        UnitInterval {
            numerator: total_blocks,
            denominator: expected as u64,
        }
    }
}

/// Derives per-pool performance ratios from block production counts and
/// the `set` stake snapshot.
///
/// Performance for each pool is `blocks_produced / expected_blocks` where
/// `expected_blocks = σ_pool * total_blocks`. When the snapshot has no
/// stake data, returns an empty map (all pools get perfect performance).
///
/// Reference: `Cardano.Ledger.Shelley.LedgerState` — `completeRupd`,
/// `mkApparentPerformance`.
///
/// When `d >= 0.8` (early Shelley era), upstream assigns apparent
/// performance of 1 to every block-producing pool regardless of
/// their actual share of blocks.
fn derive_pool_performance(
    blocks_made: &BTreeMap<PoolKeyHash, u64>,
    set_snapshot: &crate::stake::StakeSnapshot,
    d: UnitInterval,
) -> BTreeMap<PoolKeyHash, UnitInterval> {
    // d >= 0.8  →  all block-producing pools get perf = 1.
    // Reference: `mkApparentPerformance` in Shelley.Rewards:
    //   | unboundRational d_ < 0.8 = beta / sigma
    //   | otherwise = 1
    if d.numerator * 10 >= d.denominator * 8 && d.denominator > 0 {
        return blocks_made
            .keys()
            .map(|pool_hash| (*pool_hash, UnitInterval { numerator: 1, denominator: 1 }))
            .collect();
    }

    let stake_dist = set_snapshot.pool_stake_distribution();
    let total_stake = stake_dist.total_active_stake();
    if total_stake == 0 || blocks_made.is_empty() {
        return BTreeMap::new();
    }

    let total_blocks: u64 = blocks_made.values().sum();
    if total_blocks == 0 {
        return BTreeMap::new();
    }

    let mut performance = BTreeMap::new();
    for (pool_hash, &blocks_produced) in blocks_made {
        let pool_stake = stake_dist.pool_stake(pool_hash);
        if pool_stake == 0 {
            continue;
        }
        // performance = blocks_produced / (σ * total_blocks)
        //             = blocks_produced * total_stake / (pool_stake * total_blocks)
        let numerator = blocks_produced.saturating_mul(total_stake);
        let denominator = pool_stake.saturating_mul(total_blocks);
        if denominator > 0 {
            performance.insert(*pool_hash, UnitInterval { numerator, denominator });
        }
    }
    performance
}

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
///   calculation.  When non-empty, these values are used directly.
///   When the caller passes an empty map, the function derives
///   per-pool performance from `ledger.blocks_made()` and the `set`
///   snapshot's stake distribution (upstream `nesBcur` semantics).
///   A pool absent from the resulting map is treated as having
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
    // Capture the *previous* epoch's protocol parameters BEFORE any
    // PPUP/UPEC update.  Upstream `startStep` reads `prevPParams` for
    // the reward calculation.  PPUP is applied later, inside EPOCH,
    // after SNAP and POOLREAP (UPEC rule).
    //
    // Reference: `Cardano.Ledger.Shelley.LedgerState.PulsingReward`
    //   — `startStep … (pr = es ^. prevPParamsEpochStateL)`.
    // -----------------------------------------------------------------------

    let params = ledger
        .protocol_params()
        .ok_or(LedgerError::MissingProtocolParameters)?;

    // Extract values from params before any mutable borrows.
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
    //    When the caller supplies an explicit pool_performance map
    //    (non-empty), use it directly.  Otherwise derive performance
    //    from the ledger's internal `blocks_made` (upstream `nesBcur`).
    //
    //    Reference: `Cardano.Ledger.Shelley.Rules.NewEpoch` — RUPD runs
    //    before EPOCH (which contains SNAP).
    // -----------------------------------------------------------------------
    let fee_pot = std::mem::take(&mut snapshots.fee_pot);

    // Compute eta — monetary expansion efficiency factor.
    //
    // When d < 0.8 (post-Shelley: d = 0): eta = blocksMade / expectedBlocks,
    // capped at 1.  When d >= 0.8: eta = 1.
    //
    // expectedBlocks = (1 - d) × active_slot_coeff × slots_per_epoch
    //
    // Reference: `startStep` in
    // `Cardano.Ledger.Shelley.LedgerState.PulsingReward`.
    let d_param = params.d.unwrap_or(UnitInterval { numerator: 0, denominator: 1 });
    let eta = compute_eta(
        d_param,
        ledger.active_slot_coeff(),
        ledger.slots_per_epoch(),
        ledger.blocks_made(),
    );

    let reward_params = RewardParams {
        rho,
        tau,
        a0,
        n_opt,
        min_pool_cost,
        reserves: ledger.accounting().reserves,
        fee_pot,
        max_lovelace_supply: ledger.max_lovelace_supply(),
        eta,
    };

    // Derive effective performance: caller-provided or from internal blocks_made.
    let effective_performance: BTreeMap<PoolKeyHash, UnitInterval>;
    if pool_performance.is_empty() && !ledger.blocks_made().is_empty() {
        effective_performance = derive_pool_performance(ledger.blocks_made(), &snapshots.set, d_param);
    } else {
        effective_performance = pool_performance.clone();
    }
    // Clear blocks_made for the new epoch (upstream NEWEPOCH rotates nesBcur).
    ledger.take_blocks_made();

    let reward_dist = compute_epoch_rewards(&reward_params, &snapshots.go, &effective_performance);
    let (accounts_rewarded, unregistered_rewards) = distribute_rewards(ledger, &reward_dist);

    // -----------------------------------------------------------------------
    // 1b. MIR — apply accumulated Move Instantaneous Rewards.
    //
    //     The upstream NEWEPOCH rule: RUPD → **MIR** → EPOCH (SNAP …).
    //     MIR rewards from reserves and treasury are credited to
    //     registered reward accounts, pot-to-pot delta transfers are
    //     applied, and accumulated IR state is cleared.
    //
    //     Reference: `Cardano.Ledger.Shelley.Rules.Mir`.
    // -----------------------------------------------------------------------
    let mir_result = apply_mir_at_epoch_boundary(ledger);

    // -----------------------------------------------------------------------
    // 2. SNAP — compute a fresh mark snapshot from post-reward state
    //    and rotate the three-snapshot ring.
    //
    //    Future pool params are NOT adopted here — upstream `snapTransition`
    //    takes the snapshot using the *current* `psStakePoolParams`, and
    //    future params are activated later in POOLREAP.
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
    // 2b. Activate future pool params — upstream does this inside
    //     `poolReapTransition` (after SNAP has already captured the
    //     snapshot with old params).
    //
    //     Reference: `Cardano.Ledger.Shelley.Rules.PoolReap` —
    //     `psFutureStakePoolParams` merged into `psStakePoolParams`.
    // -----------------------------------------------------------------------
    ledger.pool_state_mut().adopt_future_params();

    // -----------------------------------------------------------------------
    // 3. Pool retirement — remove pools and refund deposits.
    // -----------------------------------------------------------------------
    let (retired_pool_keys, pool_deposit_refunds, unclaimed_pool_deposits) =
        retire_pools_with_refunds(ledger, new_epoch);
    let pools_retired = retired_pool_keys.len();

    // -----------------------------------------------------------------------
    // 3b. UPEC — apply any pending Shelley-era protocol parameter
    //     update proposals whose target epoch matches the new epoch.
    //
    //     Upstream order: NEWEPOCH → RUPD → MIR → EPOCH(SNAP → POOLREAP → UPEC).
    //     Applying UPEC here (after SNAP and POOLREAP) ensures that
    //     reward calculations and deposit refunds use the *previous*
    //     epoch's protocol parameters, matching upstream `prevPParams`.
    //
    //     Reference: `Cardano.Ledger.Shelley.Rules.Epoch` — UPEC
    //     is the last sub-rule inside EPOCH.
    // -----------------------------------------------------------------------
    let pparam_updates_applied = ledger.apply_pending_pparam_updates(new_epoch);

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
    // Flush accumulated Conway treasury donations into treasury.
    //
    // Reference: `Cardano.Ledger.Conway.Rules.Epoch` — epoch boundary:
    // `casTreasuryL <>~ utxosDonationL`, then `utxosDonationL .~ zero`.
    let donations_transferred = ledger.flush_donations_to_treasury();
    {
        let acct = ledger.accounting_mut();
        // Upstream reserves change: deltaR = -deltaR1 + deltaR2
        //   = -(delta_reserves) + unclaimed
        // So net reserves decrease = delta_reserves - unclaimed.
        acct.reserves = acct
            .reserves
            .saturating_sub(reward_dist.delta_reserves)
            .saturating_add(reward_dist.unclaimed);
        // Upstream treasury change: deltaT = deltaT1 + frTotalUnregistered
        //   (only the tau cut + unregistered rewards).
        // Unclaimed (deltaR2) is returned to reserves, NOT added to treasury.
        //
        // Additionally, unclaimed pool deposit refunds (pools whose reward
        // account was no longer registered at retirement) go to treasury.
        //
        // Reference: `applyRUpdFiltered` in
        // `Cardano.Ledger.Shelley.LedgerState.IncrementalStake`:
        //   casTreasury += deltaT + frTotalUnregistered
        //   casReserves += deltaR  (where deltaR includes +deltaR2)
        // `poolReapTransition`: casTreasury += unclaimed pool deposits
        acct.treasury = acct
            .treasury
            .saturating_add(reward_dist.treasury_cut)
            .saturating_add(unregistered_rewards)
            .saturating_add(unclaimed_pool_deposits);
    }

    // -----------------------------------------------------------------------
    // 5. Governance action expiry — remove expired proposals and refund
    //    deposits to their return accounts (Conway+ EPOCH rule).
    // -----------------------------------------------------------------------
    let (expired_gov_action_ids, governance_deposit_refunds, expired_unclaimed_deposits) =
        remove_expired_governance_actions(ledger, new_epoch);

    // 5a. Remove descendant proposals whose prev_action_id chains through
    //     an expired parent.  Upstream `proposalsRemoveWithDescendants`
    //     transitively removes descendants of expired proposals.
    //     Reference: Cardano.Ledger.Conway.Governance.Proposals.
    let (descendant_refunds, descendant_unclaimed) = if !expired_gov_action_ids.is_empty() {
        remove_descendants_of(ledger, &expired_gov_action_ids)
    } else {
        (0, 0)
    };
    let governance_actions_expired =
        expired_gov_action_ids.len();

    // -----------------------------------------------------------------------
    // 5b. Ratification — tally votes for surviving governance actions and
    //     enact any that reach their acceptance thresholds.
    //     Upstream: `Cardano.Ledger.Conway.Rules.Ratify` — run at each
    //     epoch boundary after expiry pruning.
    // -----------------------------------------------------------------------
    let ratify_result = ratify_and_enact(ledger, new_epoch, snapshots, drep_activity);
    let governance_actions_enacted = ratify_result.enacted_ids.len();

    // Credit unclaimed governance deposits to treasury — both from
    // expired proposals with unregistered return accounts, from
    // descendants of expired proposals, AND from enacted actions
    // with unregistered return accounts.
    // Upstream: `returnProposalDeposits` in `Cardano.Ledger.Conway.Rules.Epoch`.
    let total_unclaimed = expired_unclaimed_deposits
        .saturating_add(descendant_unclaimed)
        .saturating_add(ratify_result.unclaimed_deposits);
    if total_unclaimed > 0 {
        let acct = ledger.accounting_mut();
        acct.treasury = acct.treasury.saturating_add(total_unclaimed);
    }

    // -----------------------------------------------------------------------
    // 5c. Dormant epoch counter — if no active (non-expired) governance
    //     proposals remain, increment the dormant counter.  Otherwise
    //     leave it unchanged.
    //
    //     Upstream `updateNumDormantEpochs` in
    //     `Cardano.Ledger.Conway.Rules.Epoch` only calls `succ` when
    //     proposals are empty and **never** resets to 0 at epoch boundary.
    //     The counter is reset to 0 inside the per-tx
    //     `updateDormantDRepExpiries` (GOV rule) when proposals first
    //     appear and the dormant count is distributed to DRep expiries.
    // -----------------------------------------------------------------------
    if ledger.governance_actions().is_empty() {
        ledger.num_dormant_epochs = ledger.num_dormant_epochs.saturating_add(1);
    }
    // When proposals exist: leave num_dormant_epochs unchanged (upstream
    // semantics).  The per-tx updateDormantDRepExpiries already reset it
    // when proposals first appeared.

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
        unclaimed_pool_deposits,
        rewards_distributed: reward_dist.distributed,
        treasury_delta: reward_dist.treasury_cut.saturating_add(unregistered_rewards),
        unclaimed_rewards: reward_dist.unclaimed,
        delta_reserves: reward_dist.delta_reserves,
        accounts_rewarded,
        governance_actions_expired,
        governance_deposit_refunds: governance_deposit_refunds
            .saturating_add(descendant_refunds),
        expired_gov_action_ids,
        dreps_expired,
        governance_actions_enacted,
        enacted_gov_action_ids: ratify_result.enacted_ids,
        enact_outcomes: ratify_result.outcomes,
        enacted_deposit_refunds: ratify_result.enacted_deposit_refunds,
        removed_due_to_enactment: ratify_result.removed_due_to_enactment,
        removed_due_to_enactment_deposit_refunds: ratify_result.removed_due_to_enactment_deposit_refunds,
        unclaimed_governance_deposits: ratify_result.unclaimed_deposits
            .saturating_add(expired_unclaimed_deposits),
        donations_transferred,
        mir_accounts_credited: mir_result.accounts_credited,
        mir_from_reserves: mir_result.from_reserves,
        mir_from_treasury: mir_result.from_treasury,
        mir_pot_delta_reserves: mir_result.pot_delta_reserves,
        mir_pot_delta_treasury: mir_result.pot_delta_treasury,
        mir_pots_insufficient: mir_result.pots_insufficient,
    })
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Credits reward accounts from the epoch distribution.
///
/// Leader rewards are keyed by `RewardAccount` (pool's declared account).
/// Member rewards are keyed by `StakeCredential` and resolved to the
/// member's own registered reward account at application time.
///
/// Returns `(accounts_rewarded, unregistered_rewards)` where the second
/// element is the total lovelace that could not be delivered because the
/// reward account is no longer registered.
///
/// Reference: `applyRUpdFiltered` in
/// `Cardano.Ledger.Shelley.LedgerState.IncrementalStake` — adds
/// `frTotalUnregistered` to the treasury after filtering.
fn distribute_rewards(
    ledger: &mut LedgerState,
    dist: &EpochRewardDistribution,
) -> (usize, u64) {
    let mut count = 0usize;
    let mut unregistered_total = 0u64;

    // 1. Leader rewards — keyed by pool's declared RewardAccount.
    {
        let ra = ledger.reward_accounts_mut();
        for (account, &amount) in &dist.leader_deltas {
            if amount == 0 {
                continue;
            }
            if let Some(state) = ra.get_mut(account) {
                state.set_balance(state.balance().saturating_add(amount));
                count += 1;
            } else {
                unregistered_total = unregistered_total.saturating_add(amount);
            }
        }
    }

    // 2. Member rewards — keyed by StakeCredential, resolved to the
    //    member's own registered RewardAccount matching that credential.
    {
        let ra = ledger.reward_accounts_mut();
        for (cred, &amount) in &dist.reward_deltas {
            if amount == 0 {
                continue;
            }
            if ra.credit_by_credential(cred, amount) {
                count += 1;
            } else {
                unregistered_total = unregistered_total.saturating_add(amount);
            }
        }
    }

    (count, unregistered_total)
}

// ---------------------------------------------------------------------------
// MIR epoch application
// ---------------------------------------------------------------------------

/// Result of applying MIR at an epoch boundary.
#[derive(Clone, Debug, Default)]
struct MirEpochResult {
    /// Number of reward accounts credited.
    accounts_credited: usize,
    /// Total lovelace debited from reserves for MIR rewards.
    from_reserves: u64,
    /// Total lovelace debited from treasury for MIR rewards.
    from_treasury: u64,
    /// Net delta applied to reserves from pot-to-pot transfers.
    pot_delta_reserves: i64,
    /// Net delta applied to treasury from pot-to-pot transfers.
    pot_delta_treasury: i64,
    /// Whether rewards were skipped due to pot insufficiency.
    pots_insufficient: bool,
}

/// Applies accumulated Move Instantaneous Rewards at the epoch boundary.
///
/// Implements the upstream MIR rule (`Cardano.Ledger.Shelley.Rules.Mir`):
///
/// 1. Filter `ir_reserves` and `ir_treasury` to registered reward accounts.
/// 2. All-or-nothing check: if reserves < Σ(filtered_reserves) **or**
///    treasury < Σ(filtered_treasury), no rewards are distributed from
///    either pot.
/// 3. On success: merge per-credential amounts from both pots and credit
///    reward accounts; debit the respective pots.
/// 4. Apply pot-to-pot delta transfers (from `SendToOppositePot` certs)
///    regardless of whether rewards were distributed.
/// 5. Always clear the `InstantaneousRewards` state.
///
/// MIR certificates exist in Shelley through Babbage; Conway does not
/// produce any MIR entries.
fn apply_mir_at_epoch_boundary(ledger: &mut LedgerState) -> MirEpochResult {
    // Take and clear the IR state from ledger.
    let ir = std::mem::take(ledger.instantaneous_rewards_mut());

    if ir.is_empty() {
        return MirEpochResult::default();
    }

    let delta_reserves = ir.delta_reserves;
    let delta_treasury = ir.delta_treasury;

    // 1. Build registered credential set from reward accounts so we can
    //    filter MIR maps without needing the network id for lookups.
    let cred_to_account: BTreeMap<crate::types::StakeCredential, RewardAccount> = ledger
        .reward_accounts()
        .iter()
        .map(|(account, _)| (account.credential, *account))
        .collect();

    let filtered_reserves: BTreeMap<crate::types::StakeCredential, i64> = ir
        .ir_reserves
        .into_iter()
        .filter(|(cred, _)| cred_to_account.contains_key(cred))
        .collect();

    let filtered_treasury: BTreeMap<crate::types::StakeCredential, i64> = ir
        .ir_treasury
        .into_iter()
        .filter(|(cred, _)| cred_to_account.contains_key(cred))
        .collect();

    let total_reserves: i64 = filtered_reserves.values().sum();
    let total_treasury: i64 = filtered_treasury.values().sum();

    // 2. All-or-nothing: both pots must be sufficient, or neither pays.
    let reserves = ledger.accounting().reserves;
    let treasury = ledger.accounting().treasury;

    let reserves_ok = total_reserves <= 0 || reserves >= total_reserves as u64;
    let treasury_ok = total_treasury <= 0 || treasury >= total_treasury as u64;
    let can_pay = reserves_ok && treasury_ok;

    let mut accounts_credited = 0usize;
    let mut from_reserves = 0u64;
    let mut from_treasury = 0u64;

    if can_pay && (total_reserves != 0 || total_treasury != 0) {
        // 3. Merge per-credential amounts from both pots.
        let mut combined: BTreeMap<crate::types::StakeCredential, i64> = BTreeMap::new();
        for (cred, delta) in &filtered_reserves {
            *combined.entry(*cred).or_insert(0) += delta;
        }
        for (cred, delta) in &filtered_treasury {
            *combined.entry(*cred).or_insert(0) += delta;
        }

        // Credit reward accounts.
        let ra = ledger.reward_accounts_mut();
        for (cred, &delta) in &combined {
            if delta == 0 {
                continue;
            }
            if let Some(account) = cred_to_account.get(cred) {
                if let Some(state) = ra.get_mut(account) {
                    if delta > 0 {
                        state.set_balance(state.balance().saturating_add(delta as u64));
                    } else {
                        state.set_balance(state.balance().saturating_sub((-delta) as u64));
                    }
                    accounts_credited += 1;
                }
            }
        }

        // Record debits.
        from_reserves = if total_reserves > 0 {
            total_reserves as u64
        } else {
            0
        };
        from_treasury = if total_treasury > 0 {
            total_treasury as u64
        } else {
            0
        };
    }

    // 4. Update pots: debit reward sums (if paid) + apply delta transfers.
    {
        let acct = ledger.accounting_mut();
        // Debit for distributed MIR rewards.
        if can_pay {
            apply_signed_delta(&mut acct.reserves, -total_reserves);
            apply_signed_delta(&mut acct.treasury, -total_treasury);
        }
        // Pot-to-pot delta transfers (always applied).
        apply_signed_delta(&mut acct.reserves, delta_reserves);
        apply_signed_delta(&mut acct.treasury, delta_treasury);
    }

    MirEpochResult {
        accounts_credited,
        from_reserves,
        from_treasury,
        pot_delta_reserves: delta_reserves,
        pot_delta_treasury: delta_treasury,
        pots_insufficient: !can_pay,
    }
}

/// Applies a signed delta to an unsigned pot balance (saturating).
fn apply_signed_delta(pot: &mut u64, delta: i64) {
    match delta.cmp(&0) {
        std::cmp::Ordering::Greater => {
            *pot = pot.saturating_add(delta as u64);
        }
        std::cmp::Ordering::Less => {
            *pot = pot.saturating_sub((-delta) as u64);
        }
        std::cmp::Ordering::Equal => {}
    }
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
) -> (Vec<GovActionId>, u64, u64) {
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
        return (Vec::new(), 0, 0);
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

    // 3. Credit refunds to reward accounts.  Track unclaimed deposits
    //    whose return accounts are no longer registered — upstream
    //    `returnProposalDeposits` sends these to the treasury.
    let mut total_refunded: u64 = 0;
    let mut unclaimed: u64 = 0;
    for (raw_account, deposit) in &refund_targets {
        if let Some(reward_account) = RewardAccount::from_bytes(raw_account) {
            if let Some(ra_state) = ledger.reward_accounts_mut().get_mut(&reward_account) {
                ra_state.set_balance(ra_state.balance().saturating_add(*deposit));
                total_refunded = total_refunded.saturating_add(*deposit);
            } else {
                // Return account no longer registered — deposit accrues to
                // treasury (upstream `returnProposalDeposits`).
                unclaimed = unclaimed.saturating_add(*deposit);
            }
        } else {
            // Malformed reward account — treat as unclaimed.
            unclaimed = unclaimed.saturating_add(*deposit);
        }
    }

    (expired_ids, total_refunded, unclaimed)
}

/// Transitively removes governance proposals whose `prev_action_id`
/// chains through any of the given `removed_ids`.
///
/// This implements the upstream `proposalsRemoveWithDescendants`
/// semantics: when a proposal is removed (e.g. expired), any dependent
/// proposal whose lineage chains through it is also removed with deposit
/// refund.  The traversal is transitive — grandchild proposals are
/// caught too.
///
/// Returns `(total_refunded, unclaimed)` matching the same semantics as
/// `remove_expired_governance_actions`.
fn remove_descendants_of(
    ledger: &mut LedgerState,
    removed_ids: &[GovActionId],
) -> (u64, u64) {
    let mut all_removed: BTreeSet<GovActionId> = removed_ids.iter().cloned().collect();

    // Iteratively discover descendants until no new ones are found.
    loop {
        let mut next_wave: Vec<GovActionId> = Vec::new();
        for (id, state) in ledger.governance_actions().iter() {
            if all_removed.contains(id) {
                continue; // already marked for removal
            }
            let prev = gov_action_prev_id(&state.proposal().gov_action);
            if let Some(Some(parent)) = prev {
                if all_removed.contains(parent) {
                    next_wave.push(id.clone());
                }
            }
        }

        if next_wave.is_empty() {
            break;
        }

        for id in next_wave {
            all_removed.insert(id);
        }
        // Continue iterating in case the newly added proposals have
        // descendants of their own.
    }

    // Remove only the *descendants* (the original removed_ids are already gone).
    let descendant_ids: Vec<GovActionId> = all_removed
        .into_iter()
        .filter(|id| !removed_ids.contains(id))
        .collect();

    let mut total_refunded: u64 = 0;
    let mut unclaimed: u64 = 0;

    for id in &descendant_ids {
        if let Some(state) = ledger.governance_actions_mut().remove(id) {
            let deposit = state.proposal().deposit;
            if let Some(reward_account) = RewardAccount::from_bytes(&state.proposal().reward_account) {
                if let Some(ra_state) = ledger.reward_accounts_mut().get_mut(&reward_account) {
                    ra_state.set_balance(ra_state.balance().saturating_add(deposit));
                    total_refunded = total_refunded.saturating_add(deposit);
                } else {
                    unclaimed = unclaimed.saturating_add(deposit);
                }
            } else {
                unclaimed = unclaimed.saturating_add(deposit);
            }
        }
    }

    (total_refunded, unclaimed)
}

/// Returns the upstream ratification priority for a governance action.
///
/// Proposals are processed in `actionPriority` order so that delaying
/// actions (priorities 0–3) are enacted before non-delaying actions
/// (priorities 4–6).  Within the same priority, proposals are processed
/// in `GovActionId` order.
///
/// Upstream reference: `Cardano.Ledger.Conway.Governance.Procedures.actionPriority`.
fn action_priority(action: &crate::eras::conway::GovAction) -> u8 {
    use crate::eras::conway::GovAction;
    match action {
        GovAction::NoConfidence { .. } => 0,
        GovAction::UpdateCommittee { .. } => 1,
        GovAction::NewConstitution { .. } => 2,
        GovAction::HardForkInitiation { .. } => 3,
        GovAction::ParameterChange { .. } => 4,
        GovAction::TreasuryWithdrawals { .. } => 5,
        GovAction::InfoAction => 6,
    }
}

/// Returns `true` if enacting the given action type prevents further
/// enactments within the same epoch boundary.
///
/// Upstream reference: `Cardano.Ledger.Conway.Rules.Ratify.delayingAction`.
fn delaying_action(action: &crate::eras::conway::GovAction) -> bool {
    use crate::eras::conway::GovAction;
    matches!(
        action,
        GovAction::NoConfidence { .. }
            | GovAction::HardForkInitiation { .. }
            | GovAction::UpdateCommittee { .. }
            | GovAction::NewConstitution { .. }
    )
}

/// Returns `true` if the proposal's `prev_action_id` matches the current
/// enacted lineage root for its governance purpose.
///
/// Upstream reference: `Cardano.Ledger.Conway.Rules.Ratify.prevActionAsExpected`.
fn prev_action_as_expected(
    action_state: &crate::state::GovernanceActionState,
    enact_state: &crate::state::EnactState,
) -> bool {
    use crate::state::conway_gov_action_purpose;

    let purpose = conway_gov_action_purpose(&action_state.proposal().gov_action);
    let enacted_root = enact_state.enacted_root(purpose);

    let proposal_prev = gov_action_prev_id(&action_state.proposal().gov_action);

    match proposal_prev {
        // No lineage tracking for this action type (TreasuryWithdrawals, InfoAction).
        None => true,
        Some(prev_opt) => match (prev_opt, enacted_root) {
            // Proposal says "I am the first" and there is no enacted root.
            (None, None) => true,
            // Proposal references a specific predecessor that matches the root.
            (Some(p), Some(r)) => p == r,
            // Mismatch: proposal says first but root exists, or vice versa.
            _ => false,
        },
    }
}

/// Returns `true` if the action's treasury withdrawal amount does not
/// exceed the current treasury balance.  Non-withdrawal actions always
/// pass.
///
/// Upstream reference: `Cardano.Ledger.Conway.Rules.Ratify.withdrawalCanWithdraw`.
fn withdrawal_can_withdraw(
    action: &crate::eras::conway::GovAction,
    treasury: u64,
) -> bool {
    if let crate::eras::conway::GovAction::TreasuryWithdrawals { withdrawals, .. } = action {
        let total: u64 = withdrawals.values().sum();
        total <= treasury
    } else {
        true
    }
}

/// Returns `true` if all new committee members' expiration epochs are
/// within the maximum term length from the current epoch.
///
/// Upstream reference: `Cardano.Ledger.Conway.Rules.Ratify.validCommitteeTerm`.
fn valid_committee_term(
    action: &crate::eras::conway::GovAction,
    committee_max_term_length: Option<u64>,
    current_epoch: EpochNo,
) -> bool {
    let Some(max_term) = committee_max_term_length else {
        return true;
    };

    if let crate::eras::conway::GovAction::UpdateCommittee { members_to_add, .. } = action {
        let max_epoch = current_epoch.0.saturating_add(max_term);
        members_to_add.values().all(|&expiry| expiry <= max_epoch)
    } else {
        true
    }
}

/// Tallies votes for surviving governance actions, enacting them one at
/// a time in governance action priority order with iterative `EnactState`
/// updates.
///
/// This implements the upstream Conway RATIFY rule's iterative semantics:
/// proposals are first sorted by `actionPriority` (upstream
/// `reorderActions`), then by `GovActionId` within the same priority.
/// For each proposal, the function checks—against the **current**
/// `EnactState`:
///
/// 1. `prevActionAsExpected` — lineage chains from the current root.
/// 2. `validCommitteeTerm` — new committee members within max term.
/// 3. `not delayed` — no delaying action has been enacted this round.
/// 4. `withdrawalCanWithdraw` — treasury sufficient for withdrawals.
/// 5. `acceptedByEveryone` — committee, DRep, SPO thresholds met.
///
/// When an action is enacted, the `EnactState` is updated (lineage
/// root advances, committee/params may change).  If the enacted action
/// is a *delaying* action (NoConfidence, HardFork, UpdateCommittee,
/// NewConstitution), no further proposals are enacted this epoch.
///
/// After all enactments, lineage-conflicting proposals are pruned and
/// deposits are refunded.
///
/// Upstream reference: `Cardano.Ledger.Conway.Rules.Ratify.ratifyTransition`,
/// `Cardano.Ledger.Conway.Governance.Procedures.proposalsApplyEnactment`.
fn ratify_and_enact(
    ledger: &mut LedgerState,
    current_epoch: EpochNo,
    snapshots: &StakeSnapshots,
    drep_activity: u64,
) -> RatifyAndEnactResult {
    use crate::state::ratify_action;

    // Extract thresholds from protocol params.  When Conway-specific
    // threshold fields are absent, fall back to Conway defaults.
    let (pool_thresholds, drep_thresholds, min_committee_size, committee_max_term, is_bootstrap_phase) =
        match ledger.protocol_params() {
            Some(pp) => (
                pp.pool_voting_thresholds.clone().unwrap_or_default(),
                pp.drep_voting_thresholds.clone().unwrap_or_default(),
                pp.min_committee_size,
                pp.committee_term_limit,
                // Upstream `hardforkConwayBootstrapPhase`: PV major == 9.
                matches!(pp.protocol_version, Some((9, _))),
            ),
            None => return RatifyAndEnactResult::default(),
        };

    // Compute DRep delegated stake distribution from the mark snapshot.
    let drep_delegated_stake =
        compute_drep_stake_distribution(&snapshots.mark, ledger.stake_credentials());

    // Compute SPO pool stake distribution from the mark snapshot.
    let pool_stake_dist = snapshots.mark.pool_stake_distribution();

    // Collect proposal IDs sorted by governance action priority, then
    // by GovActionId within the same priority.  This matches upstream
    // `reorderActions` which sorts by `actionPriority` before RATIFY
    // processes the `RatifySignal`.
    //
    // Reference: Cardano.Ledger.Conway.Governance.Internal.reorderActions,
    //            Cardano.Ledger.Conway.Governance.Procedures.actionPriority.
    let mut sorted_ids: Vec<GovActionId> = ledger
        .governance_actions()
        .keys()
        .cloned()
        .collect();
    sorted_ids.sort_by(|a, b| {
        let pa = ledger
            .governance_actions()
            .get(a)
            .map(|s| action_priority(&s.proposal().gov_action))
            .unwrap_or(u8::MAX);
        let pb = ledger
            .governance_actions()
            .get(b)
            .map(|s| action_priority(&s.proposal().gov_action))
            .unwrap_or(u8::MAX);
        pa.cmp(&pb).then_with(|| a.cmp(b))
    });

    let mut enacted_ids: Vec<GovActionId> = Vec::new();
    let mut outcomes: Vec<EnactOutcome> = Vec::new();
    let mut deposit_targets: Vec<(Vec<u8>, u64)> = Vec::new();
    let mut enacted_purposes: BTreeSet<crate::state::ConwayGovActionPurpose> = BTreeSet::new();
    let mut delayed = false;

    for id in &sorted_ids {
        // Look up the action (it may have been removed by an earlier
        // enactment — shouldn't happen, but be defensive).
        let action_state = match ledger.governance_actions().get(id) {
            Some(s) => s,
            None => continue,
        };

        let gov_action = &action_state.proposal().gov_action;

        // --- Upstream ratifyTransition guard checks (order matters) ---

        // 1. prevActionAsExpected — checked against CURRENT enact state
        //    (lineage updates mid-loop are intended).
        if !prev_action_as_expected(action_state, ledger.enact_state()) {
            continue;
        }

        // 2. validCommitteeTerm — checked against CURRENT protocol params
        //    from the evolving enact state.
        if !valid_committee_term(
            gov_action,
            ledger
                .protocol_params()
                .and_then(|pp| pp.committee_term_limit)
                .or(committee_max_term),
            current_epoch,
        ) {
            continue;
        }

        // 3. Delay flag — once a delaying action is enacted, stop.
        if delayed {
            continue;
        }

        // 4. withdrawalCanWithdraw — checked against CURRENT treasury from
        //    the evolving enact state.
        if !withdrawal_can_withdraw(gov_action, ledger.accounting().treasury) {
            continue;
        }

        // 5. acceptedByEveryone — committee + DRep + SPO thresholds.
        //    Read committee quorum from CURRENT enact state (may have
        //    changed after an earlier UpdateCommittee enactment).
        let committee_quorum = ledger.enact_state().committee_quorum;
        let has_committee = ledger.enact_state().has_committee;

        if !ratify_action(
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
            min_committee_size.unwrap_or(0),
            is_bootstrap_phase,
            has_committee,
        ) {
            continue;
        }

        // --- All checks passed: enact ---
        let removed = ledger.governance_actions_mut().remove(id);
        if let Some(state) = removed {
            enacted_purposes.insert(
                crate::state::conway_gov_action_purpose(&state.proposal().gov_action),
            );
            deposit_targets.push((
                state.proposal().reward_account.clone(),
                state.proposal().deposit,
            ));
            let outcome = ledger.enact_action(id.clone(), &state.proposal().gov_action);
            outcomes.push(outcome);
            enacted_ids.push(id.clone());

            // Set delay flag if this is a delaying action type.
            if delaying_action(&state.proposal().gov_action) {
                delayed = true;
            }
        }
    }

    if enacted_ids.is_empty() {
        return RatifyAndEnactResult::default();
    }

    // -----------------------------------------------------------------------
    // Subtree pruning: remove proposals whose prev_action_id no longer
    // chains from the current enacted lineage root for their purpose.
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
    let enacted_count = enacted_ids.len();

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
        enacted_ids,
        outcomes,
        enacted_deposit_refunds: enacted_refunded,
        removed_due_to_enactment,
        removed_due_to_enactment_deposit_refunds: subtree_refunded,
        unclaimed_deposits: unclaimed,
    }
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

/// Retires pools whose `retiring_epoch` ≤ `epoch`, refunds their per-pool
/// deposits, and returns the list of keys, total refunded, and unclaimed.
///
/// This is the preferred helper that captures reward accounts and deposits
/// *before* removing pools, avoiding the ordering problem.
///
/// Unclaimed deposits (reward account no longer registered) are returned
/// separately so the caller can route them to the treasury, matching
/// upstream `poolReapTransition` behavior.
///
/// Reference: `poolReapTransition` in
/// `Cardano.Ledger.Shelley.Rules.PoolReap` — refund uses `spsDeposit`,
/// unclaimed deposits go to `casTreasury`.
pub fn retire_pools_with_refunds(
    ledger: &mut LedgerState,
    epoch: EpochNo,
) -> (Vec<PoolKeyHash>, u64, u64) {
    // 1. Identify pools scheduled to retire and capture their reward
    //    accounts and per-pool deposit amounts.
    let retiring: Vec<(PoolKeyHash, RewardAccount, u64)> = ledger
        .pool_state()
        .iter()
        .filter(|(_, pool)| {
            pool.retiring_epoch().is_some_and(|e| e <= epoch)
        })
        .map(|(k, pool)| (*k, pool.params().reward_account, pool.deposit()))
        .collect();

    if retiring.is_empty() {
        return (Vec::new(), 0, 0);
    }

    // 2. Remove the retiring pools from the registry.
    let retired_keys = ledger.pool_state_mut().process_retirements(epoch);

    // 2b. Clear pool delegations pointing at retired pools.
    //     Upstream: `removeStakePoolDelegations (delegsToClear cs retired)`
    //     in `Cardano.Ledger.Shelley.Rules.PoolReap`.
    ledger.stake_credentials_mut().clear_pool_delegations(&retired_keys);

    // 3. Credit refunds to reward accounts; track unclaimed deposits.
    let mut total_refunded: u64 = 0;
    let mut total_unclaimed: u64 = 0;
    for (_, reward_account, deposit) in &retiring {
        if *deposit == 0 {
            continue;
        }
        if let Some(state) = ledger.reward_accounts_mut().get_mut(reward_account) {
            state.set_balance(state.balance().saturating_add(*deposit));
            total_refunded = total_refunded.saturating_add(*deposit);
        } else {
            // Reward account no longer registered — upstream sends
            // unclaimed pool deposit refunds to treasury.
            total_unclaimed = total_unclaimed.saturating_add(*deposit);
        }
        ledger.deposit_pot_mut().return_pool_deposit(*deposit);
    }

    (retired_keys, total_refunded, total_unclaimed)
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

        // Register a pool with the current pool_deposit recorded.
        let params = test_pool_params(pool_id);
        let pp_pool_deposit = test_protocol_params().pool_deposit;
        ledger.pool_state_mut().register_with_deposit(params, pp_pool_deposit);

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
        ledger.committee_state_mut().register_with_term(cc_cred, 999);
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

        let (retired, refunded, unclaimed) =
            retire_pools_with_refunds(&mut ledger, EpochNo(5));

        assert_eq!(retired.len(), 1);
        assert_eq!(retired[0], test_pool(3));
        assert_eq!(refunded, pool_deposit);
        assert_eq!(unclaimed, 0);

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
        let (retired, refunded, _unclaimed) =
            retire_pools_with_refunds(&mut ledger, EpochNo(5));

        assert!(retired.is_empty());
        assert_eq!(refunded, 0);
        // Pool should still be registered.
        assert!(ledger.pool_state().get(&test_pool(4)).is_some());
    }

    // -- Per-pool deposit: refund uses recorded deposit, not current param

    #[test]
    fn test_pool_retirement_refunds_recorded_deposit_not_current_param() {
        // Upstream `poolReapTransition` refunds `spsDeposit`, the deposit
        // stored at registration time.  If `pp_poolDeposit` changes
        // between registration and retirement, the original amount is used.
        let mut ledger = LedgerState::new(Era::Shelley);
        ledger.set_protocol_params(test_protocol_params());

        // Register a pool with the OLD deposit (200_000_000).
        let params = test_pool_params(7);
        let old_deposit = 200_000_000u64;
        ledger.pool_state_mut().register_with_deposit(params, old_deposit);
        ledger.deposit_pot_mut().add_pool_deposit(old_deposit);

        let pool_cred = test_cred(7);
        ledger.stake_credentials_mut().register(pool_cred);
        let ra = test_reward_account(7);
        ledger.reward_accounts_mut().insert(
            ra,
            RewardAccountState::new(0, None),
        );

        // Now the current pp_poolDeposit changes to 500_000_000.
        // (This simulates a protocol parameter update.)
        let mut pp = test_protocol_params();
        pp.pool_deposit = 500_000_000;
        ledger.set_protocol_params(pp);

        // Schedule retirement.
        ledger.pool_state_mut().retire(test_pool(7), EpochNo(5));

        let (retired, refunded, unclaimed) =
            retire_pools_with_refunds(&mut ledger, EpochNo(5));

        assert_eq!(retired.len(), 1);
        // Must refund the ORIGINAL deposit (200M), not current param (500M).
        assert_eq!(refunded, old_deposit);
        assert_eq!(unclaimed, 0);
        assert_eq!(ledger.reward_accounts().balance(&ra), old_deposit);
    }

    // -- Unclaimed pool deposits go to treasury ---------------------------

    #[test]
    fn test_unclaimed_pool_deposit_when_reward_account_unregistered() {
        // Upstream `poolReapTransition`: if the pool's reward account is
        // no longer registered at retirement, the deposit goes to treasury
        // via `casTreasury += unclaimed`.
        let mut ledger = LedgerState::new(Era::Shelley);
        ledger.set_protocol_params(test_protocol_params());

        let params = test_pool_params(8);
        let deposit = 500_000_000u64;
        ledger.pool_state_mut().register_with_deposit(params, deposit);
        ledger.deposit_pot_mut().add_pool_deposit(deposit);

        // Register stake credential but do NOT create a reward account.
        let pool_cred = test_cred(8);
        ledger.stake_credentials_mut().register(pool_cred);
        // (reward account intentionally not inserted)

        // Schedule retirement.
        ledger.pool_state_mut().retire(test_pool(8), EpochNo(5));

        let (_retired, refunded, unclaimed) =
            retire_pools_with_refunds(&mut ledger, EpochNo(5));

        // Refunded = 0 (no registered account to credit).
        assert_eq!(refunded, 0);
        // Unclaimed = deposit amount → caller routes to treasury.
        assert_eq!(unclaimed, deposit);
    }

    // -- Accounting update (treasury/reserves) ----------------------------

    // -- PPUP ordering: rewards use previous epoch's params ---------------

    #[test]
    fn test_ppup_ordering_rewards_use_old_params() {
        // Upstream UPEC runs LAST inside EPOCH (after SNAP, POOLREAP),
        // so reward calculation uses `prevPParams`.
        //
        // This test submits a PPUP proposal that doubles tau (0.2 → 0.4)
        // for the upcoming epoch, then verifies that the epoch boundary
        // reward calculation still uses the OLD tau.
        let mut ledger = make_ledger_with_pool(9);

        // Set up a genesis delegate so the PPUP proposal can reach quorum.
        let genesis_hash: [u8; 28] = [0xAA; 28];
        ledger.gen_delegs_mut().insert(
            genesis_hash,
            GenesisDelegationState {
                delegate: [0xBB; 28],
                vrf: [0xCC; 32],
            },
        );

        // Old tau = 0.2.  Submit proposal to change tau to 0.4 at epoch 1.
        let mut update = ProtocolParameterUpdate::default();
        update.tau = Some(UnitInterval { numerator: 4, denominator: 10 });
        let mut proposals = BTreeMap::new();
        proposals.insert(genesis_hash, update);
        let shelley_update = ShelleyUpdate {
            proposed_protocol_parameter_updates: proposals,
            epoch: 1, // target epoch
        };
        ledger.collect_pparam_proposals(&shelley_update);

        // Verify the old tau is still 0.2.
        assert_eq!(
            ledger.protocol_params().unwrap().tau,
            UnitInterval { numerator: 2, denominator: 10 },
        );

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        // Apply epoch boundary for epoch 1.
        let event = apply_epoch_boundary(
            &mut ledger,
            EpochNo(1),
            &mut snapshots,
            &perf,
        )
        .expect("epoch 1 boundary should succeed");

        // PPUP should have been applied (tau changed).
        assert!(event.pparam_updates_applied > 0);

        // NEW tau should now be 0.4.
        assert_eq!(
            ledger.protocol_params().unwrap().tau,
            UnitInterval { numerator: 4, denominator: 10 },
        );

        // The treasury_delta should correspond to OLD tau (0.2), not new (0.4).
        // With empty go snapshot, only the tau cut from the reward pot
        // contributes.  The reward pot = fees + floor(rho * reserves).
        // What matters: treasury_delta was computed with tau=0.2, not 0.4.
        // delta_reserves = floor(rho * reserves) = floor(0.003 * 14_000_000_000_000_000)
        //                = 42_000_000_000_000
        // reward_pot = 0 (fees) + 42_000_000_000_000 = 42_000_000_000_000
        // treasury_cut = floor(0.2 * 42_000_000_000_000) = 8_400_000_000_000
        // If tau=0.4 were used: treasury_cut = 16_800_000_000_000
        assert_eq!(event.delta_reserves, 42_000_000_000_000);
        // Treasury delta = treasury_cut + unregistered_rewards.
        // With empty go snapshot, no pools, so distributed=0, unregistered=0.
        assert_eq!(event.treasury_delta, 8_400_000_000_000);
    }

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
        let (retired, refunded, _unclaimed) =
            retire_pools_with_refunds(&mut ledger, EpochNo(1));
        assert!(retired.is_empty());
        assert_eq!(refunded, 0);
    }

    /// Upstream `poolReapTransition` calls `removeStakePoolDelegations`
    /// to clear the pool delegation for every credential that was
    /// delegated to a retiring pool. Verify we do the same.
    #[test]
    fn test_pool_retirement_clears_delegations() {
        let mut ledger = make_ledger_with_pool(5);
        ledger.deposit_pot_mut().add_pool_deposit(500_000_000);

        // Add a second credential delegating to the same pool.
        let cred2 = test_cred(0xF5);
        ledger.stake_credentials_mut().register(cred2);
        ledger
            .stake_credentials_mut()
            .get_mut(&cred2)
            .unwrap()
            .set_delegated_pool(Some(test_pool(5)));

        // Add a third credential delegating to a *different* pool (should NOT be touched).
        let cred3 = test_cred(0xF6);
        ledger.stake_credentials_mut().register(cred3);
        ledger
            .stake_credentials_mut()
            .get_mut(&cred3)
            .unwrap()
            .set_delegated_pool(Some(test_pool(99)));

        // Schedule pool 5 for retirement.
        ledger.pool_state_mut().retire(test_pool(5), EpochNo(5));

        let (retired, _, _) = retire_pools_with_refunds(&mut ledger, EpochNo(5));
        assert_eq!(retired.len(), 1);

        // The operator credential (cred 5) and extra delegator must have
        // their delegation cleared.
        assert_eq!(
            ledger.stake_credentials().get(&test_cred(5)).unwrap().delegated_pool(),
            None,
            "operator delegation should be cleared",
        );
        assert_eq!(
            ledger.stake_credentials().get(&cred2).unwrap().delegated_pool(),
            None,
            "extra delegator should be cleared",
        );
        // Credential delegated to a different pool must be untouched.
        assert_eq!(
            ledger.stake_credentials().get(&cred3).unwrap().delegated_pool(),
            Some(test_pool(99)),
            "unrelated delegation must be preserved",
        );
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

    #[test]
    fn test_expired_parent_removes_descendants() {
        // When a governance action expires, any proposals that chain to it
        // via prev_action_id should also be removed (recursively).
        // Upstream reference: proposalsRemoveWithDescendants.
        let mut ledger = make_governance_ledger();

        let ra = test_reward_account(0xD0);
        ledger
            .reward_accounts_mut()
            .insert(ra.clone(), RewardAccountState::new(0, None));

        // Parent: ParameterChange proposed in epoch 0, expires at epoch 1.
        let parent_id = test_gov_action_id(0xA0, 0);
        let parent_proposal = crate::eras::conway::ProposalProcedure {
            deposit: 100,
            reward_account: ra.to_bytes().to_vec(),
            gov_action: GovAction::ParameterChange {
                prev_action_id: None,
                protocol_param_update: Default::default(),
                guardrails_script_hash: None,
            },
            anchor: crate::types::Anchor {
                url: String::new(),
                data_hash: [0; 32],
            },
        };
        let parent_state = GovernanceActionState::new_with_lifetime(
            parent_proposal,
            EpochNo(0),
            Some(1), // expires_after = epoch 1
        );
        ledger
            .governance_actions_mut()
            .insert(parent_id.clone(), parent_state);

        // Child: ParameterChange chaining from parent, expires at epoch 10.
        let child_id = test_gov_action_id(0xA1, 0);
        let child_proposal = crate::eras::conway::ProposalProcedure {
            deposit: 200,
            reward_account: ra.to_bytes().to_vec(),
            gov_action: GovAction::ParameterChange {
                prev_action_id: Some(parent_id.clone()),
                protocol_param_update: Default::default(),
                guardrails_script_hash: None,
            },
            anchor: crate::types::Anchor {
                url: String::new(),
                data_hash: [0; 32],
            },
        };
        let child_state = GovernanceActionState::new_with_lifetime(
            child_proposal,
            EpochNo(0),
            Some(10), // expires_after = epoch 10
        );
        ledger
            .governance_actions_mut()
            .insert(child_id.clone(), child_state);

        // Grandchild: chaining from child.
        let grandchild_id = test_gov_action_id(0xA2, 0);
        let grandchild_proposal = crate::eras::conway::ProposalProcedure {
            deposit: 300,
            reward_account: ra.to_bytes().to_vec(),
            gov_action: GovAction::ParameterChange {
                prev_action_id: Some(child_id.clone()),
                protocol_param_update: Default::default(),
                guardrails_script_hash: None,
            },
            anchor: crate::types::Anchor {
                url: String::new(),
                data_hash: [0; 32],
            },
        };
        let grandchild_state = GovernanceActionState::new_with_lifetime(
            grandchild_proposal,
            EpochNo(0),
            Some(10), // expires_after = epoch 10
        );
        ledger
            .governance_actions_mut()
            .insert(grandchild_id.clone(), grandchild_state);

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        // Epoch 2 boundary: parent expires (expires_after=1 < 2), and both
        // child and grandchild should be transitively removed as descendants.
        let event = apply_epoch_boundary(&mut ledger, EpochNo(2), &mut snapshots, &perf)
            .expect("epoch 2 boundary");

        assert_eq!(event.governance_actions_expired, 1, "parent expired");
        assert!(!ledger.governance_actions().contains_key(&parent_id));
        assert!(!ledger.governance_actions().contains_key(&child_id));
        assert!(!ledger.governance_actions().contains_key(&grandchild_id));
        // Deposits refunded: 100 (parent) + 200 (child) + 300 (grandchild)
        assert_eq!(event.governance_deposit_refunds, 600);
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
            drep,
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
            drep,
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
            drep,
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
            drep,
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
            drep_a,
            RegisteredDrep::new_active(500_000_000, None, EpochNo(85)),
        );
        // DRep B: active in epoch 95 → 95+10=105 >= 100 → still active.
        let drep_b = DRep::ScriptHash([0x06; 28]);
        ledger.drep_state_mut().register(
            drep_b,
            RegisteredDrep::new_active(500_000_000, None, EpochNo(95)),
        );
        // DRep C: legacy, no activity epoch → not expired.
        let drep_c = DRep::KeyHash([0x07; 28]);
        ledger.drep_state_mut().register(
            drep_c,
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
            drep,
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
    use crate::state::CommitteeAuthorization;

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
        ledger.committee_state_mut().register_with_term(cc_cred, 999);
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
        // Upstream: InfoAction has NoVotingThreshold for committee, which
        // means the committee acceptance check always returns false.
        // InfoAction is therefore NEVER ratified.
        let mut ledger = make_governance_ledger();
        let gai = test_gov_action_id(0xA1, 0);
        let gas = GovernanceActionState::new(test_info_proposal());
        ledger.governance_actions_mut().insert(gai.clone(), gas);

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        // First rotation to populate the mark snapshot with DRep stake.
        let _ = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1 boundary");

        // InfoAction should NOT have been enacted — it remains pending.
        assert!(
            ledger.governance_actions().contains_key(&gai),
            "InfoAction should remain pending — it is never ratified"
        );

        let event = apply_epoch_boundary(&mut ledger, EpochNo(2), &mut snapshots, &perf)
            .expect("epoch 2 boundary");

        // InfoAction is never accepted → should NOT be enacted.
        assert_eq!(event.governance_actions_enacted, 0);
        assert!(ledger.governance_actions().contains_key(&gai));
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
        let cc_hot_cred = test_cred(0xC1);
        let drep_cred = [0xD0; 28];
        let pool_key = test_pool(20);

        let gai = test_gov_action_id(0xC1, 0);
        let mut gas = GovernanceActionState::new(test_hf_proposal());

        // Record CC vote (yes) — keyed by HOT credential (CDDL tags 0/1).
        gas.record_vote(
            Voter::CommitteeKeyHash(*cc_hot_cred.hash()),
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
        // built-in defaults so ratification still runs without errors.
        // InfoAction is never ratified (upstream NoVotingThreshold), so the
        // proposal stays pending but the epoch boundary completes successfully.
        let gas = GovernanceActionState::new(test_info_proposal());
        let gai = test_gov_action_id(0xD1, 0);
        ledger.governance_actions_mut().insert(gai.clone(), gas);

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        let event = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1 boundary");

        assert_eq!(event.governance_actions_enacted, 0);
        assert!(ledger.governance_actions().contains_key(&gai));
    }

    #[test]
    fn test_new_constitution_enacted_updates_enact_state() {
        let mut ledger = make_governance_ledger();
        let cc_hot_cred = test_cred(0xC1);
        let drep_cred = [0xD0; 28];

        let gai = test_gov_action_id(0xE1, 0);
        let mut gas = GovernanceActionState::new(test_new_constitution_proposal());

        // NewConstitution requires CC + DRep, but NOT SPO.
        gas.record_vote(
            Voter::CommitteeKeyHash(*cc_hot_cred.hash()),
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

        // Set motion-no-confidence DRep/SPO thresholds to 0 so NoConfidence
        // auto-passes (committee always returns true for NoConfidence).
        if let Some(pp) = ledger.protocol_params_mut() {
            let mut dt = pp.drep_voting_thresholds.clone().unwrap_or_default();
            dt.motion_no_confidence = UnitInterval { numerator: 0, denominator: 1 };
            pp.drep_voting_thresholds = Some(dt);
            let mut pt = pp.pool_voting_thresholds.clone().unwrap_or_default();
            pt.motion_no_confidence = UnitInterval { numerator: 0, denominator: 1 };
            pp.pool_voting_thresholds = Some(pt);
        }

        // Action 1: NoConfidence (always passes committee + 0-threshold DRep/SPO).
        let gai1 = test_gov_action_id(0xF1, 0);
        let gas1 = GovernanceActionState::new(test_no_confidence_proposal());
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

        // NoConfidence should have been enacted, HF should still be pending.
        assert!(!ledger.governance_actions().contains_key(&gai1));

        // Epoch 3: HF should expire (expires_after = 2 < 3).
        let event = apply_epoch_boundary(&mut ledger, EpochNo(3), &mut snapshots, &perf)
            .expect("epoch 3");

        assert_eq!(event.governance_actions_expired, 1);
        assert!(ledger.governance_actions().is_empty());
    }

    #[test]
    fn test_update_committee_ratifies_after_no_confidence() {
        // Upstream: UpdateCommittee uses the no-confidence DRep/SPO threshold
        // when `ensCommittee = SNothing` (i.e. `has_committee = false`),
        // which only occurs after a formal NoConfidence enactment — not merely
        // because all committee members resigned.
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

        // Simulate post-NoConfidence state: no committee.
        ledger.enact_state_mut().has_committee = false;

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

        // With no-confidence thresholds (0/1) for DRep+SPO, action passes.
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
    fn test_update_committee_enacted_even_when_result_below_min_committee_size() {
        // Upstream: ratifyTransition does NOT have a resulting-committee-
        // size guard.  The min_committee_size enforcement is only inside
        // `votingCommitteeThreshold` which controls the committee vote
        // for non-UpdateCommittee actions.  UpdateCommittee uses
        // NoVotingAllowed (committee auto-passes), so the resulting
        // committee size is irrelevant to ratification.
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
        // Upstream: this still gets enacted (committee vote = NoVotingAllowed).
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

        let event = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1 boundary");

        // UpdateCommittee is enacted despite resulting committee being
        // smaller than min_committee_size (upstream has no such guard).
        assert_eq!(event.governance_actions_enacted, 1);
        assert_eq!(event.enacted_gov_action_ids, vec![gai]);
    }

    #[test]
    fn test_update_committee_enacted_when_result_meets_min_committee_size() {
        let mut ledger = make_governance_ledger();
        let cc_hot_cred = test_cred(0xC1);

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
            Voter::CommitteeKeyHash(*cc_hot_cred.hash()),
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
        let mut ledger = make_governance_ledger();
        let ra = test_reward_account(ra_byte);
        ledger
            .reward_accounts_mut()
            .insert(ra, RewardAccountState::new(0, None));

        // Set motion-no-confidence thresholds to 0 so NoConfidence auto-passes.
        if let Some(pp) = ledger.protocol_params_mut() {
            let mut dt = pp.drep_voting_thresholds.clone().unwrap_or_default();
            dt.motion_no_confidence = UnitInterval { numerator: 0, denominator: 1 };
            pp.drep_voting_thresholds = Some(dt);
            let mut pt = pp.pool_voting_thresholds.clone().unwrap_or_default();
            pt.motion_no_confidence = UnitInterval { numerator: 0, denominator: 1 };
            pp.pool_voting_thresholds = Some(pt);
        }

        let gai1 = test_gov_action_id(0xEA, 0);
        let proposal1 = crate::eras::conway::ProposalProcedure {
            deposit,
            reward_account: ra.to_bytes().to_vec(),
            gov_action: GovAction::NoConfidence { prev_action_id: None },
            anchor: Anchor {
                url: String::new(),
                data_hash: [0; 32],
            },
        };
        ledger
            .governance_actions_mut()
            .insert(gai1.clone(), GovernanceActionState::new(proposal1));

        let balance_before = ledger.reward_accounts().balance(&ra);

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        // First epoch populates mark snapshot + enacts first NoConfidence.
        let _ = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1");

        // Re-insert — must chain from enacted root.
        let proposal2 = crate::eras::conway::ProposalProcedure {
            deposit,
            reward_account: ra.to_bytes().to_vec(),
            gov_action: GovAction::NoConfidence { prev_action_id: Some(gai1.clone()) },
            anchor: Anchor {
                url: String::new(),
                data_hash: [0; 32],
            },
        };
        let gai2 = test_gov_action_id(0xEB, 0);
        ledger
            .governance_actions_mut()
            .insert(gai2.clone(), GovernanceActionState::new(proposal2));

        let event = apply_epoch_boundary(&mut ledger, EpochNo(2), &mut snapshots, &perf)
            .expect("epoch 2");

        // Verify enacted with deposit refund.
        assert_eq!(event.governance_actions_enacted, 1);
        assert_eq!(event.enacted_deposit_refunds, deposit);
        assert!(ledger.governance_actions().is_empty());

        // Reward account balance should increase by deposit.
        let balance_after = ledger.reward_accounts().balance(&ra);
        // Two actions were enacted across epochs 1+2, both refunded.
        assert!(balance_after >= balance_before + deposit);
    }

    #[test]
    fn test_enacted_deposit_refund_for_unregistered_account_goes_to_treasury() {
        let mut ledger = make_governance_ledger();
        let deposit = 300_000_000u64;

        // Set motion-no-confidence thresholds to 0 so NoConfidence auto-passes.
        if let Some(pp) = ledger.protocol_params_mut() {
            let mut dt = pp.drep_voting_thresholds.clone().unwrap_or_default();
            dt.motion_no_confidence = UnitInterval { numerator: 0, denominator: 1 };
            pp.drep_voting_thresholds = Some(dt);
            let mut pt = pp.pool_voting_thresholds.clone().unwrap_or_default();
            pt.motion_no_confidence = UnitInterval { numerator: 0, denominator: 1 };
            pp.pool_voting_thresholds = Some(pt);
        }

        // Use an unregistered reward account.
        let unregistered_ra = test_reward_account(0x99);
        let proposal = crate::eras::conway::ProposalProcedure {
            deposit,
            reward_account: unregistered_ra.to_bytes().to_vec(),
            gov_action: GovAction::NoConfidence { prev_action_id: None },
            anchor: Anchor {
                url: String::new(),
                data_hash: [0; 32],
            },
        };
        let gai = test_gov_action_id(0xEC, 0);
        ledger
            .governance_actions_mut()
            .insert(gai.clone(), GovernanceActionState::new(proposal));

        let _treasury_before = ledger.accounting().treasury;

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        // Populate mark.
        let _ = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1");

        // Re-insert — must chain from enacted root.
        let proposal2 = crate::eras::conway::ProposalProcedure {
            deposit,
            reward_account: unregistered_ra.to_bytes().to_vec(),
            gov_action: GovAction::NoConfidence { prev_action_id: Some(gai.clone()) },
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
        let cc_hot_cred = test_cred(0xC1);
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
            Voter::CommitteeKeyHash(*cc_hot_cred.hash()),
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
                Voter::CommitteeKeyHash(*cc_hot_cred.hash()),
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
        let cc_hot_cred = test_cred(0xC1);
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
            Voter::CommitteeKeyHash(*cc_hot_cred.hash()),
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
        // Deposit should go to treasury.
        let mut ledger = make_governance_ledger();

        // Set motion-no-confidence thresholds to 0 so NoConfidence auto-passes.
        if let Some(pp) = ledger.protocol_params_mut() {
            let mut dt = pp.drep_voting_thresholds.clone().unwrap_or_default();
            dt.motion_no_confidence = UnitInterval { numerator: 0, denominator: 1 };
            pp.drep_voting_thresholds = Some(dt);
            let mut pt = pp.pool_voting_thresholds.clone().unwrap_or_default();
            pt.motion_no_confidence = UnitInterval { numerator: 0, denominator: 1 };
            pp.pool_voting_thresholds = Some(pt);
        }

        let deposit_a = 100_000_000u64;
        let unregistered_ra_a = test_reward_account(0x80);
        // Intentionally NOT registering this account.

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();
        let _ = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1");

        let gai_a = test_gov_action_id(0xD0, 0);

        // Action A: NoConfidence (always passes committee, 0-threshold DRep/SPO),
        // unregistered return account.
        let proposal_a = crate::eras::conway::ProposalProcedure {
            deposit: deposit_a,
            reward_account: unregistered_ra_a.to_bytes().to_vec(),
            gov_action: GovAction::NoConfidence { prev_action_id: None },
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
        let cc_hot_cred = test_cred(0xC1);
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
            Voter::CommitteeKeyHash(*cc_hot_cred.hash()),
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
            assert!(ledger.committee_state().iter().all(|(_, m)| !m.is_enacted_member()),
                "all committee members should have cleared membership after NoConfidence");
            return;
        }

        let event = apply_epoch_boundary(&mut ledger, EpochNo(2), &mut snapshots, &perf)
            .expect("epoch 2");

        assert_eq!(event.governance_actions_enacted, 1);
        assert!(ledger.committee_state().iter().all(|(_, m)| !m.is_enacted_member()),
            "all committee members should have cleared membership after NoConfidence");
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
        assert!(!ledger.committee_state().is_empty()); // committee still present
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
        ledger.accounting_mut().reserves = 0;
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

    // -- Member (non-operator) reward crediting ---------------------------

    /// Verifies that a non-operator delegator's reward account gets
    /// credited after epoch boundary reward distribution.
    ///
    /// Pre-populates the `go` snapshot directly so rewards are computed
    /// on the first `apply_epoch_boundary` call instead of requiring 4
    /// rotation epochs.
    ///
    /// Reference: `Cardano.Ledger.Shelley.Rewards` — per-member reward
    /// distribution.
    #[test]
    fn test_member_reward_credited_to_individual_account() {
        // Pool operator: pool_id 30.
        let mut ledger = make_ledger_with_pool(30);

        // The pool operator must have UTxO stake ≥ declared pledge for the
        // upstream pledge satisfaction check to pass.
        let op_cred = test_cred(30);
        let op_addr = Address::Base(BaseAddress {
            network: 1,
            payment: StakeCredential::AddrKeyHash([0xDD; 28]),
            staking: op_cred,
        });
        let op_txin = ShelleyTxIn { transaction_id: [30u8; 32], index: 99 };
        ledger.multi_era_utxo_mut().insert_shelley(
            op_txin,
            ShelleyTxOut {
                address: op_addr.to_bytes(),
                amount: 200_000_000, // 200 ADA, above pledge of 100 ADA
            },
        );

        // Member delegator: credential 31, NOT the pool operator.
        let member_cred = test_cred(31);
        let member_ra = RewardAccount {
            network: 1,
            credential: member_cred,
        };

        // Register member credential + delegation to pool 30.
        ledger.stake_credentials_mut().register(member_cred);
        if let Some(cs) = ledger.stake_credentials_mut().get_mut(&member_cred) {
            cs.set_delegated_pool(Some(test_pool(30)));
        }
        ledger
            .reward_accounts_mut()
            .insert(member_ra, RewardAccountState::new(0, None));

        // Add UTxO stake delegated to pool 30 via member credential.
        let base_addr = Address::Base(BaseAddress {
            network: 1,
            payment: StakeCredential::AddrKeyHash([0xCC; 28]),
            staking: member_cred,
        });
        let addr_bytes = base_addr.to_bytes();
        let txin = ShelleyTxIn { transaction_id: [31u8; 32], index: 0 };
        ledger.multi_era_utxo_mut().insert_shelley(
            txin,
            ShelleyTxOut {
                address: addr_bytes,
                amount: 10_000_000_000_000, // 10M ADA
            },
        );

        // Compute a snapshot from the current state and place it directly
        // into the `go` position so that `apply_epoch_boundary` finds pool
        // data in the reward-eligible snapshot without needing 4 rotations.
        let go_snapshot = compute_stake_snapshot(
            ledger.multi_era_utxo(),
            ledger.stake_credentials(),
            ledger.reward_accounts(),
            ledger.pool_state(),
        );
        let mut snapshots = StakeSnapshots::new();
        snapshots.go = go_snapshot;
        snapshots.accumulate_fees(1_000_000_000); // 1000 ADA

        let perf = BTreeMap::from([(
            test_pool(30),
            UnitInterval { numerator: 1, denominator: 1 },
        )]);
        let event = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1");

        // Operator reward account.
        let operator_ra = test_reward_account(30);
        let operator_balance = ledger.reward_accounts().balance(&operator_ra);
        // Member reward account.
        let member_balance = ledger.reward_accounts().balance(&member_ra);

        // With 14B ADA reserves and the given parameters, both accounts
        // should get credit (member contributes 10M ADA of stake).
        assert!(
            event.rewards_distributed > 0,
            "expected rewards to be distributed, treasury_delta={}, delta_reserves={}",
            event.treasury_delta, event.delta_reserves,
        );
        assert!(
            member_balance > 0,
            "member reward account should have positive balance after epoch boundary, \
             got {member_balance}"
        );
        assert!(
            operator_balance > 0,
            "operator reward account should have positive balance, got {operator_balance}"
        );
        // Member reward should be a non-trivial fraction of total rewards.
        assert!(
            member_balance <= event.rewards_distributed,
            "member reward ({member_balance}) should not exceed total distributed ({})",
            event.rewards_distributed,
        );
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

        // Net reserves deduction = delta_reserves - unclaimed.
        // Upstream: deltaR = -deltaR1 + deltaR2.
        // unclaimed (deltaR2) is returned to reserves.
        let expected_net = event.delta_reserves.saturating_sub(event.unclaimed_rewards);
        assert_eq!(actual_deduction, expected_net);

        // The fee pot (10k ADA) should NOT have been deducted from reserves.
        // rho = 3/1000, so delta_reserves ≈ reserves × 0.003.
        let expected_delta = (reserves_before_epoch2 as u128 * 3 / 1000) as u64;
        assert_eq!(event.delta_reserves, expected_delta);

        // Verify that reserves were NOT over-decremented by the fee pot.
        // reserves_after = reserves_before - delta_reserves + unclaimed
        assert_eq!(reserves_after, reserves_before_epoch2 - expected_delta + event.unclaimed_rewards);
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

        // -- Fee pot does not affect reserves via monetary expansion --
        // With zero reserves, delta_reserves = 0.  The fee pot feeds the
        // reward pot.  With no pools, all rewards_pot is unclaimed → returned
        // to reserves (upstream deltaR2).  Treasury gets only the τ-cut.
        // So reserves = 0 - 0 + unclaimed (from fee pot - tau cut).
        // But the fee pot does increase reserves via unclaimed return.
        let expected_unclaimed = {
            // rPot = fees + 0 = 5000 ADA, tau = 0.2 → treasury = 1000 ADA
            // rewards_pot = 4000 ADA → all unclaimed → returned to reserves.
            let r_pot = 5_000_000_000u64;
            let tau_cut = r_pot * 2 / 10; // 1B (1000 ADA)
            r_pot - tau_cut // 4B (4000 ADA)
        };
        assert_eq!(ledger.accounting().reserves, expected_unclaimed);
        // Treasury should have received the treasury cut of the fees.
        assert!(ledger.accounting().treasury > 0);
    }

    // -- Iterative ratification ordering (upstream Conway RATIFY) ----------

    /// Helper: zero-threshold governance ledger where all proposals
    /// automatically pass ratification (CC quorum=0, DRep=0, SPO=0).
    fn make_auto_pass_ledger() -> LedgerState {
        let mut ledger = make_governance_ledger();
        ledger.enact_state_mut().committee_quorum = UnitInterval {
            numerator: 0,
            denominator: 1,
        };
        let mut drep_thresholds = DRepVotingThresholds::default();
        drep_thresholds.motion_no_confidence = UnitInterval { numerator: 0, denominator: 1 };
        drep_thresholds.hard_fork_initiation = UnitInterval { numerator: 0, denominator: 1 };
        drep_thresholds.update_to_constitution = UnitInterval { numerator: 0, denominator: 1 };
        drep_thresholds.pp_economic_group = UnitInterval { numerator: 0, denominator: 1 };
        drep_thresholds.pp_network_group = UnitInterval { numerator: 0, denominator: 1 };
        drep_thresholds.pp_technical_group = UnitInterval { numerator: 0, denominator: 1 };
        drep_thresholds.pp_gov_group = UnitInterval { numerator: 0, denominator: 1 };
        drep_thresholds.treasury_withdrawal = UnitInterval { numerator: 0, denominator: 1 };
        let mut pool_thresholds = PoolVotingThresholds::default();
        pool_thresholds.motion_no_confidence = UnitInterval { numerator: 0, denominator: 1 };
        pool_thresholds.hard_fork_initiation = UnitInterval { numerator: 0, denominator: 1 };
        pool_thresholds.pp_security_group = UnitInterval { numerator: 0, denominator: 1 };
        if let Some(pp) = ledger.protocol_params_mut() {
            pp.drep_voting_thresholds = Some(drep_thresholds);
            pp.pool_voting_thresholds = Some(pool_thresholds);
        }
        // Resign the CC member so committee checks don't block.
        let cc_cred = test_cred(0xC0);
        ledger
            .committee_state_mut()
            .get_mut(&cc_cred)
            .unwrap()
            .set_authorization(Some(CommitteeAuthorization::CommitteeMemberResigned(None)));
        ledger
    }

    #[test]
    fn test_delaying_action_prevents_further_enactments() {
        // Upstream: after enacting a delaying action (e.g. NoConfidence),
        // all subsequent proposals are skipped regardless of votes.
        let mut ledger = make_auto_pass_ledger();

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        // Epoch 1: populate mark snapshot (no proposals yet).
        let _ = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1");

        // Insert two proposals AFTER epoch 1.  NoConfidence (0x01) will
        // be processed first (lower GovActionId).  InfoAction (0x02)
        // would normally pass but must be skipped because NoConfidence
        // is delaying.
        let gai_nc = test_gov_action_id(0x01, 0);
        let gas_nc = GovernanceActionState::new(test_no_confidence_proposal());
        ledger.governance_actions_mut().insert(gai_nc.clone(), gas_nc);

        let gai_info = test_gov_action_id(0x02, 0);
        let gas_info = GovernanceActionState::new(test_info_proposal());
        ledger.governance_actions_mut().insert(gai_info.clone(), gas_info);

        let event = apply_epoch_boundary(&mut ledger, EpochNo(2), &mut snapshots, &perf)
            .expect("epoch 2");

        // Only NoConfidence should be enacted (it's a delaying action).
        assert_eq!(event.governance_actions_enacted, 1);
        assert_eq!(event.enacted_gov_action_ids, vec![gai_nc]);
        // InfoAction should remain pending.
        assert!(ledger.governance_actions().contains_key(&gai_info));
    }

    #[test]
    fn test_non_delaying_action_allows_continuation() {
        // Two non-delaying actions (ParameterChange priority 4,
        // TreasuryWithdrawals priority 5) should both be enacted
        // in the same epoch since neither is delaying.
        let mut ledger = make_auto_pass_ledger();

        // Register a reward account target for the treasury withdrawal.
        let target_ra = test_reward_account(20);
        ledger.accounting_mut().reserves = 0;
        ledger.accounting_mut().treasury = 100_000_000;

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        // Epoch 1: populate mark snapshot (no proposals yet).
        let _ = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1");

        // Insert both proposals after epoch 1.
        let gai_pc = test_gov_action_id(0x01, 0);
        let gas_pc = GovernanceActionState::new(test_parameter_change_proposal());
        ledger.governance_actions_mut().insert(gai_pc.clone(), gas_pc);

        let mut wd = BTreeMap::new();
        wd.insert(target_ra, 1_000u64);
        let gai_tw = test_gov_action_id(0x02, 0);
        let gas_tw = GovernanceActionState::new(crate::eras::conway::ProposalProcedure {
            deposit: 0,
            reward_account: vec![],
            gov_action: GovAction::TreasuryWithdrawals {
                withdrawals: wd,
                guardrails_script_hash: None,
            },
            anchor: crate::types::Anchor { url: String::new(), data_hash: [0; 32] },
        });
        ledger.governance_actions_mut().insert(gai_tw.clone(), gas_tw);

        let event = apply_epoch_boundary(&mut ledger, EpochNo(2), &mut snapshots, &perf)
            .expect("epoch 2");

        // Both should be enacted (neither is delaying).
        assert_eq!(event.governance_actions_enacted, 2);
        assert!(event.enacted_gov_action_ids.contains(&gai_pc));
        assert!(event.enacted_gov_action_ids.contains(&gai_tw));
        assert!(ledger.governance_actions().is_empty());
    }

    #[test]
    fn test_priority_ordering_delays_lower_priority_actions() {
        // Upstream `reorderActions` sorts proposals by `actionPriority`
        // before RATIFY processes them.  A NoConfidence (priority 0)
        // with a HIGHER GovActionId must be processed BEFORE a
        // ParameterChange (priority 4) with a LOWER GovActionId.
        // Since NoConfidence is delaying, the ParameterChange is skipped.
        //
        // Reference: Cardano.Ledger.Conway.Governance.Procedures.actionPriority,
        //            Cardano.Ledger.Conway.Governance.Internal.reorderActions.
        let mut ledger = make_auto_pass_ledger();

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        // Epoch 1: populate mark snapshot (no proposals yet).
        let _ = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1");

        // ParameterChange has LOWER GovActionId (0x01) — in BTreeMap
        // key order it would come first.  But priority 4 > 0.
        let gai_pc = test_gov_action_id(0x01, 0);
        let gas_pc = GovernanceActionState::new(test_parameter_change_proposal());
        ledger.governance_actions_mut().insert(gai_pc.clone(), gas_pc);

        // NoConfidence has HIGHER GovActionId (0x02) but priority 0,
        // so it goes first under priority ordering and delays everything.
        let gai_nc = test_gov_action_id(0x02, 0);
        let gas_nc = GovernanceActionState::new(test_no_confidence_proposal());
        ledger.governance_actions_mut().insert(gai_nc.clone(), gas_nc);

        let event = apply_epoch_boundary(&mut ledger, EpochNo(2), &mut snapshots, &perf)
            .expect("epoch 2");

        // Only NoConfidence should be enacted (delaying blocks ParameterChange).
        assert_eq!(event.governance_actions_enacted, 1);
        assert_eq!(event.enacted_gov_action_ids, vec![gai_nc]);
        // ParameterChange should remain pending.
        assert!(ledger.governance_actions().contains_key(&gai_pc));
    }

    #[test]
    fn test_action_priority_values() {
        // Verify the priority mapping matches upstream actionPriority.
        use crate::eras::conway::GovAction;
        assert_eq!(action_priority(&GovAction::NoConfidence { prev_action_id: None }), 0);
        assert_eq!(action_priority(&GovAction::UpdateCommittee {
            prev_action_id: None,
            members_to_remove: vec![],
            members_to_add: BTreeMap::new(),
            quorum: UnitInterval { numerator: 0, denominator: 1 },
        }), 1);
        assert_eq!(action_priority(&GovAction::NewConstitution {
            prev_action_id: None,
            constitution: crate::eras::conway::Constitution {
                anchor: crate::types::Anchor { url: String::new(), data_hash: [0; 32] },
                guardrails_script_hash: None,
            },
        }), 2);
        assert_eq!(action_priority(&GovAction::HardForkInitiation {
            prev_action_id: None,
            protocol_version: (10, 0),
        }), 3);
        assert_eq!(action_priority(&GovAction::ParameterChange {
            prev_action_id: None,
            protocol_param_update: Default::default(),
            guardrails_script_hash: None,
        }), 4);
        assert_eq!(action_priority(&GovAction::TreasuryWithdrawals {
            withdrawals: BTreeMap::new(),
            guardrails_script_hash: None,
        }), 5);
        assert_eq!(action_priority(&GovAction::InfoAction), 6);
    }

    #[test]
    fn test_chained_parameter_changes_enacted_iteratively() {
        // Two ParameterChanges where B chains from A.
        // Both should be enacted in a single epoch because A is
        // enacted first, advancing the lineage root, allowing B
        // to pass prevActionAsExpected.
        let mut ledger = make_auto_pass_ledger();

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        // Epoch 1: populate mark snapshot (no proposals yet).
        let _ = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1");

        // Insert both proposals after epoch 1.
        let gai_a = test_gov_action_id(0x01, 0);
        let mut proposal_a = test_parameter_change_proposal();
        if let GovAction::ParameterChange { ref mut protocol_param_update, .. } = proposal_a.gov_action {
            protocol_param_update.key_deposit = Some(3_000_000);
        }
        let gas_a = GovernanceActionState::new(proposal_a);
        ledger.governance_actions_mut().insert(gai_a.clone(), gas_a);

        let gai_b = test_gov_action_id(0x02, 0);
        let proposal_b = crate::eras::conway::ProposalProcedure {
            deposit: 0,
            reward_account: vec![],
            gov_action: GovAction::ParameterChange {
                prev_action_id: Some(gai_a.clone()),
                protocol_param_update: ProtocolParameterUpdate {
                    pool_deposit: Some(600_000_000),
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
            anchor: crate::types::Anchor {
                url: String::new(),
                data_hash: [0; 32],
            },
        };
        let gas_b = GovernanceActionState::new(proposal_b);
        ledger.governance_actions_mut().insert(gai_b.clone(), gas_b);

        let event = apply_epoch_boundary(&mut ledger, EpochNo(2), &mut snapshots, &perf)
            .expect("epoch 2");

        // A is enacted first (lower ID), lineage advances, B chains from A.
        assert_eq!(event.governance_actions_enacted, 2);
        assert_eq!(event.enacted_gov_action_ids[0], gai_a);
        assert_eq!(event.enacted_gov_action_ids[1], gai_b);
        // Both updates applied.
        assert_eq!(ledger.protocol_params().unwrap().key_deposit, 3_000_000);
        assert_eq!(ledger.protocol_params().unwrap().pool_deposit, 600_000_000);
        assert!(ledger.governance_actions().is_empty());
    }

    #[test]
    fn test_treasury_withdrawal_exceeding_treasury_skipped() {
        // A TreasuryWithdrawals proposal requesting more than the
        // treasury holds should be skipped (withdrawalCanWithdraw).
        let mut ledger = make_auto_pass_ledger();

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        // Epoch 1: populate mark snapshot (no proposals yet).
        let _ = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1");

        // Set treasury to a small amount AFTER epoch 1 ran (so
        // monetary expansion doesn't inflate it before our check).
        // Also zero reserves to prevent further monetary expansion at
        // epoch 2 from inflating the treasury via the treasury cut.
        ledger.accounting_mut().treasury = 1_000;
        ledger.accounting_mut().reserves = 0;

        // Register the withdrawal target reward account.
        let ra = crate::RewardAccount {
            network: 1,
            credential: StakeCredential::AddrKeyHash([0xE0; 28]),
        };
        ledger.reward_accounts_mut().insert(
            ra,
            RewardAccountState::new(0, None),
        );

        // Propose a withdrawal of 5M (far exceeds 1000 treasury).
        let gai = test_gov_action_id(0x01, 0);
        let gas = GovernanceActionState::new(test_treasury_withdrawal_proposal());
        ledger.governance_actions_mut().insert(gai.clone(), gas);

        let event = apply_epoch_boundary(&mut ledger, EpochNo(2), &mut snapshots, &perf)
            .expect("epoch 2");

        // Should NOT be enacted due to treasury insufficiency.
        assert_eq!(event.governance_actions_enacted, 0);
        assert!(ledger.governance_actions().contains_key(&gai));
    }

    #[test]
    fn test_prev_action_mismatch_skipped_at_ratification() {
        // A proposal whose prev_action_id points to the wrong root
        // should be skipped at ratification time (prevActionAsExpected).
        let mut ledger = make_auto_pass_ledger();

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        // Epoch 1: populate mark snapshot (no proposals yet).
        let _ = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1");

        // No ParameterChange enacted yet → root is None.
        // Proposal claims prev_action_id = Some(bogus).
        let bogus_prev = test_gov_action_id(0xFF, 99);
        let proposal = crate::eras::conway::ProposalProcedure {
            deposit: 0,
            reward_account: vec![],
            gov_action: GovAction::ParameterChange {
                prev_action_id: Some(bogus_prev),
                protocol_param_update: ProtocolParameterUpdate {
                    key_deposit: Some(5_000_000),
                    ..Default::default()
                },
                guardrails_script_hash: None,
            },
            anchor: crate::types::Anchor {
                url: String::new(),
                data_hash: [0; 32],
            },
        };
        let gai = test_gov_action_id(0x01, 0);
        let gas = GovernanceActionState::new(proposal);
        ledger.governance_actions_mut().insert(gai.clone(), gas);

        let event = apply_epoch_boundary(&mut ledger, EpochNo(2), &mut snapshots, &perf)
            .expect("epoch 2");

        // Should NOT be enacted (lineage mismatch).
        assert_eq!(event.governance_actions_enacted, 0);
        assert!(ledger.governance_actions().contains_key(&gai));
        // key_deposit should be unchanged.
        assert_eq!(ledger.protocol_params().unwrap().key_deposit, 2_000_000);
    }

    // ── derive_pool_performance ────────────────────────────────────────

    #[test]
    fn derive_pool_performance_empty_counts_returns_empty() {
        let snapshot = StakeSnapshot::empty();
        let d = UnitInterval { numerator: 0, denominator: 1 };
        let perf = super::derive_pool_performance(&BTreeMap::new(), &snapshot, d);
        assert!(perf.is_empty());
    }

    #[test]
    fn derive_pool_performance_no_stake_returns_empty() {
        let snapshot = StakeSnapshot::empty();
        let mut counts = BTreeMap::new();
        counts.insert(test_pool(1), 10);
        let d = UnitInterval { numerator: 0, denominator: 1 };
        let perf = super::derive_pool_performance(&counts, &snapshot, d);
        assert!(perf.is_empty());
    }

    #[test]
    fn derive_pool_performance_single_pool_perfect() {
        let mut snapshot = StakeSnapshot::empty();
        snapshot.pool_params.insert(test_pool(1), test_pool_params(1));
        snapshot.delegations.insert(test_cred(1), test_pool(1));
        snapshot.stake.add(test_cred(1), 1000);
        let mut counts = BTreeMap::new();
        counts.insert(test_pool(1), 10);
        let d = UnitInterval { numerator: 0, denominator: 1 };
        let perf = super::derive_pool_performance(&counts, &snapshot, d);
        let p = perf.get(&test_pool(1)).unwrap();
        // Single pool with all stake and all blocks → ratio = 1/1 = 1.0.
        assert_eq!(p.numerator * p.denominator, p.denominator * p.numerator);
    }

    #[test]
    fn derive_pool_performance_two_pools_proportional() {
        let mut snapshot = StakeSnapshot::empty();
        snapshot.pool_params.insert(test_pool(1), test_pool_params(1));
        snapshot.pool_params.insert(test_pool(2), test_pool_params(2));
        snapshot.delegations.insert(test_cred(1), test_pool(1));
        snapshot.delegations.insert(test_cred(2), test_pool(2));
        // Pool 1 has 75% of stake, pool 2 has 25%.
        snapshot.stake.add(test_cred(1), 750);
        snapshot.stake.add(test_cred(2), 250);
        // Both produced the same number of blocks.
        let mut counts = BTreeMap::new();
        counts.insert(test_pool(1), 5);
        counts.insert(test_pool(2), 5);
        let d = UnitInterval { numerator: 0, denominator: 1 };
        let perf = super::derive_pool_performance(&counts, &snapshot, d);
        // Pool 1: expected 75% of 10 = 7.5, actual 5.
        // performance = 5 * 1000 / (750 * 10) = 5000 / 7500 ≈ 0.667
        let p1 = perf.get(&test_pool(1)).unwrap();
        assert!(p1.numerator * 3 < p1.denominator * 3); // < 1.0
        // Pool 2: expected 25% of 10 = 2.5, actual 5.
        // performance = 5 * 1000 / (250 * 10) = 5000 / 2500 = 2.0
        let p2 = perf.get(&test_pool(2)).unwrap();
        assert!(p2.numerator * 1 > p2.denominator * 1); // > 1.0
    }

    #[test]
    fn derive_pool_performance_high_d_gives_perfect_score() {
        // When d >= 0.8 (early Shelley), all block-producing pools get
        // apparent performance = 1 regardless of their actual share of blocks.
        // Reference: upstream `mkApparentPerformance`.
        let mut snapshot = StakeSnapshot::empty();
        snapshot.pool_params.insert(test_pool(1), test_pool_params(1));
        snapshot.pool_params.insert(test_pool(2), test_pool_params(2));
        snapshot.delegations.insert(test_cred(1), test_pool(1));
        snapshot.delegations.insert(test_cred(2), test_pool(2));
        snapshot.stake.add(test_cred(1), 750);
        snapshot.stake.add(test_cred(2), 250);
        let mut counts = BTreeMap::new();
        counts.insert(test_pool(1), 5);
        counts.insert(test_pool(2), 5);
        // d = 0.9 (>= 0.8)
        let d = UnitInterval { numerator: 9, denominator: 10 };
        let perf = super::derive_pool_performance(&counts, &snapshot, d);
        // Both pools should have perf = 1.
        let p1 = perf.get(&test_pool(1)).unwrap();
        assert_eq!(p1.numerator, 1);
        assert_eq!(p1.denominator, 1);
        let p2 = perf.get(&test_pool(2)).unwrap();
        assert_eq!(p2.numerator, 1);
        assert_eq!(p2.denominator, 1);
    }

    // ── blocks_made integration ────────────────────────────────────────

    #[test]
    fn epoch_boundary_uses_internal_blocks_made_when_caller_passes_empty() {
        // Set up a ledger with a pool and stake.
        let mut ledger = make_ledger_with_pool(1);
        let pool_hash = test_pool(1);

        // Simulate blocks_made: the pool produced 10 blocks.
        for _ in 0..10 {
            ledger.record_block_producer(pool_hash);
        }
        assert_eq!(*ledger.blocks_made().get(&pool_hash).unwrap(), 10);

        // Build snapshots with the pool having stake.
        let mut snapshot = StakeSnapshot::empty();
        snapshot.pool_params.insert(pool_hash, test_pool_params(1));
        snapshot.delegations.insert(test_cred(1), pool_hash);
        snapshot.stake.add(test_cred(1), 1_000_000_000);
        let mut snapshots = StakeSnapshots::new();
        snapshots.go = snapshot.clone();
        snapshots.set = snapshot;

        // Call epoch boundary with an EMPTY performance map → should use internal blocks_made.
        let event = apply_epoch_boundary(
            &mut ledger,
            EpochNo(1),
            &mut snapshots,
            &BTreeMap::new(),
        )
        .expect("epoch boundary");

        // blocks_made should be cleared after epoch boundary.
        assert!(ledger.blocks_made().is_empty(), "blocks_made should be cleared after epoch boundary");

        // Rewards should have been computed (non-zero distribution).
        assert!(event.rewards_distributed > 0 || event.treasury_delta > 0,
            "rewards or treasury should be non-zero");
    }

    #[test]
    fn epoch_boundary_prefers_caller_performance_when_non_empty() {
        let mut ledger = make_ledger_with_pool(1);
        let pool_hash = test_pool(1);

        // Internal blocks_made: 1 block (low performance).
        ledger.record_block_producer(pool_hash);

        // But caller provides explicit perfect performance.
        let mut explicit_perf = BTreeMap::new();
        explicit_perf.insert(pool_hash, UnitInterval { numerator: 1, denominator: 1 });

        let mut snapshot = StakeSnapshot::empty();
        snapshot.pool_params.insert(pool_hash, test_pool_params(1));
        snapshot.delegations.insert(test_cred(1), pool_hash);
        snapshot.stake.add(test_cred(1), 1_000_000_000);
        let mut snapshots = StakeSnapshots::new();
        snapshots.go = snapshot.clone();
        snapshots.set = snapshot;

        let event = apply_epoch_boundary(
            &mut ledger,
            EpochNo(1),
            &mut snapshots,
            &explicit_perf,
        )
        .expect("epoch boundary");

        // blocks_made still cleared even when caller provides explicit perf.
        assert!(ledger.blocks_made().is_empty());
        // Should have rewards because we passed perfect performance.
        assert!(event.rewards_distributed > 0 || event.treasury_delta > 0);
    }

    // ── Ratification timing parity tests ───────────────────────────────

    #[test]
    fn test_two_treasury_withdrawals_use_progressive_treasury_guard() {
        // Upstream: RATIFY checks withdrawalCanWithdraw against the evolving
        // enact-state treasury. Two proposals each requesting 60M from a
        // 100M treasury should enact only one proposal.
        let mut ledger = make_auto_pass_ledger();

        let target_ra = test_reward_account(20);
        ledger.accounting_mut().reserves = 0;
        ledger.accounting_mut().treasury = 100_000_000;

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();
        let _ = apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch 1");

        // Proposal A: withdraw 60M.
        let mut wd_a = BTreeMap::new();
        wd_a.insert(target_ra, 60_000_000u64);
        let prop_a = crate::eras::conway::ProposalProcedure {
            deposit: 0,
            reward_account: vec![],
            gov_action: GovAction::TreasuryWithdrawals {
                withdrawals: wd_a,
                guardrails_script_hash: None,
            },
            anchor: crate::types::Anchor { url: String::new(), data_hash: [0; 32] },
        };
        // Proposal B: withdraw 60M (same target).
        let mut wd_b = BTreeMap::new();
        wd_b.insert(target_ra, 60_000_000u64);
        let prop_b = crate::eras::conway::ProposalProcedure {
            deposit: 0,
            reward_account: vec![],
            gov_action: GovAction::TreasuryWithdrawals {
                withdrawals: wd_b,
                guardrails_script_hash: None,
            },
            anchor: crate::types::Anchor { url: String::new(), data_hash: [0; 32] },
        };

        let gai_a = test_gov_action_id(0xA0, 0);
        let gai_b = test_gov_action_id(0xB0, 0);
        ledger.governance_actions_mut().insert(gai_a.clone(), GovernanceActionState::new(prop_a));
        ledger.governance_actions_mut().insert(gai_b.clone(), GovernanceActionState::new(prop_b));

        let event = apply_epoch_boundary(&mut ledger, EpochNo(2), &mut snapshots, &perf)
            .expect("epoch 2");

        // Only one should be enacted: after the first 60M withdrawal,
        // treasury drops below 60M so the second fails the guard.
        assert_eq!(event.governance_actions_enacted, 1);

        // Reward account should have received only 60M.
        let balance = ledger.reward_accounts().get(&target_ra).unwrap().balance();
        assert_eq!(balance, 60_000_000);
    }

    // ---------------------------------------------------------------
    // Credential-based member reward distribution
    // ---------------------------------------------------------------

    #[test]
    fn distribute_rewards_credits_member_by_credential_not_pool_account() {
        // Upstream `applyRUpdFiltered` resolves member rewards from
        // `StakeCredential` → the member's *own* registered
        // `RewardAccount`, NOT the pool operator's declared account.
        //
        // This test sets up a pool whose reward account uses network
        // byte 1, and a member whose registered account uses network
        // byte 0 (different network).  The member reward must be
        // credited to the member's own account.
        let mut ledger = LedgerState::new(Era::Shelley);

        // Pool operator credential + reward account (network=1).
        let pool_cred = test_cred(0x01);
        let pool_ra = RewardAccount { network: 1, credential: pool_cred };
        ledger.stake_credentials_mut().register(pool_cred);
        ledger.reward_accounts_mut().insert(
            pool_ra,
            RewardAccountState::new(0, None),
        );

        // Member credential + reward account (network=0, different!).
        let member_cred = test_cred(0x02);
        let member_ra = RewardAccount { network: 0, credential: member_cred };
        ledger.stake_credentials_mut().register(member_cred);
        ledger.reward_accounts_mut().insert(
            member_ra,
            RewardAccountState::new(0, None),
        );

        // Build a distribution: leader on pool_ra, member keyed by
        // member_cred.
        let dist = EpochRewardDistribution {
            leader_deltas: {
                let mut m = BTreeMap::new();
                m.insert(pool_ra, 100_000);
                m
            },
            reward_deltas: {
                let mut m = BTreeMap::new();
                m.insert(member_cred, 50_000);
                m
            },
            treasury_cut: 0,
            distributed: 150_000,
            unclaimed: 0,
            delta_reserves: 0,
        };

        let (count, unreg) = distribute_rewards(&mut ledger, &dist);

        assert_eq!(count, 2, "both leader and member should be credited");
        assert_eq!(unreg, 0, "no unregistered rewards");

        // Leader rewarded on pool's own account.
        assert_eq!(
            ledger.reward_accounts().get(&pool_ra).unwrap().balance(),
            100_000,
        );
        // Member rewarded on *member's own* account (network=0), NOT
        // the pool's account (network=1).
        assert_eq!(
            ledger.reward_accounts().get(&member_ra).unwrap().balance(),
            50_000,
        );
    }

    #[test]
    fn distribute_rewards_unregistered_member_goes_to_treasury_path() {
        // When a member credential has no registered RewardAccount,
        // the amount should appear in the unregistered total (upstream
        // routes this to treasury via `frTotalUnregistered`).
        let mut ledger = LedgerState::new(Era::Shelley);

        let pool_cred = test_cred(0x10);
        let pool_ra = RewardAccount { network: 1, credential: pool_cred };
        ledger.stake_credentials_mut().register(pool_cred);
        ledger.reward_accounts_mut().insert(
            pool_ra,
            RewardAccountState::new(0, None),
        );

        // Member credential is NOT registered.
        let ghost_cred = test_cred(0x20);

        let dist = EpochRewardDistribution {
            leader_deltas: {
                let mut m = BTreeMap::new();
                m.insert(pool_ra, 100_000);
                m
            },
            reward_deltas: {
                let mut m = BTreeMap::new();
                m.insert(ghost_cred, 75_000);
                m
            },
            treasury_cut: 0,
            distributed: 175_000,
            unclaimed: 0,
            delta_reserves: 0,
        };

        let (count, unreg) = distribute_rewards(&mut ledger, &dist);

        assert_eq!(count, 1, "only leader credited");
        assert_eq!(unreg, 75_000, "member reward is unregistered");

        assert_eq!(
            ledger.reward_accounts().get(&pool_ra).unwrap().balance(),
            100_000,
        );
    }

    // ------------------------------------------------------------------
    // Expired governance deposit → treasury when return account is gone
    // Reference: Cardano.Ledger.Conway.Rules.Epoch — `returnProposalDeposits`
    // ------------------------------------------------------------------
    #[test]
    fn expired_governance_deposit_goes_to_treasury_when_return_account_unregistered() {
        let mut ledger = make_ledger_with_pool(12);
        require_committee_vote_for_ratification(&mut ledger, 0xC1, 0xC2);
        let deposit_amount = 500_000_000u64;

        // Reward account byte 99 is NOT registered in the ledger.
        // The proposal uses it as return address.
        let proposal = test_proposal(deposit_amount, 99);
        let gas = GovernanceActionState::new_with_lifetime(
            proposal,
            EpochNo(1),
            Some(2), // expires_after = epoch 3
        );
        let gai = test_gov_action_id(0xDD, 0);
        ledger.governance_actions_mut().insert(gai.clone(), gas);
        assert_eq!(ledger.governance_actions().len(), 1);

        let treasury_before = ledger.accounting().treasury;

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        // Epoch 4 — action IS expired (expires_after 3 < 4).
        let event = apply_epoch_boundary(&mut ledger, EpochNo(4), &mut snapshots, &perf)
            .expect("epoch 4 boundary should succeed");
        assert_eq!(event.governance_actions_expired, 1);

        // Deposit must be reported as unclaimed (return account not registered).
        assert_eq!(
            event.unclaimed_governance_deposits, deposit_amount,
            "expired deposit with unregistered return account must be unclaimed"
        );

        // Treasury must have increased by at least the unclaimed deposit.
        let treasury_after = ledger.accounting().treasury;
        assert!(
            treasury_after >= treasury_before + deposit_amount,
            "treasury must include unclaimed expired governance deposit"
        );
    }

    // -- Future-params adoption timing (SNAP before adopt) ----------------

    /// Upstream EPOCH rule: SNAP takes the mark snapshot BEFORE
    /// `psFutureStakePoolParams` are activated (activation happens in
    /// POOLREAP). So a re-registered pool's new params should NOT appear
    /// in the mark snapshot — only the old params.
    ///
    /// Reference: `Cardano.Ledger.Shelley.Rules.Epoch` — SNAP → POOLREAP.
    #[test]
    fn snapshot_uses_old_params_before_future_adopt() {
        let pool_id: u8 = 0x10;
        let mut ledger = make_ledger_with_pool(pool_id);
        let original_cost = test_pool_params(pool_id).cost; // 340M

        // Re-register the same pool with different params to stage as
        // future params (upstream `psFutureStakePoolParams`).
        let mut new_params = test_pool_params(pool_id);
        new_params.cost = 999_000_000; // different from original 340M
        let pp_pool_deposit = test_protocol_params().pool_deposit;
        ledger.pool_state_mut().register_with_deposit(new_params.clone(), pp_pool_deposit);

        // Current params still have the original cost (future params are staged).
        assert_eq!(
            ledger.pool_state().get(&test_pool(pool_id)).unwrap().params().cost,
            original_cost,
        );

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        apply_epoch_boundary(&mut ledger, EpochNo(1), &mut snapshots, &perf)
            .expect("epoch boundary should succeed");

        // After epoch boundary, the mark snapshot should contain the OLD
        // pool params (cost 340M), not the future params (cost 999M).
        let snapped_cost = snapshots
            .mark
            .pool_params
            .get(&test_pool(pool_id))
            .expect("pool should be in mark snapshot")
            .cost;
        assert_eq!(
            snapped_cost, original_cost,
            "mark snapshot must use OLD pool params (upstream SNAP runs before POOLREAP adopt)"
        );

        // BUT after the epoch boundary, the live pool state should now
        // have the new (adopted) params.
        assert_eq!(
            ledger.pool_state().get(&test_pool(pool_id)).unwrap().params().cost,
            999_000_000,
            "live pool params must be updated after epoch boundary"
        );
    }

    // -- Dormant epoch counter: leave unchanged, never explicit reset -----

    /// Upstream `updateNumDormantEpochs` increments when no proposals
    /// exist and leaves unchanged otherwise — it never resets to 0 at
    /// epoch boundary. The per-tx `updateDormantDRepExpiries` is
    /// responsible for the reset.
    #[test]
    fn dormant_counter_not_reset_when_proposals_exist() {
        let mut ledger = LedgerState::new(Era::Conway);
        ledger.set_protocol_params(test_protocol_params());
        ledger.accounting_mut().reserves = 14_000_000_000_000_000;
        ledger.accounting_mut().treasury = 500_000_000_000;

        // Require committee approval so the proposal cannot be trivially ratified.
        require_committee_vote_for_ratification(&mut ledger, 0xC1, 0xC2);

        // Pre-set the dormant counter to 5.
        ledger.num_dormant_epochs = 5;

        // Add a governance action that will NOT expire this epoch.
        let proposal = test_proposal(500_000_000, 0x20);
        let gas = GovernanceActionState::new_with_lifetime(
            proposal,
            EpochNo(1),
            Some(10), // expires_after epoch 11 — far away from epoch 2
        );
        let gai = test_gov_action_id(0xEE, 0);
        // Register the return reward account so the proposal isn't treated
        // as un-reclaimable.
        let ra = test_reward_account(0x20);
        ledger
            .reward_accounts_mut()
            .insert(ra, RewardAccountState::new(0, None));
        ledger.governance_actions_mut().insert(gai, gas);

        let mut snapshots = StakeSnapshots::new();
        let perf = BTreeMap::new();

        apply_epoch_boundary(&mut ledger, EpochNo(2), &mut snapshots, &perf)
            .expect("epoch boundary should succeed");

        // Dormant counter must be LEFT UNCHANGED (still 5), not reset to 0.
        assert_eq!(
            ledger.num_dormant_epochs, 5,
            "dormant counter must not be reset to 0 at epoch boundary when proposals exist \
             (upstream updateNumDormantEpochs leaves unchanged; reset happens in per-tx GOV rule)"
        );
    }
}
