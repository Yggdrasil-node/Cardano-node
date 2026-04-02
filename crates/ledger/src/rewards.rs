//! Epoch reward calculation implementing the Shelley reward formula.
//!
//! The Shelley formal specification (Section 10) defines the reward
//! distribution at each epoch boundary.  At a high level:
//!
//! 1. A per-epoch reward pot is formed from monetary expansion of the
//!    reserves plus accumulated transaction fees.
//! 2. A portion of the reward pot goes to the treasury (controlled by τ).
//! 3. The remainder is distributed to pools proportionally to their
//!    stake, modulated by pledge influence (a₀) and pool saturation.
//! 4. Within each pool, the operator takes the declared cost plus a margin
//!    of the remaining profit.  Members divide the rest proportionally
//!    to their individual stake.
//!
//! Reference: `Cardano.Ledger.Shelley.Rewards` — `reward`, `maxPool`,
//! `memberRew`, `leaderRew`.

use std::collections::BTreeMap;

use crate::stake::{PoolStakeDistribution, StakeSnapshot};
use crate::types::{PoolKeyHash, RewardAccount, StakeCredential, UnitInterval};

// ---------------------------------------------------------------------------
// Rational arithmetic helpers (u128-based)
// ---------------------------------------------------------------------------

/// Multiplies a coin value by a `UnitInterval` (rational), rounding down.
fn mul_rational(coin: u64, ratio: UnitInterval) -> u64 {
    if ratio.denominator == 0 {
        return 0;
    }
    let num = coin as u128 * ratio.numerator as u128;
    (num / ratio.denominator as u128) as u64
}

// ---------------------------------------------------------------------------
// Reward parameters
// ---------------------------------------------------------------------------

/// Parameters controlling the epoch reward distribution.
///
/// These are drawn from `ProtocolParameters` at the epoch boundary.
///
/// Reference: `Globals` in `Cardano.Ledger.Shelley.Rewards`.
#[derive(Clone, Debug)]
pub struct RewardParams {
    /// Monetary expansion rate (ρ).
    pub rho: UnitInterval,
    /// Treasury growth rate (τ).
    pub tau: UnitInterval,
    /// Pool pledge influence (a₀).
    pub a0: UnitInterval,
    /// Desired number of pools (k / n_opt).
    pub n_opt: u64,
    /// Minimum pool cost (lovelace per epoch).
    pub min_pool_cost: u64,
    /// Total reserves (lovelace) at the beginning of this epoch.
    pub reserves: u64,
    /// Transaction fees accumulated during the epoch.
    pub fee_pot: u64,
    /// Maximum lovelace supply (genesis constant, e.g. 45B ADA on mainnet).
    ///
    /// Used to compute `circulation = max_lovelace_supply - reserves`,
    /// which is the `totalStake` denominator in the upstream `maxPool`
    /// formula.  When zero, falls back to `total_active_stake` from the
    /// pool stake distribution.
    ///
    /// Reference: `startStep` in
    /// `Cardano.Ledger.Shelley.LedgerState.PulsingReward` —
    /// `totalStake = circulation es maxSupply`.
    pub max_lovelace_supply: u64,
    /// Monetary expansion efficiency factor (η).
    ///
    /// Upstream: `eta = min(1, blocksMade / expectedBlocks)` when `d < 0.8`,
    /// otherwise `eta = 1`.  Scales the monetary expansion (`ΔR = η × ρ ×
    /// reserves`) to penalize epochs with fewer blocks than expected.
    ///
    /// A value of `(1, 1)` (the default) means no adjustment.
    ///
    /// Reference: `startStep` in
    /// `Cardano.Ledger.Shelley.LedgerState.PulsingReward`.
    pub eta: UnitInterval,
}

// ---------------------------------------------------------------------------
// Epoch rewards pot
// ---------------------------------------------------------------------------

/// The reward pot available for distribution at an epoch boundary.
///
/// `rewards_pot = monetary_expansion + fees - treasury_cut`
///
/// Reference: `createRUpd` in `Cardano.Ledger.Shelley.Rewards`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EpochRewardPot {
    /// Total rewards available for pool/member distribution.
    pub rewards_pot: u64,
    /// Amount allocated to the treasury.
    pub treasury_cut: u64,
    /// Monetary expansion drawn from reserves (ΔR).
    pub delta_reserves: u64,
}

/// Computes the epoch reward pot from reserves, fees, and protocol params.
///
/// 1. `delta_reserves = ⌊min(1, η) × ρ × reserves⌋`
/// 2. `total_reward = delta_reserves + fee_pot`
/// 3. `treasury_cut = total_reward × τ`
/// 4. `rewards_pot = total_reward - treasury_cut`
///
/// Reference: `startStep` in
/// `Cardano.Ledger.Shelley.LedgerState.PulsingReward` —
/// `deltaR1 = rationalToCoinViaFloor (min 1 eta * rho * reserves)`.
pub fn compute_epoch_reward_pot(params: &RewardParams) -> EpochRewardPot {
    // η clamped to 1: min(1, eta).
    let eta_clamped = if params.eta.denominator == 0 {
        UnitInterval { numerator: 1, denominator: 1 }
    } else if params.eta.numerator >= params.eta.denominator {
        UnitInterval { numerator: 1, denominator: 1 }
    } else {
        params.eta
    };
    // delta_reserves = ⌊min(1, η) × ρ × reserves⌋
    let rho_reserves = mul_rational(params.reserves, params.rho);
    let delta_reserves = mul_rational(rho_reserves, eta_clamped);
    let total_reward = delta_reserves.saturating_add(params.fee_pot);
    let treasury_cut = mul_rational(total_reward, params.tau);
    let rewards_pot = total_reward.saturating_sub(treasury_cut);

    EpochRewardPot {
        rewards_pot,
        treasury_cut,
        delta_reserves,
    }
}

