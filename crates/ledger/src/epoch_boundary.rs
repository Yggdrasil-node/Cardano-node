//! Epoch boundary processing for the Shelley-based ledger.
//!
//! At each epoch transition the ledger performs the NEWEPOCH / RUPD /
//! EPOCH sequence defined in the Shelley formal specification:
//!
//! 1. **Reward distribution** (RUPD reward update) — the reward pot is
//!    formed from monetary expansion (ρ) and delayed accumulated fees,
//!    the treasury cut (τ) is deducted, and the remainder is distributed
//!    to pools and delegators according to the **go** snapshot and
//!    previous-epoch block counts (`nesBprev`).
//! 2. **Stake snapshot rotation** (SNAP rule) — a fresh snapshot is
//!    computed from the post-reward UTxO and reward accounts, and the
//!    three-snapshot ring is rotated (`go ← set ← mark ← new`).
//! 3. **Pool retirement** — pools whose `retiring_epoch` ≤ the new
//!    epoch are removed and their deposits refunded.
//! 4. **Accounting update** — treasury receives its cut plus unclaimed
//!    deposits; unclaimed reward remainder returns to reserves, and
//!    reserves are reduced by monetary expansion.
//!
//! The orchestration entry point is [`apply_epoch_boundary`], which
//! operates on a [`LedgerState`] and returns an [`EpochBoundaryEvent`]
//! summarising the transition.
//!
//! Reference: `Cardano.Ledger.Shelley.Rules.NewEpoch`,
//! `Cardano.Ledger.Shelley.Rules.Epoch`.

use std::collections::{BTreeMap, BTreeSet};

use crate::eras::conway::GovActionId;
use crate::error::LedgerError;
use crate::rewards::{EpochRewardDistribution, RewardParams, compute_epoch_rewards};
use crate::stake::{
    StakeSnapshots, augment_pool_dist_with_proposal_deposits, compute_drep_stake_distribution,
    compute_proposal_deposits_per_credential, compute_stake_snapshot,
};
use crate::state::{EnactOutcome, LedgerState};
use crate::types::{EpochNo, PoolKeyHash, RewardAccount, UnitInterval};

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
    let one = UnitInterval {
        numerator: 1,
        denominator: 1,
    };

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
    let expected = one_minus_d_num * (asc.numerator as u128) * (slots_per_epoch as u128)
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

