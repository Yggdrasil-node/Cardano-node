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
/// 1. `delta_reserves = reserves × ρ`
/// 2. `total_reward = delta_reserves + fee_pot`
/// 3. `treasury_cut = total_reward × τ`
/// 4. `rewards_pot = total_reward - treasury_cut`
///
/// Reference: RUPD rule in the Shelley formal specification.
pub fn compute_epoch_reward_pot(params: &RewardParams) -> EpochRewardPot {
    let delta_reserves = mul_rational(params.reserves, params.rho);
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
/// 1. `optimal = maxPool(R, n_opt, a0, pool_stake, pledge, total_stake)`
/// 2. `apparent = optimal × performance` (clamped to \[0, 1\])
/// 3. If `apparent < cost`, the pool gets no reward (and the remainder
///    goes to treasury as unallocated).
/// 4. `profit = apparent - cost`
/// 5. `leader_reward = cost + ⌊profit × margin⌋ + leader_member_share`
/// 6. Each member gets `⌊profit × (1 - margin) × member_stake / pool_member_stake⌋`
///
/// Reference: `leaderRew`, `memberRew` in `Cardano.Ledger.Shelley.Rewards`.
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
    let total_stake = pool_dist.total_active_stake();

    let optimal = max_pool_reward(
        rewards_pot,
        params.n_opt,
        params.a0,
        pool_stake,
        pool_params.pledge,
        total_stake,
    );

    // Apparent pool reward = optimal × performance (performance ∈ [0, 1]).
    let apparent = mul_rational(optimal, performance);

    let cost = pool_params.cost.max(params.min_pool_cost);

    if apparent <= cost {
        // Pool reward does not cover declared cost — no distribution.
        // The formal spec sends the unclaimed portion to the treasury.
        return PoolRewardBreakdown {
            apparent_performance_reward: apparent,
            leader_reward: 0,
            member_rewards: BTreeMap::new(),
        };
    }

    let profit = apparent - cost;
    let margin_share = mul_rational(profit, pool_params.margin);
    let member_pot = profit - margin_share;

    // Distribute member_pot proportionally among delegators.
    let operator_cred = StakeCredential::AddrKeyHash(pool_params.operator);
    let mut member_rewards = BTreeMap::new();
    let mut total_member_stake: u64 = 0;

    // Enumerate delegators and their stake for this pool.
    for (cred, delegated_pool) in snapshot.delegations.iter() {
        if delegated_pool != pool_hash {
            continue;
        }
        let member_stake = snapshot.stake.get(cred);
        if member_stake == 0 {
            continue;
        }
        total_member_stake = total_member_stake.saturating_add(member_stake);
    }

    // Leader's own member share comes from their stake as a regular delegator.
    let mut leader_member_reward: u64 = 0;

    if total_member_stake > 0 {
        for (cred, delegated_pool) in snapshot.delegations.iter() {
            if delegated_pool != pool_hash {
                continue;
            }
            let member_stake = snapshot.stake.get(cred);
            if member_stake == 0 {
                continue;
            }
            // member_reward = member_pot × member_stake / total_member_stake
            let member_reward =
                (member_pot as u128 * member_stake as u128 / total_member_stake as u128) as u64;
            if member_reward == 0 {
                continue;
            }

            if *cred == operator_cred {
                // Operator's delegator share is folded into leader_reward.
                leader_member_reward = member_reward;
            } else {
                member_rewards.insert(*cred, member_reward);
            }
        }
    }

    let leader_reward = cost + margin_share + leader_member_reward;

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
    /// Per-reward-account reward amounts.
    pub reward_deltas: BTreeMap<RewardAccount, u64>,
    /// Amount allocated to the treasury this epoch.
    pub treasury_delta: u64,
    /// Total distributed to pools/members.
    pub distributed: u64,
    /// Unclaimed rewards (cost-exceeds-apparent, rounding).
    pub unclaimed: u64,
}

/// Computes reward distribution for all pools at an epoch boundary.
///
/// Uses the **go** snapshot for stake data and the accumulated fee pot.
/// Pool performance is currently passed as a uniform value (simplified;
/// a full implementation would derive per-pool performance from the
/// number of blocks produced vs. expected).
///
/// Reference: `createRUpd` in `Cardano.Ledger.Shelley.Rewards`.
pub fn compute_epoch_rewards(
    params: &RewardParams,
    go_snapshot: &StakeSnapshot,
    pool_performance: &BTreeMap<PoolKeyHash, UnitInterval>,
) -> EpochRewardDistribution {
    let pot = compute_epoch_reward_pot(params);
    let pool_dist = go_snapshot.pool_stake_distribution();

    let mut reward_deltas: BTreeMap<RewardAccount, u64> = BTreeMap::new();
    let mut total_distributed: u64 = 0;

    let perfect = UnitInterval {
        numerator: 1,
        denominator: 1,
    };

    for pool_hash in go_snapshot.pool_params.keys() {
        let performance = pool_performance
            .get(pool_hash)
            .copied()
            .unwrap_or(perfect);

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
            *reward_deltas.entry(reward_account).or_insert(0) += breakdown.leader_reward;
            total_distributed += breakdown.leader_reward;
        }

        // Member rewards → each delegator's reward account.
        for (cred, amount) in &breakdown.member_rewards {
            // The member's reward account uses the same network as the pool's
            // reward account and their own credential.
            let network = go_snapshot
                .pool_params
                .get(pool_hash)
                .map(|pp| pp.reward_account.network)
                .unwrap_or(1);
            let member_account = RewardAccount {
                network,
                credential: *cred,
            };
            *reward_deltas.entry(member_account).or_insert(0) += amount;
            total_distributed += amount;
        }
    }

    let unclaimed = pot.rewards_pot.saturating_sub(total_distributed);

    EpochRewardDistribution {
        reward_deltas,
        treasury_delta: pot.treasury_cut.saturating_add(unclaimed),
        distributed: total_distributed,
        unclaimed,
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
        assert_eq!(dist.treasury_delta, dist.unclaimed);
        assert!(dist.distributed > 0);
        assert!(dist.distributed <= 10000);
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
}