// ---------------------------------------------------------------------------
// Pool optimal reward (maxPool)
// ---------------------------------------------------------------------------

/// Computes the optimal reward for a fully-performing pool.
///
/// This is the `maxPool` function from the Shelley formal specification:
///
/// ```text
/// maxPool(R, n_opt, a0, σ, s) =
///   R / (1 + a0) × (σ' + s' × a0 × (σ' - s' × (z - σ') / z) / z)
/// ```
///
/// where:
/// - R = total rewards pot
/// - σ = pool relative stake (= pool_stake / total_stake)
/// - s = pool pledge relative stake (= pledge / total_stake)
/// - z = 1 / n_opt (saturation threshold)
/// - σ' = min(σ, z)
/// - s' = min(s, z)
///
/// All arithmetic uses u128 to avoid overflow with mainnet-scale values.
///
/// Reference: `maxPool` in `Cardano.Ledger.Shelley.Rewards`.
pub fn max_pool_reward(
    rewards_pot: u64,
    n_opt: u64,
    a0: UnitInterval,
    pool_stake: u64,
    pledge: u64,
    total_stake: u64,
) -> u64 {
    if total_stake == 0 || n_opt == 0 || rewards_pot == 0 {
        return 0;
    }

    // We use a fixed-point representation scaled by SCALE to maintain precision.
    // All ratios are represented as numerator/SCALE.
    const SCALE: u128 = 1_000_000_000_000; // 10^12

    let total = total_stake as u128;

    // z = 1 / n_opt (saturation point)
    let z = SCALE / (n_opt as u128);

    // σ = pool_stake / total_stake
    let sigma = (pool_stake as u128) * SCALE / total;
    // s = pledge / total_stake
    let s = (pledge as u128) * SCALE / total;

    // σ' = min(σ, z), s' = min(s, z)
    let sigma_prime = sigma.min(z);
    let s_prime = s.min(z);

    // a0 as scaled value: a0_scaled = a0.numerator * SCALE / a0.denominator
    let a0_scaled = if a0.denominator == 0 {
        0u128
    } else {
        (a0.numerator as u128) * SCALE / (a0.denominator as u128)
    };

    // 1 + a0 (scaled)
    let one_plus_a0 = SCALE + a0_scaled;
    if one_plus_a0 == 0 {
        return 0;
    }

    // R / (1 + a0) — keep as u128 for further multiplication
    let r_div_a0 = (rewards_pot as u128) * SCALE / one_plus_a0;

    // Inner term: σ' + s' × a0 × ((σ' - s' × (z - σ') / z) / z)
    //
    // We compute piece-by-piece:
    //   term1 = (z - σ') / z — represents how far the pool is from saturation
    //   term2 = s' × term1 / z — pledge-weighted distance
    //   term3 = σ' - term2 — effective relative stake adjusted for pledge
    //   term4 = s' × a0 × term3 / z — pledge influence contribution
    //   inner = σ' + term4

    // (z - σ') — this is always ≥ 0 because σ' = min(σ, z)
    let z_minus_sigma = z.saturating_sub(sigma_prime);

    // s' × (z - σ') / z
    let term2 = s_prime * z_minus_sigma / z;

    // σ' - term2 (can be negative conceptually but in Shelley spec σ' ≥ term2)
    let term3 = sigma_prime.saturating_sub(term2);

    // s' × a0 × term3 / z
    let term4 = s_prime * a0_scaled / SCALE * term3 / z;

    // inner = σ' + term4
    let inner = sigma_prime + term4;

    // result = R / (1 + a0) × inner / SCALE
    let result = r_div_a0 * inner / SCALE;

    result as u64
}

// ---------------------------------------------------------------------------
// Safe u128 rational arithmetic
// ---------------------------------------------------------------------------