fn gcd_u64(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

fn gcd_u128(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

fn ceil_div_u128(a: u128, b: u128) -> u128 {
    if b == 0 { 0 } else { a.div_ceil(b) }
}

fn apparent_performance_ratio(
    blocks_produced: u64,
    total_active_stake: u64,
    pool_stake: u64,
    total_blocks: u64,
) -> Option<UnitInterval> {
    if blocks_produced == 0 || total_active_stake == 0 || pool_stake == 0 || total_blocks == 0 {
        return None;
    }

    // performance = blocks_produced * total_active_stake
    //             / (pool_stake * total_blocks)
    //
    // Reduce factors before multiplication. Preview/mainnet-scale values
    // can otherwise overflow u64 even when the rational itself is small.
    let mut n1 = blocks_produced;
    let mut n2 = total_active_stake;
    let mut d1 = pool_stake;
    let mut d2 = total_blocks;

    let g = gcd_u64(n1, d2);
    n1 /= g;
    d2 /= g;
    let g = gcd_u64(n2, d1);
    n2 /= g;
    d1 /= g;
    let g = gcd_u64(n1, d1);
    n1 /= g;
    d1 /= g;
    let g = gcd_u64(n2, d2);
    n2 /= g;
    d2 /= g;

    let mut numerator = (n1 as u128).saturating_mul(n2 as u128);
    let mut denominator = (d1 as u128).saturating_mul(d2 as u128);
    if denominator == 0 {
        return None;
    }

    let g = gcd_u128(numerator, denominator);
    numerator /= g;
    denominator /= g;

    let max = u64::MAX as u128;
    if numerator > max || denominator > max {
        let scale = ceil_div_u128(numerator, max).max(ceil_div_u128(denominator, max));
        numerator = (numerator / scale).max(1);
        denominator = (denominator / scale).max(1);
    }

    Some(UnitInterval {
        numerator: numerator as u64,
        denominator: denominator as u64,
    })
}

/// Derives per-pool performance ratios from block production counts and
/// the reward stake snapshot (`ssStakeGo`).
///
/// Upstream `startStep` passes `ssStakeGo` into `mkPoolRewardInfo`.
/// Performance for each block-producing pool is
/// `blocks_produced / (σ_pool * total_blocks)` where `total_blocks` is the
/// actual number of blocks produced in the epoch. When the snapshot has no
/// stake data, returns an empty map.
///
/// Reference: `Cardano.Ledger.Shelley.LedgerState` — `completeRupd`,
/// `mkApparentPerformance`.
///
/// When `d >= 0.8` (early Shelley era), upstream assigns apparent
/// performance of 1 to every block-producing pool regardless of
/// their actual share of blocks.
fn derive_pool_performance(
    blocks_made: &BTreeMap<PoolKeyHash, u64>,
    reward_snapshot: &crate::stake::StakeSnapshot,
    d: UnitInterval,
) -> BTreeMap<PoolKeyHash, UnitInterval> {
    // d >= 0.8  →  all block-producing pools get perf = 1.
    // Reference: `mkApparentPerformance` in Shelley.Rewards:
    //   | unboundRational d_ < 0.8 = beta / sigma
    //   | otherwise = 1
    if d.numerator * 10 >= d.denominator * 8 && d.denominator > 0 {
        return blocks_made
            .keys()
            .map(|pool_hash| {
                (
                    *pool_hash,
                    UnitInterval {
                        numerator: 1,
                        denominator: 1,
                    },
                )
            })
            .collect();
    }

    let stake_dist = reward_snapshot.pool_stake_distribution();
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
        if let Some(ratio) =
            apparent_performance_ratio(blocks_produced, total_stake, pool_stake, total_blocks)
        {
            performance.insert(*pool_hash, ratio);
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
///   per-pool performance from `ledger.previous_blocks_made()` and the `go`
///   snapshot's stake distribution (upstream `nesBprev` semantics).
///   A pool absent from the resulting map is treated as having
///   zero performance.
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
    //
    // Reward-formula inputs (rho, tau, a0, n_opt, d) MUST be read from the
    // *previous* protocol parameters, mirroring upstream
    // `Cardano.Ledger.Shelley.LedgerState.PulsingReward.startStep` which
    // pulses with `pr = es.prevPParamsEpochStateL`:
    //
    //   pr = es ^. prevPParamsEpochStateL
    //   deltaR1 = floor (min 1 eta * (pr ^. ppRhoL) * reserves)
    //   deltaT1 = floor ((pr ^. ppTauL) * rPot)
    //   maxPool = maxPool' (pr ^. ppA0L) (pr ^. ppNOptL) ...
    //
    // The verify17/19/20 chain-replay diff against upstream's
    // /tmp/upstream-epoch-state.jsonl pinned the n_opt mismatch at preview
    // B(10): upstream's prevPParams.stakePoolTargetNum = 150 (= the
    // pre-Vasil-PPUP value) while curPParams.stakePoolTargetNum = 500.
    // The PPUP that bumped k from 150 → 500 was effective at the start of
    // epoch 9; at B(10)'s startStep upstream still reads prevPP.n_opt = 150,
    // which uncaps σ' = min(σ, 1/n_opt) for the bootstrap pools (σ ≈
    // 0.0033 < 1/150 ≈ 0.0067) and produces a per-pool maxPool of
    // 74,314 ADA vs our 54,088 ADA — a +60,679 ADA total under-payment
    // surfacing as the +228-lovelace withdrawal surplus at slot 1.04M.
    //
    // Reading prevPP here is *only* parity-correct because the prefilter
    // fix above (`prefilter_unregistered`) already drops unregistered
    // rewards into reserves for PV<7, matching upstream's `collectLRs`
    // behaviour. Without that fix, the B(4) over-drain bug would mask
    // (and require unwinding) the n_opt fix.
    //
    // `min_pool_cost` and `drep_activity` are admission/governance
    // parameters, not reward-formula inputs, so they keep their active
    // (curPP) values.
    let prev_params_for_rewards = ledger
        .previous_protocol_params()
        .cloned()
        .ok_or(LedgerError::MissingProtocolParameters)?;
    let rho = prev_params_for_rewards.rho;
    let tau = prev_params_for_rewards.tau;
    let a0 = prev_params_for_rewards.a0;
    let n_opt = prev_params_for_rewards.n_opt;
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
    //    from the ledger's delayed `previous_blocks_made` (upstream
    //    `nesBprev`).
    //
    //    Upstream reward computation is pulsed across the epoch via
    //    `startStep`/`pulseStep`/`completeStep` and applied at the next
    //    NEWEPOCH.  This inline implementation preserves the observable
    //    boundary effect by using the same delayed inputs: `ssStakeGo`,
    //    `ssFee`, and `nesBprev`, all captured before SNAP rotates the
    //    current epoch state.
    //
    //    Reference: `Cardano.Ledger.Shelley.Rules.NewEpoch` — RUPD runs
    //    before EPOCH (which contains SNAP).
    //    Reference: `Cardano.Ledger.Shelley.LedgerState.PulsingReward`
    //    — `startStep`, `completeRupd`.
    // -----------------------------------------------------------------------
    let fee_pot = snapshots.previous_fee_pot;

    // Compute eta — monetary expansion efficiency factor.
    //
    // When d < 0.8 (post-Shelley: d = 0): eta = blocksMade / expectedBlocks,
    // capped at 1.  When d >= 0.8: eta = 1.
    //
    // expectedBlocks = (1 - d) × active_slot_coeff × slots_per_epoch
    //
    // Crucially, upstream `startStep` reads `pr ^. ppDL` (= `prevPParams.d`),
    // not `pp ^. ppDL` (= `curPParams.d`).  The TPraos→Praos transition (d=1
    // to d=0) is one such update: at the boundary entering the *first*
    // d=0 epoch, `prevPParams.d` is still 1 (overlay era) so eta=1 and the
    // monetary expansion fires; only one boundary later does
    // `prevPParams` advance to d=0 and the eta=blocks/expected formula
    // kick in.  Reading `curPParams.d` would prematurely zero `delta_R`
    // for the boundary that lands the d=0 update, costing ~9 T lovelace
    // of monetary expansion that upstream still credits.
    //
    // Reference: `Cardano.Ledger.Shelley.LedgerState.PulsingReward.startStep`.
    let d_param = ledger
        .previous_protocol_params()
        .and_then(|pp| pp.d)
        .unwrap_or(UnitInterval {
            numerator: 0,
            denominator: 1,
        });
    let eta = compute_eta(
        d_param,
        ledger.active_slot_coeff(),
        ledger.slots_per_epoch(),
        ledger.previous_blocks_made(),
    );

    // Whether to apply upstream's pre-Vasil "drop unregistered rewards before
    // they enter the pulser" filter. Mirrors `hardforkBabbageForgoRewardPrefilter`:
    //
    //   prefilter_active <==> pvMajor (pr ^. ppProtocolVersionL) < 7
    //
    // upstream `startStep` reads `pr = es.prevPParams` so we compare against
    // the **previous** protocol-version's major. For PV < 7 the unregistered
    // rewards stay in `R` and flow back to reserves via `deltaR2`, matching
    // upstream's `collectLRs`/`rewardOnePoolMember` semantics. From Vasil
    // (PV ≥ 7) onward the gate is bypassed and unregistered amounts route
    // through `frTotalUnregistered` to the treasury via `applyRUpdFiltered`.
    let prev_pv_major = ledger
        .previous_protocol_params()
        .and_then(|pp| pp.protocol_version)
        .map(|(major, _)| major)
        .unwrap_or(0);
    let prefilter_unregistered = prev_pv_major < 7;
    let registered_credentials: std::collections::BTreeSet<crate::types::StakeCredential> =
        if prefilter_unregistered {
            ledger
                .reward_accounts()
                .iter()
                .map(|(account, _)| account.credential)
                .collect()
        } else {
            std::collections::BTreeSet::new()
        };

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
        prefilter_unregistered,
        registered_credentials,
    };

    // Derive effective performance: caller-provided or from delayed blocks_made.
    //
    // Upstream `mkApparentPerformance` in `mkPoolRewardInfo` uses data from
    // `ssStakeGo` — the same snapshot used for reward distribution.
    // Pre-rotation in our code, `snapshots.go` corresponds to upstream's
    // `ssStakeGo` at the time `startStep` would read it.
    let effective_performance: BTreeMap<PoolKeyHash, UnitInterval> =
        if pool_performance.is_empty() && !ledger.previous_blocks_made().is_empty() {
            derive_pool_performance(ledger.previous_blocks_made(), &snapshots.go, d_param)
        } else {
            pool_performance.clone()
        };

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
    // Rotate the just-ended epoch's fees into `previous_fee_pot` for the next
    // reward update.  The current boundary has already consumed the old
    // delayed fee pot above.
    let _ = snapshots.rotate(new_mark);
    ledger.rotate_blocks_made_for_epoch_boundary();

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
    //
    //     Snapshot the current `protocol_params` into
    //     `previous_protocol_params` BEFORE the update applies so that
    //     the next boundary's reward calc reads the pre-update value
    //     (matching upstream's `esPrevPParams`).
    // -----------------------------------------------------------------------
    ledger.snapshot_previous_protocol_params();
    let pparam_updates_applied = ledger.apply_due_pending_pparam_updates(new_epoch);

    // -----------------------------------------------------------------------
    // 4. Accounting — update treasury and reserves.
    //
    //    Only `delta_reserves` (= reserves × ρ, the monetary expansion)
    //    is subtracted from reserves.  The fee pot comes from transaction
    //    fees, not from reserves.
    //
    //    NOTE: Conway treasury donations are flushed AFTER ratification
    //    (step 5b below), matching upstream ordering where
    //    `casTreasuryL <>~ utxosDonationL` runs after
    //    `applyEnactedWithdrawals` / `proposalsApplyEnactment`.
    //
    //    Reference: `Cardano.Ledger.Shelley.Rules.NewEpoch` — accounting
    //    update step.
    // -----------------------------------------------------------------------
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
    // 5. Ratification — tally votes for ALL surviving governance actions
    //    (including expired-but-not-yet-removed ones) and enact any that
    //    reach their acceptance thresholds.
    //
    //    Upstream: the DRep pulser runs `ratifyTransition` during the
    //    epoch on ALL proposals, including those that will expire at the
    //    epoch boundary.  An expired action that passes all ratification
    //    checks IS enacted (upstream `ratifyTransition`: enactment happens
    //    BEFORE the `gasExpiresAfter < reCurrentEpoch` guard).  Only
    //    non-enacted expired actions are added to `rsExpired` and cleaned
    //    up afterwards.  Therefore ratification MUST run before expiry
    //    pruning so that an action expiring in the same epoch it becomes
    //    ratifiable still gets enacted.
    //
    //    Reference: `Cardano.Ledger.Conway.Rules.Epoch` — epochTransition,
    //    `Cardano.Ledger.Conway.Rules.Ratify` — ratifyTransition.
    // -----------------------------------------------------------------------
    let ratify_result = ratify_and_enact(ledger, new_epoch, snapshots, drep_activity);
    let governance_actions_enacted = ratify_result.enacted_ids.len();

    // -----------------------------------------------------------------------
    // 5a. Governance action expiry — remove expired proposals that were
    //     NOT enacted and refund their deposits to return accounts.
    //     (Enacted proposals were already removed by `ratify_and_enact`.)
    // -----------------------------------------------------------------------
    let (expired_gov_action_ids, governance_deposit_refunds, expired_unclaimed_deposits) =
        remove_expired_governance_actions(ledger, new_epoch);

    // 5b. Remove descendant proposals whose prev_action_id chains through
    //     an expired parent.  Upstream `proposalsRemoveWithDescendants`
    //     transitively removes descendants of expired proposals.
    //     Reference: Cardano.Ledger.Conway.Governance.Proposals.
    let (descendant_refunds, descendant_unclaimed) = if !expired_gov_action_ids.is_empty() {
        remove_descendants_of(ledger, &expired_gov_action_ids)
    } else {
        (0, 0)
    };
    let governance_actions_expired = expired_gov_action_ids.len();

    // Credit unclaimed governance deposits to treasury — from
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
    // 5c. Flush accumulated Conway treasury donations into treasury.
    //
    //      Upstream ordering: donations are flushed to the main
    //      `ChainAccountState` treasury AFTER ratification results have
    //      been applied (`applyEnactedWithdrawals`, `proposalsApplyEnactment`,
    //      `returnProposalDeposits`):
    //
    //        chainAccountState3 = chainAccountState2
    //           & casTreasuryL <>~ (utxosDonationL <> fold unclaimed)
    //
    //      This ensures `withdrawal_can_withdraw` during ratification
    //      evaluates against a treasury that does NOT include the
    //      current epoch's accumulated donations — matching the upstream
    //      pulsing model where `ensTreasury` is captured before donations
    //      are flushed.
    //
    //      Reference: `Cardano.Ledger.Conway.Rules.Epoch` — epoch
    //      boundary: `casTreasuryL <>~ utxosDonationL`, then
    //      `utxosDonationL .~ zero`.
    // -----------------------------------------------------------------------
    let donations_transferred = ledger.flush_donations_to_treasury();

    // Debit the deposit pot for all returned/unclaimed proposal deposits.
    // Upstream reconciles via `utxosDepositedL .~ totalObligation certState govState`
    // which recomputes the full obligation from ground truth.  We track
    // incrementally, so we debit the total of all refunded + unclaimed
    // proposal deposits.
    {
        let total_proposal_deposit_reduction = governance_deposit_refunds
            .saturating_add(expired_unclaimed_deposits)
            .saturating_add(descendant_refunds)
            .saturating_add(descendant_unclaimed)
            .saturating_add(ratify_result.enacted_deposit_refunds)
            .saturating_add(ratify_result.removed_due_to_enactment_deposit_refunds)
            .saturating_add(ratify_result.unclaimed_deposits);
        ledger
            .deposit_pot_mut()
            .return_proposal_deposit(total_proposal_deposit_reduction);
    }

    // -----------------------------------------------------------------------
    // 5d. Dormant epoch counter — if no active (non-expired) governance
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
    // 5e. Committee state cleanup — prune hot-key authorization entries
    //     for cold credentials that are no longer active committee members.
    //
    //     Upstream `updateCommitteeState` in `Cardano.Ledger.Conway.Rules.Epoch`
    //     applies `Map.intersection creds members` where `members` is
    //     `committeeMembers` of the post-enactment committee.  In our
    //     combined model, non-members have `expires_at = None` after
    //     enactment (via `clear_membership`/`clear_all_membership`).
    //     Pruning removes these stale entries so that re-elected members
    //     must re-authorize their hot key.
    // -----------------------------------------------------------------------
    ledger.committee_state_mut().prune_non_members();

    // -----------------------------------------------------------------------
    // 6. DRep inactivity — compute the set of DReps that have exceeded
    //    the `drep_activity` window.  Inactive DReps remain registered
    //    but are excluded from ratification quorum calculations.
    //    Upstream: `Cardano.Ledger.Conway.Rules.Epoch` — drepExpiry.
    // -----------------------------------------------------------------------
    let dreps_expired = {
        ledger
            .drep_state()
            .inactive_dreps(new_epoch, drep_activity)
            .len()
    };

    Ok(EpochBoundaryEvent {
        new_epoch,
        pparam_updates_applied,
        pools_retired,
        retired_pool_keys,
        pool_deposit_refunds,
        unclaimed_pool_deposits,
        rewards_distributed: reward_dist.distributed,
        treasury_delta: reward_dist
            .treasury_cut
            .saturating_add(unregistered_rewards),
        unclaimed_rewards: reward_dist.unclaimed,
        delta_reserves: reward_dist.delta_reserves,
        accounts_rewarded,
        governance_actions_expired,
        governance_deposit_refunds: governance_deposit_refunds.saturating_add(descendant_refunds),
        expired_gov_action_ids,
        dreps_expired,
        governance_actions_enacted,
        enacted_gov_action_ids: ratify_result.enacted_ids,
        enact_outcomes: ratify_result.outcomes,
        enacted_deposit_refunds: ratify_result.enacted_deposit_refunds,
        removed_due_to_enactment: ratify_result.removed_due_to_enactment,
        removed_due_to_enactment_deposit_refunds: ratify_result
            .removed_due_to_enactment_deposit_refunds,
        unclaimed_governance_deposits: ratify_result
            .unclaimed_deposits
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
fn distribute_rewards(ledger: &mut LedgerState, dist: &EpochRewardDistribution) -> (usize, u64) {
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

    // 2. All-or-nothing: both pots must be sufficient AGAINST the
    //    POST-pot-to-pot-delta values, or neither pays. Matching upstream
    //    `Cardano.Ledger.Shelley.Rules.Mir`:
    //
    //      availableReserves = reserves `addDeltaCoin` deltaReserves
    //      availableTreasury = treasury `addDeltaCoin` deltaTreasury
    //      if totR <= availableReserves && totT <= availableTreasury
    //
    //    The pot-to-pot delta is applied BEFORE the sufficiency check, so a
    //    SendToOppositePot cert can make rewards payable that wouldn't be
    //    against the pre-delta values.
    let reserves = ledger.accounting().reserves;
    let treasury = ledger.accounting().treasury;
    let available_reserves = (reserves as i128 + delta_reserves as i128).max(0) as u64;
    let available_treasury = (treasury as i128 + delta_treasury as i128).max(0) as u64;

    let reserves_ok = total_reserves <= 0 || available_reserves >= total_reserves as u64;
    let treasury_ok = total_treasury <= 0 || available_treasury >= total_treasury as u64;
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

    // 4. Update pots: ONLY when sufficiency passes do we apply
    //    (delta_reserves, -totR) and (delta_treasury, -totT). When the
    //    sufficiency check fails, upstream's `Mir` rule returns the
    //    UNCHANGED `chainAccountState` (= no pot-to-pot delta is applied
    //    either) — only the IR state itself is cleared, which we already
    //    do via the `std::mem::take` above.
    //
    //    Reference: `Cardano.Ledger.Shelley.Rules.Mir.mirTransition` —
    //      else do
    //        tellEvent $ NoMirTransfer ...
    //        pure $ EpochState chainAccountState (ls & ... .~ emptyInstantaneousRewards) ...
    if can_pay {
        let acct = ledger.accounting_mut();
        // Order matches upstream `Mir`:
        //   casReserves = availableReserves <-> totR
        //              = (reserves + delta_reserves) - totR
        // Apply pot-to-pot delta FIRST so the saturating debit doesn't
        // underflow when delta brings the pot above the debit threshold.
        apply_signed_delta(&mut acct.reserves, delta_reserves);
        apply_signed_delta(&mut acct.treasury, delta_treasury);
        apply_signed_delta(&mut acct.reserves, -total_reserves);
        apply_signed_delta(&mut acct.treasury, -total_treasury);
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
        .filter(|(_, state)| state.expires_after().is_some_and(|exp| exp.0 < epoch.0))
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
fn remove_descendants_of(ledger: &mut LedgerState, removed_ids: &[GovActionId]) -> (u64, u64) {
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
            if let Some(reward_account) =
                RewardAccount::from_bytes(&state.proposal().reward_account)
            {
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
fn withdrawal_can_withdraw(action: &crate::eras::conway::GovAction, treasury: u64) -> bool {
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

    // Early exit when protocol params are absent.
    if ledger.protocol_params().is_none() {
        return RatifyAndEnactResult::default();
    }

    // Compute per-credential governance proposal deposits for use in
    // DRep and SPO voting weight calculations.
    //
    // Upstream `proposalsDeposits` aggregates proposal deposits by the
    // staking credential of each proposal's return address, and
    // `computeDRepDistr` adds these to both DRep and pool distributions.
    //
    // Reference: Cardano.Ledger.Conway.Governance.DRepPulser.computeDRepDistr.
    let proposal_deposits = compute_proposal_deposits_per_credential(ledger.governance_actions());

    // Compute DRep delegated stake distribution from the mark snapshot,
    // including proposal deposits in each credential's voting weight.
    let drep_delegated_stake = compute_drep_stake_distribution(
        &snapshots.mark,
        ledger.stake_credentials(),
        &proposal_deposits,
    );

    // Compute SPO pool stake distribution from the mark snapshot,
    // then augment with per-credential proposal deposits for pool-delegated
    // credentials (upstream adds only deposits to pool distribution since
    // regular stake is already in the SNAP snapshot).
    let mut pool_stake_dist = snapshots.mark.pool_stake_distribution();
    augment_pool_dist_with_proposal_deposits(
        &mut pool_stake_dist,
        ledger.stake_credentials(),
        &proposal_deposits,
    );

    // Collect proposal IDs sorted by governance action priority, then
    // by GovActionId within the same priority.  This matches upstream
    // `reorderActions` which sorts by `actionPriority` before RATIFY
    // processes the `RatifySignal`.
    //
    // Reference: Cardano.Ledger.Conway.Governance.Internal.reorderActions,
    //            Cardano.Ledger.Conway.Governance.Procedures.actionPriority.
    let mut sorted_ids: Vec<GovActionId> = ledger.governance_actions().keys().cloned().collect();
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

    // Upstream `ensTreasury` is decremented by the FULL proposed withdrawal
    // amount (including amounts to unregistered accounts) after each enacted
    // `TreasuryWithdrawals`.  The actual treasury in `LedgerState` only
    // debits registered accounts.  To match the upstream check semantics we
    // track a separate withdrawal budget that is reduced by the full
    // proposed total regardless of registration status.
    //
    // Reference: `Cardano.Ledger.Conway.Rules.Enact` —
    //   `ensTreasury st <-> wdrlsAmount` where `wdrlsAmount = fold wdrls`.
    let mut withdrawal_budget: u64 = ledger.accounting().treasury;

    for id in &sorted_ids {
        // Look up the action (it may have been removed by an earlier
        // enactment — shouldn't happen, but be defensive).
        let action_state = match ledger.governance_actions().get(id) {
            Some(s) => s,
            None => continue,
        };

        let gov_action = &action_state.proposal().gov_action;

        // Pre-extract values needed after the immutable borrow ends.
        let is_delaying = delaying_action(gov_action);
        let is_expired = action_state
            .expires_after()
            .is_some_and(|ea| ea.0 < current_epoch.0);

        // --- Upstream ratifyTransition guard checks (order matters) ---
        //
        // The upstream multi-guard has three branches:
        //   1. All guards pass → ENACT.  Set `rsDelayed || delayingAction`.
        //   2. `gasExpiresAfter < reCurrentEpoch` → add to `rsExpired`.
        //      `rsDelayed` is **unchanged** (expired actions do NOT block
        //      subsequent enactments).
        //   3. Otherwise (not enacted, not expired) → set `rsDelayed ||
        //      delayingAction`.  This means a non-enacted, non-expired
        //      NoConfidence / HardFork / UpdateCommittee / NewConstitution
        //      STILL prevents subsequent actions from being enacted.
        //
        // Reference: `Cardano.Ledger.Conway.Rules.Ratify` — ratifyTransition.
        let passed_all_checks = 'guards: {
            // 1. prevActionAsExpected — checked against CURRENT enact state
            //    (lineage updates mid-loop are intended).
            if !prev_action_as_expected(action_state, ledger.enact_state()) {
                break 'guards false;
            }

            // 2. validCommitteeTerm — checked against CURRENT protocol params
            //    from the evolving enact state.
            if !valid_committee_term(
                gov_action,
                ledger
                    .protocol_params()
                    .and_then(|pp| pp.committee_term_limit),
                current_epoch,
            ) {
                break 'guards false;
            }

            // 3. Delay flag — once a delaying action is enacted (or a
            //    non-enacted non-expired delaying action is encountered),
            //    stop enacting.
            if delayed {
                break 'guards false;
            }

            // 4. withdrawalCanWithdraw — checked against the withdrawal budget
            //    that tracks the full proposed amount (including unregistered
            //    accounts), matching upstream `ensTreasury` semantics.
            //
            //    Reference: `Cardano.Ledger.Conway.Rules.Ratify.withdrawalCanWithdraw`.
            if !withdrawal_can_withdraw(gov_action, withdrawal_budget) {
                break 'guards false;
            }

            // 5. acceptedByEveryone — committee + DRep + SPO thresholds.
            //    Read committee quorum from CURRENT enact state (may have
            //    changed after an earlier UpdateCommittee enactment).
            //
            //    Upstream `ratifyTransition` recursively passes the updated
            //    `RatifyState` (including `ensCurPParams` inside the
            //    `EnactState`) so that after a `ParameterChange` enactment,
            //    subsequent proposals see updated voting thresholds and
            //    `min_committee_size`.  We re-read from `protocol_params()`
            //    each iteration to match.
            //
            //    Reference: `votingDRepThreshold`, `votingStakePoolThreshold`,
            //    `committeeAccepted` — all read from `rs ^. rsEnactStateL . ensCurPParamsL`.
            let committee_quorum = ledger.enact_state().committee_quorum;
            let has_committee = ledger.enact_state().has_committee;

            // Re-read thresholds from the (possibly updated) protocol params.
            let pp = ledger.protocol_params().expect("checked at entry");
            let pool_thresholds = pp.pool_voting_thresholds.clone().unwrap_or_default();
            let drep_thresholds = pp.drep_voting_thresholds.clone().unwrap_or_default();
            let min_committee_size = pp.min_committee_size.unwrap_or(0);
            let is_bootstrap_phase = matches!(pp.protocol_version, Some((9, _)));

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
                min_committee_size,
                is_bootstrap_phase,
                has_committee,
                ledger.pool_state(),
                ledger.stake_credentials(),
            ) {
                break 'guards false;
            }

            true
        };

        if !passed_all_checks {
            // Upstream `otherwise` branch: non-enacted, non-expired
            // delaying actions set the delay flag.  Expired actions
            // do NOT change the flag (upstream expired branch passes
            // `rsDelayed` unchanged).
            if !is_expired && is_delaying {
                delayed = true;
            }
            continue;
        }

        // --- All checks passed: enact ---
        let removed = ledger.governance_actions_mut().remove(id);
        if let Some(state) = removed {
            enacted_purposes.insert(crate::state::conway_gov_action_purpose(
                &state.proposal().gov_action,
            ));
            deposit_targets.push((
                state.proposal().reward_account.clone(),
                state.proposal().deposit,
            ));
            let outcome = ledger.enact_action(id.clone(), &state.proposal().gov_action);

            // ----- HARDFORK rule: one-time state fixups -----
            //
            // Upstream `Cardano.Ledger.Conway.Rules.HardFork`:
            //   pvMajor newPv == natVersion @10 →
            //     updateDRepDelegations (removes dangling DRep delegations
            //     from accounts that pointed to non-existent DReps created
            //     during the bootstrap phase).
            //   pvMajor newPv == natVersion @11 →
            //     populateVRFKeyHashes (initializes VRF counting map;
            //     not needed here — our VRF uniqueness uses linear scan).
            if let crate::eras::conway::GovAction::HardForkInitiation {
                protocol_version: (major, _),
                ..
            } = state.proposal().gov_action
            {
                if major == 10 {
                    ledger.cleanup_dangling_drep_delegations();
                }
            }

            outcomes.push(outcome);
            enacted_ids.push(id.clone());

            // Decrement the withdrawal budget by the FULL proposed amount
            // (upstream `ensTreasury -= fold wdrls` in ENACT rule) so that
            // subsequent `withdrawalCanWithdraw` checks see the reduced
            // budget regardless of how much was actually credited to
            // registered accounts.
            if let crate::eras::conway::GovAction::TreasuryWithdrawals { withdrawals, .. } =
                &state.proposal().gov_action
            {
                let full_proposed: u64 = withdrawals.values().sum();
                withdrawal_budget = withdrawal_budget.saturating_sub(full_proposed);
            }

            // Set delay flag if this is a delaying action type.
            if is_delaying {
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
    let removed_due_to_enactment = remove_lineage_conflicting_proposals(ledger, &enacted_purposes);

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
            .filter(|(_, state)| conway_gov_action_purpose(&state.proposal().gov_action) == purpose)
            .map(|(id, state)| {
                let prev =
                    gov_action_prev_id(&state.proposal().gov_action).and_then(|opt| opt.clone());
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
        .filter(|(_, pool)| pool.retiring_epoch().is_some_and(|e| e <= epoch))
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
    ledger
        .stake_credentials_mut()
        .clear_pool_delegations(&retired_keys);

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
mod tests;