/// Safely computes `floor(a * b / c)` using u128, with overflow fallback.
///
/// When `a * b` would overflow u128, uses the identity
/// `a*b/c = (a/c)*b + (a%c)*b/c` to split the computation.
fn floor_mul_div(a: u128, b: u128, c: u128) -> u128 {
    if c == 0 {
        return 0;
    }
    match a.checked_mul(b) {
        Some(product) => product / c,
        None => {
            let (q, r) = (a / c, a % c);
            q.saturating_mul(b).saturating_add(match r.checked_mul(b) {
                Some(rb) => rb / c,
                None => {
                    // Both a*b and r*b overflow — split further on b.
                    let (q2, r2) = (b / c, b % c);
                    q2.saturating_mul(r)
                        .saturating_add(r2.checked_mul(r).map_or(0, |v| v / c))
                }
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Per-pool reward distribution
// ---------------------------------------------------------------------------

/// Reward distribution result for a single pool.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PoolRewardBreakdown {
    /// Total apparent pool reward (before cost/margin).
    pub apparent_performance_reward: u64,
    /// Operator (leader) reward after cost + margin.
    pub leader_reward: u64,
    /// Per-member rewards: (stake_credential, reward_lovelace).
    pub member_rewards: BTreeMap<StakeCredential, u64>,
}

/// Computes the reward breakdown for a single pool.
///
/// Implements the upstream `mkPoolRewardInfo` + `rewardOnePoolMember`
/// flow from `Cardano.Ledger.Shelley.Rewards`:
///
/// 1. `maxP = maxPool(R, n_opt, a0, σ, pledge/totalStake)` when pledge
///    satisfied, else 0.
/// 2. `poolR = ⌊apparentPerformance × maxP⌋`
/// 3. **Leader reward** (`calcStakePoolOperatorReward`):
///    - When `poolR ≤ cost`: leader gets `poolR` (the full pool reward).
///    - Otherwise: `cost + ⌊(poolR − cost) × (m + (1−m) × s/σ)⌋`
///      where `s/σ = ownerDelegatedStake / poolStake`.
/// 4. **Member reward** (`calcStakePoolMemberReward`):
///    - When `poolR ≤ cost`: members get nothing.
///    - Otherwise: `⌊(poolR − cost) × (1−m) × t/σ⌋`
///      where `t/σ = memberStake / poolStake`.
///    - Pool owners (`pool_owners`) are excluded from member rewards;
///      their stake is folded into the leader's `s` term.
///
/// Reference: `calcStakePoolOperatorReward`, `calcStakePoolMemberReward`,
/// `mkPoolRewardInfo`, `rewardOnePoolMember` in
/// `Cardano.Ledger.Shelley.Rewards`.
pub fn compute_pool_reward(
    rewards_pot: u64,
    params: &RewardParams,
    pool_hash: &PoolKeyHash,
    snapshot: &StakeSnapshot,
    pool_dist: &PoolStakeDistribution,
    performance: UnitInterval,
) -> PoolRewardBreakdown {
    let pool_params = match snapshot.pool_params.get(pool_hash) {
        Some(pp) => pp,
        None => {
            return PoolRewardBreakdown {
                apparent_performance_reward: 0,
                leader_reward: 0,
                member_rewards: BTreeMap::new(),
            };
        }
    };

    let pool_stake = pool_dist.pool_stake(pool_hash);

    // The upstream `maxPool` formula uses `totalStake = circulation =
    // maxLovelaceSupply - reserves` as the denominator for σ and s, NOT
    // the total active stake.  Fall back to active stake when
    // `max_lovelace_supply` is not set (zero).
    //
    // Reference: `startStep` in
    // `Cardano.Ledger.Shelley.LedgerState.PulsingReward` —
    // `totalStake = circulation es maxSupply`.
    let total_stake_for_sigma = if params.max_lovelace_supply > 0 {
        params.max_lovelace_supply.saturating_sub(params.reserves)
    } else {
        pool_dist.total_active_stake()
    };

    // Upstream pledge satisfaction check: compute the aggregate stake held
    // by the pool's registered owners that is delegated to this pool.
    // If owner-delegated stake < declared pledge, the pool forfeits all
    // rewards for this epoch (upstream `mkPoolRewardInfo`: `if pledge <=
    // selfDelegatedOwnersStake then maxPool' ... else mempty`).
    // Reference: `Cardano.Ledger.Shelley.Rewards.mkPoolRewardInfo`.
    let owner_delegated_stake: u64 = pool_params
        .pool_owners
        .iter()
        .map(|owner_hash| {
            let cred = StakeCredential::AddrKeyHash(*owner_hash);
            // Owner must be delegated to THIS pool to count.
            if snapshot.delegations.get(&cred) == Some(pool_hash) {
                snapshot.stake.get(&cred)
            } else {
                0
            }
        })
        .sum();

    if owner_delegated_stake < pool_params.pledge {
        // Pool owners have not met the declared pledge — no rewards.
        return PoolRewardBreakdown {
            apparent_performance_reward: 0,
            leader_reward: 0,
            member_rewards: BTreeMap::new(),
        };
    }

    let optimal = max_pool_reward(
        rewards_pot,
        params.n_opt,
        params.a0,
        pool_stake,
        pool_params.pledge,
        total_stake_for_sigma,
    );

    // poolR = floor(apparentPerformance × maxP)
    let apparent = mul_rational(optimal, performance);

    let cost = pool_params.cost.max(params.min_pool_cost);

    // Upstream `calcStakePoolOperatorReward`: when f <= cost, leader
    // receives the entire pool reward `f` (not zero).
    // Members receive nothing.
    if apparent <= cost {
        return PoolRewardBreakdown {
            apparent_performance_reward: apparent,
            leader_reward: apparent,
            member_rewards: BTreeMap::new(),
        };
    }

    let profit = apparent - cost;

    // Build owner set for O(1) lookup.
    let owner_set: std::collections::BTreeSet<StakeCredential> = pool_params
        .pool_owners
        .iter()
        .map(|h| StakeCredential::AddrKeyHash(*h))
        .collect();

    // -- Leader reward (upstream `calcStakePoolOperatorReward`) --
    //
    // cost + floor(profit × (m + (1−m) × s/σ))
    //
    // where s/σ = ownerDelegatedStake / poolStake (the σ and s denominators
    // cancel since both are divided by totalStake).
    //
    // We combine into a single rational:
    //   m + (1−m) × s/σ = (m_num × pool + (m_den − m_num) × owner) / (m_den × pool)
    //
    // Then leader_extra = floor(profit × combined_num / combined_den).
    let m_num = pool_params.margin.numerator as u128;
    let m_den = pool_params.margin.denominator.max(1) as u128;
    let own = owner_delegated_stake as u128;
    let pool = pool_stake.max(1) as u128;
    let p = profit as u128;

    let one_minus_m_num = m_den.saturating_sub(m_num);

    // combined_num = m_num * pool + (m_den - m_num) * owner
    // combined_den = m_den * pool
    let combined_num = m_num.saturating_mul(pool)
        .saturating_add(one_minus_m_num.saturating_mul(own));
    let combined_den = m_den.saturating_mul(pool);

    let leader_extra = floor_mul_div(p, combined_num, combined_den) as u64;
    let leader_reward = cost.saturating_add(leader_extra);

    // -- Member rewards (upstream `calcStakePoolMemberReward`) --
    //
    // For each non-owner delegator:
    //   floor(profit × (1−m) × memberStake / poolStake)
    // = floor(profit × (m_den − m_num) × memberStake / (m_den × poolStake))
    let member_den = m_den.saturating_mul(pool);
    let mut member_rewards = BTreeMap::new();

    for (cred, delegated_pool) in snapshot.delegations.iter() {
        if delegated_pool != pool_hash {
            continue;
        }
        // Pool owners are excluded from member rewards — their share
        // is folded into the leader reward via the `s/σ` term.
        // Reference: `notPoolOwner` check in `rewardOnePoolMember`.
        if owner_set.contains(cred) {
            continue;
        }
        let member_stake = snapshot.stake.get(cred);
        if member_stake == 0 {
            continue;
        }
        // floor(profit * (1-m) * memberStake / poolStake)
        let member_reward = if member_den > 0 {
            // floor(profit * (m_den - m_num) * memberStake / (m_den * poolStake))
            floor_mul_div(p, one_minus_m_num.saturating_mul(member_stake as u128), member_den)
                as u64
        } else {
            0
        };
        if member_reward > 0 {
            member_rewards.insert(*cred, member_reward);
        }
    }

    PoolRewardBreakdown {
        apparent_performance_reward: apparent,
        leader_reward,
        member_rewards,
    }
}

// ---------------------------------------------------------------------------
// Epoch-level reward distribution
// ---------------------------------------------------------------------------

/// Full epoch reward distribution result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EpochRewardDistribution {
    /// Per-credential reward amounts.
    ///
    /// Keyed by `StakeCredential` (not `RewardAccount`) matching upstream
    /// `RewardAns (Map (Credential Staking) …)`.  The credential→account
    /// mapping is resolved at application time from the current DState.
    pub reward_deltas: BTreeMap<StakeCredential, u64>,
    /// Leader rewards keyed by the pool's declared `RewardAccount`.
    ///
    /// Separated from member rewards because leaders use the explicit
    /// pool reward account (which carries its own network byte), while
    /// members are keyed by credential and resolved at application time.
    pub leader_deltas: BTreeMap<RewardAccount, u64>,
    /// Treasury τ-cut: `⌊τ × (fees + deltaR1)⌋`.
    ///
    /// This is the **only** amount from the reward calculation that goes
    /// to the treasury.  Unclaimed rewards (deltaR2) go **back to
    /// reserves** in the upstream accounting.
    ///
    /// Reference: `completeRupd` in
    /// `Cardano.Ledger.Shelley.LedgerState.PulsingReward`:
    /// `deltaT = DeltaCoin deltaT1`.
    pub treasury_cut: u64,
    /// Total distributed to pools/members.
    pub distributed: u64,
    /// Unclaimed rewards (`deltaR2 = _R - sumRewards`).
    ///
    /// Upstream returns this to **reserves**, not treasury:
    /// `deltaR = invert deltaR1 <> toDeltaCoin deltaR2`.
    pub unclaimed: u64,
    /// Monetary expansion drawn from reserves (ΔR1 = ⌊min(1,η)×ρ×reserves⌋).
    ///
    /// The net reserves change is `-(delta_reserves - unclaimed)`:
    /// `deltaR1` is withdrawn, `deltaR2` is returned.
    pub delta_reserves: u64,
}

/// Computes reward distribution for all pools at an epoch boundary.
///
/// Uses the **go** snapshot for stake data and the accumulated fee pot.
/// Pool performance is a per-pool ratio of blocks produced vs. expected,
/// typically derived via `derive_pool_performance()` in `epoch_boundary.rs`.
///
/// Reference: `createRUpd` in `Cardano.Ledger.Shelley.Rewards`.
pub fn compute_epoch_rewards(
    params: &RewardParams,
    go_snapshot: &StakeSnapshot,
    pool_performance: &BTreeMap<PoolKeyHash, UnitInterval>,
) -> EpochRewardDistribution {
    let pot = compute_epoch_reward_pot(params);
    let pool_dist = go_snapshot.pool_stake_distribution();

    let mut reward_deltas: BTreeMap<StakeCredential, u64> = BTreeMap::new();
    let mut leader_deltas: BTreeMap<RewardAccount, u64> = BTreeMap::new();
    let mut total_distributed: u64 = 0;

    // Pools absent from the performance map made zero blocks and receive
    // zero reward.  Upstream `mkPoolRewardInfo` returns `Left` (no
    // PoolRewardInfo) for pools not in `BlocksMade`.
    let zero_perf = UnitInterval {
        numerator: 0,
        denominator: 1,
    };

    for pool_hash in go_snapshot.pool_params.keys() {
        let performance = pool_performance
            .get(pool_hash)
            .copied()
            .unwrap_or(zero_perf);

        let breakdown = compute_pool_reward(
            pot.rewards_pot,
            params,
            pool_hash,
            go_snapshot,
            &pool_dist,
            performance,
        );

        // Leader reward → pool's declared reward account.
        if breakdown.leader_reward > 0 {
            let reward_account = go_snapshot
                .pool_params
                .get(pool_hash)
                .map(|pp| pp.reward_account)
                .expect("pool_params should contain pool_hash from iteration");
            *leader_deltas.entry(reward_account).or_insert(0) += breakdown.leader_reward;
            total_distributed += breakdown.leader_reward;
        }

        // Member rewards → keyed by credential (upstream `RewardAns`).
        // The credential→RewardAccount mapping is resolved at application
        // time from the current DState, not from the pool operator's
        // reward account.
        // Reference: `rewardOnePoolMember` in
        // `Cardano.Ledger.Shelley.Rewards` returns `Maybe Coin`.
        for (cred, amount) in &breakdown.member_rewards {
            *reward_deltas.entry(*cred).or_insert(0) += amount;
            total_distributed += amount;
        }
    }

    let unclaimed = pot.rewards_pot.saturating_sub(total_distributed);

    EpochRewardDistribution {
        reward_deltas,
        leader_deltas,
        treasury_cut: pot.treasury_cut,
        distributed: total_distributed,
        unclaimed,
        delta_reserves: pot.delta_reserves,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::PoolParams;

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

    fn test_pool_params(b: u8, pledge: u64, cost: u64, margin: UnitInterval) -> PoolParams {
        PoolParams {
            operator: test_pool(b),
            vrf_keyhash: [b; 32],
            pledge,
            cost,
            margin,
            reward_account: test_reward_account(b),
            pool_owners: vec![[b; 28]],
            relays: vec![],
            pool_metadata: None,
        }
    }

    fn default_params(reserves: u64, fee_pot: u64) -> RewardParams {
        RewardParams {
            rho: UnitInterval {
                numerator: 3,
                denominator: 1000,
            },
            tau: UnitInterval {
                numerator: 2,
                denominator: 10,
            },
            a0: UnitInterval {
                numerator: 3,
                denominator: 10,
            },
            n_opt: 150,
            min_pool_cost: 340_000_000,
            reserves,
            fee_pot,
            max_lovelace_supply: 0,
            eta: UnitInterval { numerator: 1, denominator: 1 },
        }
    }

    #[test]
    fn epoch_reward_pot_basic() {
        let params = default_params(10_000_000_000_000, 500_000_000);
        let pot = compute_epoch_reward_pot(&params);

        // delta_reserves = 10T × 3/1000 = 30B
        assert_eq!(pot.delta_reserves, 30_000_000_000);

        // total = 30B + 500M = 30,500,000,000
        let total = pot.delta_reserves + params.fee_pot;
        assert_eq!(total, 30_500_000_000);

        // treasury = total × 2/10 = 6,100,000,000
        assert_eq!(pot.treasury_cut, 6_100_000_000);

        // rewards_pot = total - treasury = 24,400,000,000
        assert_eq!(pot.rewards_pot, 24_400_000_000);
    }

    #[test]
    fn epoch_reward_pot_zero_reserves() {
        let params = default_params(0, 1_000_000);
        let pot = compute_epoch_reward_pot(&params);
        assert_eq!(pot.delta_reserves, 0);
        assert_eq!(pot.rewards_pot, 800_000); // 1M - 1M*2/10
        assert_eq!(pot.treasury_cut, 200_000);
    }

    #[test]
    fn max_pool_reward_zero_inputs() {
        let a0 = UnitInterval {
            numerator: 3,
            denominator: 10,
        };
        assert_eq!(max_pool_reward(0, 150, a0, 1000, 100, 10000), 0);
        assert_eq!(max_pool_reward(1000, 0, a0, 1000, 100, 10000), 0);
        assert_eq!(max_pool_reward(1000, 150, a0, 1000, 100, 0), 0);
    }

    #[test]
    fn max_pool_reward_basic() {
        let a0 = UnitInterval {
            numerator: 3,
            denominator: 10,
        };
        // With a single pool holding all the stake, reward should be close to
        // the full rewards_pot.
        let reward = max_pool_reward(
            24_400_000_000, // 24.4B rewards pot
            150,
            a0,
            10_000_000_000_000, // 10T pool stake
            500_000_000_000,    // 500B pledge
            10_000_000_000_000, // 10T total
        );
        // A single pool at saturation (σ'=z=1/n_opt) gets ≈ R/n_opt = 24.4B/150 ≈ 162.7M.
        assert!(reward > 160_000_000);
        assert!(reward < 170_000_000);
    }

    #[test]
    fn pool_reward_cost_exceeds_apparent() {
        let params = default_params(10_000_000_000_000, 0);
        let pool = test_pool(1);

        let mut snapshot = StakeSnapshot::empty();
        // Very small pool — its optimal reward will be tiny.
        snapshot.stake.add(test_cred(1), 1000);
        snapshot.delegations.insert(test_cred(1), pool);
        snapshot.pool_params.insert(
            pool,
            test_pool_params(
                1,
                1000,
                340_000_000, // cost = 340 ADA
                UnitInterval {
                    numerator: 1,
                    denominator: 100,
                },
            ),
        );

        let pool_dist = snapshot.pool_stake_distribution();
        let perfect = UnitInterval {
            numerator: 1,
            denominator: 1,
        };

        let breakdown =
            compute_pool_reward(100, &params, &pool, &snapshot, &pool_dist, perfect);

        // The apparent reward (100 lovelace pot, tiny pool) < cost of 340 ADA.
        assert_eq!(breakdown.leader_reward, 0);
        assert!(breakdown.member_rewards.is_empty());
    }

    #[test]
    fn pool_reward_distribution_two_members() {
        let params = RewardParams {
            rho: UnitInterval {
                numerator: 3,
                denominator: 1000,
            },
            tau: UnitInterval {
                numerator: 2,
                denominator: 10,
            },
            a0: UnitInterval {
                numerator: 0,
                denominator: 1,
            },
            n_opt: 1, // Only 1 pool saturated → z = 1
            min_pool_cost: 0,
            reserves: 0,
            fee_pot: 0,
            max_lovelace_supply: 0,
            eta: UnitInterval { numerator: 1, denominator: 1 },
        };

        let pool = test_pool(1);
        let operator_cred = test_cred(1);
        let member_cred = test_cred(2);

        let mut snapshot = StakeSnapshot::empty();
        snapshot.stake.add(operator_cred, 3000);
        snapshot.stake.add(member_cred, 7000);
        snapshot.delegations.insert(operator_cred, pool);
        snapshot.delegations.insert(member_cred, pool);
        snapshot.pool_params.insert(
            pool,
            test_pool_params(
                1,
                3000,
                100, // cost = 100 lovelace
                UnitInterval {
                    numerator: 10,
                    denominator: 100,
                },
            ),
        );

        let pool_dist = snapshot.pool_stake_distribution();
        let perfect = UnitInterval {
            numerator: 1,
            denominator: 1,
        };

        // Use a direct rewards pot to test distribution.
        let breakdown =
            compute_pool_reward(10_000, &params, &pool, &snapshot, &pool_dist, perfect);

        // The pool has 100% of total stake, n_opt=1, a0=0.
        // maxPool with a0=0 simplifies to R × σ' (since the pledge term vanishes).
        // σ' = min(pool_stake/total, 1/n_opt) = min(1, 1) = 1.
        // maxPool ≈ R × 1 = 10000 (with a0=0 the denominator is 1).
        // apparent = 10000
        // cost = 100, profit = 9900
        // margin_share = 9900 × 10/100 = 990
        // member_pot = 9900 - 990 = 8910
        // operator member share = 8910 × 3000/10000 = 2673
        // member share = 8910 × 7000/10000 = 6237
        // leader_reward = cost + margin + operator_member = 100 + 990 + 2673 = 3763
        assert_eq!(breakdown.leader_reward, 3763);
        assert_eq!(breakdown.member_rewards.get(&member_cred), Some(&6237));
    }

    #[test]
    fn compute_epoch_rewards_integration() {
        let params = RewardParams {
            rho: UnitInterval {
                numerator: 1,
                denominator: 10,
            },
            tau: UnitInterval {
                numerator: 0,
                denominator: 1,
            },
            a0: UnitInterval {
                numerator: 0,
                denominator: 1,
            },
            n_opt: 1,
            min_pool_cost: 0,
            reserves: 100_000,
            fee_pot: 0,
            max_lovelace_supply: 0,
            eta: UnitInterval { numerator: 1, denominator: 1 },
        };

        let pool = test_pool(1);
        let operator_cred = test_cred(1);

        let mut snapshot = StakeSnapshot::empty();
        snapshot.stake.add(operator_cred, 1000);
        snapshot.delegations.insert(operator_cred, pool);
        snapshot.pool_params.insert(
            pool,
            test_pool_params(
                1,
                1000,
                0,
                UnitInterval {
                    numerator: 0,
                    denominator: 1,
                },
            ),
        );

        let perf_map = BTreeMap::from([(
            pool,
            UnitInterval {
                numerator: 1,
                denominator: 1,
            },
        )]);

        let dist = compute_epoch_rewards(&params, &snapshot, &perf_map);

        // Reserves = 100000, rho = 1/10 → delta = 10000
        // tau = 0 → treasury = 0, rewards_pot = 10000
        // Single pool with all stake, a0=0, n_opt=1 → maxPool = 10000
        // cost = 0, margin = 0 → leader gets full member share = 10000
        assert_eq!(dist.treasury_cut, 0);
        // With tau=0, no treasury cut; only unclaimed (rounding) returned
        // to reserves.
        assert!(dist.distributed > 0);
        assert!(dist.distributed <= 10000);
        // delta_reserves should match reserves × rho = 100000 × 1/10 = 10000.
        assert_eq!(dist.delta_reserves, 10000);
    }

    #[test]
    fn compute_epoch_rewards_delta_reserves_independent_of_fees() {
        // delta_reserves should only depend on reserves × rho,
        // NOT on the fee_pot.
        let params_no_fees = RewardParams {
            rho: UnitInterval { numerator: 5, denominator: 100 },
            tau: UnitInterval { numerator: 0, denominator: 1 },
            a0: UnitInterval { numerator: 0, denominator: 1 },
            n_opt: 1,
            min_pool_cost: 0,
            reserves: 200_000,
            fee_pot: 0,
            max_lovelace_supply: 0,
            eta: UnitInterval { numerator: 1, denominator: 1 },
        };
        let params_with_fees = RewardParams {
            fee_pot: 50_000,
            ..params_no_fees.clone()
        };

        let pool = test_pool(1);
        let mut snapshot = StakeSnapshot::empty();
        snapshot.stake.add(test_cred(1), 1000);
        snapshot.delegations.insert(test_cred(1), pool);
        snapshot.pool_params.insert(
            pool,
            test_pool_params(1, 1000, 0, UnitInterval { numerator: 0, denominator: 1 }),
        );

        let perf = BTreeMap::from([(pool, UnitInterval { numerator: 1, denominator: 1 })]);

        let dist_no_fees = compute_epoch_rewards(&params_no_fees, &snapshot, &perf);
        let dist_with_fees = compute_epoch_rewards(&params_with_fees, &snapshot, &perf);

        // delta_reserves = 200000 × 5/100 = 10000 regardless of fee_pot.
        assert_eq!(dist_no_fees.delta_reserves, 10000);
        assert_eq!(dist_with_fees.delta_reserves, 10000);
    }

    #[test]
    fn mul_rational_basic() {
        assert_eq!(
            mul_rational(
                1000,
                UnitInterval {
                    numerator: 3,
                    denominator: 10
                }
            ),
            300
        );
        assert_eq!(
            mul_rational(
                1000,
                UnitInterval {
                    numerator: 0,
                    denominator: 1
                }
            ),
            0
        );
        assert_eq!(
            mul_rational(
                1000,
                UnitInterval {
                    numerator: 1,
                    denominator: 0
                }
            ),
            0
        );
    }

    // -- Upstream-parity tests --

    #[test]
    fn leader_gets_full_reward_when_below_cost() {
        // Upstream `calcStakePoolOperatorReward`: when f <= cost, the
        // leader receives the entire pool reward `f`, not zero.
        let params = default_params(10_000_000_000_000, 500_000_000);
        let pool = test_pool(1);

        let mut snapshot = StakeSnapshot::empty();
        snapshot.stake.add(test_cred(1), 500_000_000_000);
        snapshot.delegations.insert(test_cred(1), pool);
        snapshot.pool_params.insert(
            pool,
            test_pool_params(
                1,
                500_000_000_000,
                500_000_000_000, // cost = 500B (very high)
                UnitInterval { numerator: 1, denominator: 10 },
            ),
        );

        let pool_dist = snapshot.pool_stake_distribution();
        let perfect = UnitInterval { numerator: 1, denominator: 1 };

        // Use a small pot so apparent < cost.
        let breakdown = compute_pool_reward(1_000_000, &params, &pool, &snapshot, &pool_dist, perfect);

        // apparent is non-zero but < cost → leader should get the full apparent amount.
        assert!(breakdown.apparent_performance_reward > 0);
        assert!(breakdown.apparent_performance_reward <= 1_000_000);
        assert_eq!(breakdown.leader_reward, breakdown.apparent_performance_reward);
        assert!(breakdown.member_rewards.is_empty());
    }

    #[test]
    fn multi_owner_pool_excludes_owners_from_member_rewards() {
        // Upstream `rewardOnePoolMember` uses `notPoolOwner` to exclude
        // ALL pool owners from member rewards, not just the operator.
        let pool_id = test_pool(1);
        let operator = [1u8; 28];
        let second_owner = [2u8; 28];
        let member = [3u8; 28];

        let pool_params = PoolParams {
            operator,
            vrf_keyhash: [1; 32],
            pledge: 0,
            cost: 0,
            margin: UnitInterval { numerator: 0, denominator: 1 },
            reward_account: test_reward_account(1),
            pool_owners: vec![operator, second_owner],
            relays: vec![],
            pool_metadata: None,
        };

        let mut snapshot = StakeSnapshot::empty();
        snapshot.stake.add(StakeCredential::AddrKeyHash(operator), 3000);
        snapshot.stake.add(StakeCredential::AddrKeyHash(second_owner), 3000);
        snapshot.stake.add(StakeCredential::AddrKeyHash(member), 4000);
        snapshot.delegations.insert(StakeCredential::AddrKeyHash(operator), pool_id);
        snapshot.delegations.insert(StakeCredential::AddrKeyHash(second_owner), pool_id);
        snapshot.delegations.insert(StakeCredential::AddrKeyHash(member), pool_id);
        snapshot.pool_params.insert(pool_id, pool_params);

        let pool_dist = snapshot.pool_stake_distribution();
        let perfect = UnitInterval { numerator: 1, denominator: 1 };

        let params = RewardParams {
            rho: UnitInterval { numerator: 1, denominator: 10 },
            tau: UnitInterval { numerator: 0, denominator: 1 },
            a0: UnitInterval { numerator: 0, denominator: 1 },
            n_opt: 1,
            min_pool_cost: 0,
            reserves: 0,
            fee_pot: 0,
            max_lovelace_supply: 0,
            eta: UnitInterval { numerator: 1, denominator: 1 },
        };

        let breakdown = compute_pool_reward(10_000, &params, &pool_id, &snapshot, &pool_dist, perfect);

        // Both owners are excluded from member_rewards.
        assert!(!breakdown.member_rewards.contains_key(&StakeCredential::AddrKeyHash(operator)));
        assert!(!breakdown.member_rewards.contains_key(&StakeCredential::AddrKeyHash(second_owner)));

        // Only the non-owner member gets a member reward.
        assert!(breakdown.member_rewards.contains_key(&StakeCredential::AddrKeyHash(member)));
        assert_eq!(breakdown.member_rewards.len(), 1);

        // Leader absorbs cost + margin + ALL owners' delegated shares.
        // With margin=0: leader gets cost(0) + floor(profit * (0 + 1 * 6000/10000))
        // profit = 10000, owner_stake=6000, pool=10000
        // leader = floor(10000 * 6000 / 10000) = 6000
        assert_eq!(breakdown.leader_reward, 6000);

        // Member gets floor(profit * 1 * 4000/10000) = floor(4000) = 4000
        assert_eq!(*breakdown.member_rewards.get(&StakeCredential::AddrKeyHash(member)).unwrap(), 4000);
    }

    #[test]
    fn single_floor_leader_formula_matches_upstream() {
        // Verify the single-floor computation:
        // leader = cost + floor(profit * (m + (1-m) * s/sigma))
        // Not: cost + floor(profit*m) + floor(profit*(1-m)*s/sigma)
        let pool_id = test_pool(1);

        let mut snapshot = StakeSnapshot::empty();
        snapshot.stake.add(test_cred(1), 3333);
        snapshot.stake.add(test_cred(2), 6667);
        snapshot.delegations.insert(test_cred(1), pool_id);
        snapshot.delegations.insert(test_cred(2), pool_id);
        snapshot.pool_params.insert(
            pool_id,
            test_pool_params(
                1,
                3333,
                100,
                UnitInterval { numerator: 1, denominator: 3 }, // margin = 1/3
            ),
        );

        let pool_dist = snapshot.pool_stake_distribution();
        let perfect = UnitInterval { numerator: 1, denominator: 1 };

        let params = RewardParams {
            rho: UnitInterval { numerator: 1, denominator: 10 },
            tau: UnitInterval { numerator: 0, denominator: 1 },
            a0: UnitInterval { numerator: 0, denominator: 1 },
            n_opt: 1,
            min_pool_cost: 0,
            reserves: 0,
            fee_pot: 0,
            max_lovelace_supply: 0,
            eta: UnitInterval { numerator: 1, denominator: 1 },
        };

        let breakdown = compute_pool_reward(10_000, &params, &pool_id, &snapshot, &pool_dist, perfect);

        // profit = 10000 - 100 = 9900
        // m = 1/3, s/sigma = 3333/10000
        // inner = 1/3 + (2/3) * (3333/10000) = 1/3 + 6666/30000 = 10000/30000 + 6666/30000 = 16666/30000
        // leader_extra = floor(9900 * 16666/30000) = floor(9900 * 16666 / 30000)
        //              = floor(164973400 / 30000) = floor(5499.1133...) = 5499
        // leader = 100 + 5499 = 5599
        assert_eq!(breakdown.leader_reward, 5599);

        // member_reward = floor(9900 * (2/3) * 6667/10000)
        //               = floor(9900 * 2 * 6667 / (3 * 10000))
        //               = floor(132006600 / 30000) = floor(4400.22) = 4400
        assert_eq!(*breakdown.member_rewards.get(&test_cred(2)).unwrap(), 4400);
    }

    #[test]
    fn pledge_unsatisfied_zero_rewards() {
        // When owner-delegated stake < declared pledge, the pool
        // forfeits all rewards.
        let pool_id = test_pool(1);

        let mut snapshot = StakeSnapshot::empty();
        snapshot.stake.add(test_cred(1), 500);    // owner has only 500
        snapshot.stake.add(test_cred(2), 9500);
        snapshot.delegations.insert(test_cred(1), pool_id);
        snapshot.delegations.insert(test_cred(2), pool_id);
        snapshot.pool_params.insert(
            pool_id,
            test_pool_params(
                1,
                1000, // pledge = 1000, but owner only has 500
                0,
                UnitInterval { numerator: 0, denominator: 1 },
            ),
        );

        let pool_dist = snapshot.pool_stake_distribution();
        let perfect = UnitInterval { numerator: 1, denominator: 1 };
        let params = RewardParams {
            rho: UnitInterval { numerator: 1, denominator: 10 },
            tau: UnitInterval { numerator: 0, denominator: 1 },
            a0: UnitInterval { numerator: 0, denominator: 1 },
            n_opt: 1,
            min_pool_cost: 0,
            reserves: 0,
            fee_pot: 0,
            max_lovelace_supply: 0,
            eta: UnitInterval { numerator: 1, denominator: 1 },
        };

        let breakdown = compute_pool_reward(10_000, &params, &pool_id, &snapshot, &pool_dist, perfect);

        assert_eq!(breakdown.leader_reward, 0);
        assert!(breakdown.member_rewards.is_empty());
    }

    #[test]
    fn circulation_sigma_differs_from_active_stake() {
        // When max_lovelace_supply is set, sigma uses circulation
        // (max_supply - reserves) not active delegated stake.
        let pool_id = test_pool(1);

        let mut snapshot = StakeSnapshot::empty();
        snapshot.stake.add(test_cred(1), 10_000_000);
        snapshot.delegations.insert(test_cred(1), pool_id);
        snapshot.pool_params.insert(
            pool_id,
            test_pool_params(
                1,
                10_000_000,
                0,
                UnitInterval { numerator: 0, denominator: 1 },
            ),
        );

        let pool_dist = snapshot.pool_stake_distribution();
        let perfect = UnitInterval { numerator: 1, denominator: 1 };

        // Without max_lovelace_supply (falls back to active stake).
        let params_no_supply = RewardParams {
            rho: UnitInterval { numerator: 1, denominator: 10 },
            tau: UnitInterval { numerator: 0, denominator: 1 },
            a0: UnitInterval { numerator: 3, denominator: 10 },
            n_opt: 1,
            min_pool_cost: 0,
            reserves: 1_000_000_000,
            fee_pot: 0,
            max_lovelace_supply: 0,
            eta: UnitInterval { numerator: 1, denominator: 1 },
        };

        // With max_lovelace_supply → circulation = 10B - 1B = 9B.
        let params_with_supply = RewardParams {
            max_lovelace_supply: 10_000_000_000,
            ..params_no_supply.clone()
        };

        let b1 = compute_pool_reward(1_000_000, &params_no_supply, &pool_id, &snapshot, &pool_dist, perfect);
        let b2 = compute_pool_reward(1_000_000, &params_with_supply, &pool_id, &snapshot, &pool_dist, perfect);

        // With circulation-based sigma, the pool's relative stake is much
        // smaller (10M/9B vs 10M/10M), so its reward is smaller.
        assert!(b2.leader_reward < b1.leader_reward,
            "circulation sigma ({}) should produce smaller reward than active-stake sigma ({})",
            b2.leader_reward, b1.leader_reward);
    }

    #[test]
    fn eta_scales_monetary_expansion() {
        // When eta < 1, monetary expansion is reduced.
        let params_full = RewardParams {
            rho: UnitInterval { numerator: 3, denominator: 1000 },
            tau: UnitInterval { numerator: 0, denominator: 1 },
            a0: UnitInterval { numerator: 0, denominator: 1 },
            n_opt: 1,
            min_pool_cost: 0,
            reserves: 10_000_000_000,
            fee_pot: 0,
            max_lovelace_supply: 0,
            eta: UnitInterval { numerator: 1, denominator: 1 },
        };

        let params_half = RewardParams {
            eta: UnitInterval { numerator: 1, denominator: 2 },
            ..params_full.clone()
        };

        let pot_full = compute_epoch_reward_pot(&params_full);
        let pot_half = compute_epoch_reward_pot(&params_half);

        // eta=1: delta_reserves = 10B * 3/1000 = 30M
        assert_eq!(pot_full.delta_reserves, 30_000_000);
        // eta=1/2: delta_reserves = 30M * 1/2 = 15M
        assert_eq!(pot_half.delta_reserves, 15_000_000);
    }

    #[test]
    fn floor_mul_div_basic() {
        // Non-overflowing cases.
        assert_eq!(floor_mul_div(10, 3, 4), 7);   // 30/4 = 7
        assert_eq!(floor_mul_div(100, 7, 10), 70); // 700/10 = 70
        assert_eq!(floor_mul_div(0, 100, 1), 0);
        assert_eq!(floor_mul_div(100, 0, 1), 0);
        assert_eq!(floor_mul_div(100, 1, 0), 0);   // division by zero → 0
    }

    #[test]
    fn floor_mul_div_overflow_fallback() {
        // Force an overflow in a * b by using large u128 values.
        let a = u128::MAX / 2;
        let b = 3u128;
        let c = 4u128;
        // a * b would overflow, but the fallback should still compute correctly.
        let result = floor_mul_div(a, b, c);
        // Expected: (MAX/2 * 3) / 4 ≈ MAX * 3/8
        let expected = (a / c) * b + (a % c) * b / c;
        assert_eq!(result, expected);
    }
}
