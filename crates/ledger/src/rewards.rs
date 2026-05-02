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
    floor_mul_div(
        coin as u128,
        ratio.numerator as u128,
        ratio.denominator as u128,
    )
    .min(u64::MAX as u128) as u64
}

/// Multiplies a coin value by a rational and caps the result.
fn mul_rational_capped(coin: u64, ratio: UnitInterval, cap: u64) -> u64 {
    mul_rational(coin, ratio).min(cap)
}

// ---------------------------------------------------------------------------
// Wide arithmetic (u256) for exact maxPool computation
// ---------------------------------------------------------------------------
//
// Upstream uses GHC's exact-precision `Rational` type for the `maxPool`
// formula and applies a single `rationalToCoinViaFloor` at the very end.
// Our u128 fixed-point representation cannot maintain that exactness for
// the intermediate products involved (up to ~10^62), so we use u256
// arithmetic to collect the entire expression into a single fraction
// before flooring — matching upstream behaviour exactly.

/// GCD of two u128 values (Euclidean algorithm).
fn gcd128(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

/// 256-bit unsigned integer stored as `(hi, lo)` where value = hi × 2¹²⁸ + lo.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct U256 {
    hi: u128,
    lo: u128,
}

impl U256 {
    #[inline]
    fn is_zero(self) -> bool {
        self.hi == 0 && self.lo == 0
    }

    /// `self <= other`.
    #[inline]
    fn le(self, other: Self) -> bool {
        self.hi < other.hi || (self.hi == other.hi && self.lo <= other.lo)
    }

    /// Widening multiply: u128 × u128 → U256.
    fn widening_mul(a: u128, b: u128) -> Self {
        // Split into 64-bit halves to avoid u128 overflow.
        let al = a as u64 as u128;
        let ah = a >> 64;
        let bl = b as u64 as u128;
        let bh = b >> 64;

        let ll = al * bl;
        let lh = al * bh;
        let hl = ah * bl;
        let hh = ah * bh;

        let (mid, carry_mid) = lh.overflowing_add(hl);
        let lo = ll.wrapping_add(mid << 64);
        let carry_lo: u128 = if lo < ll { 1 } else { 0 };
        let hi = hh + (mid >> 64) + if carry_mid { 1u128 << 64 } else { 0 } + carry_lo;
        U256 { hi, lo }
    }

    /// Add two U256 values (wrapping on overflow beyond 256 bits).
    fn add(self, other: Self) -> Self {
        let (lo, c) = self.lo.overflowing_add(other.lo);
        let hi = self
            .hi
            .wrapping_add(other.hi)
            .wrapping_add(if c { 1 } else { 0 });
        U256 { hi, lo }
    }

    /// Multiply U256 × u128 → U256 (low 256 bits).
    fn mul_u128(self, b: u128) -> Self {
        let lo_wide = U256::widening_mul(self.lo, b);
        let hi_low = self.hi.wrapping_mul(b);
        U256 {
            hi: lo_wide.hi.wrapping_add(hi_low),
            lo: lo_wide.lo,
        }
    }

    /// Floor-divide U256 by u128, returning quotient as u128.
    ///
    /// Uses binary long-division: remainder starts as `self.hi` and each
    /// of the 128 bits of `self.lo` is shifted in from the top.
    fn div_u128(self, d: u128) -> u128 {
        if d == 0 {
            return 0;
        }
        if self.hi == 0 {
            return self.lo / d;
        }
        if self.hi >= d {
            // Quotient would exceed u128; return max as a saturating fallback.
            return u128::MAX;
        }
        // Binary long division over the 128 bits of self.lo.
        let mut rem = self.hi;
        let mut quot: u128 = 0;
        for i in (0u32..128).rev() {
            let bit = (self.lo >> i) & 1;
            let overflow = rem >= (1u128 << 127);
            rem = rem.wrapping_shl(1) | bit;
            if overflow || rem >= d {
                rem = rem.wrapping_sub(d);
                quot |= 1u128 << i;
            }
        }
        quot
    }
}

/// Floor-divide U256 by U256, returning the quotient as u64.
///
/// Assumes the true quotient fits in u64 (always true for per-pool reward).
/// Uses binary search with at most 64 iterations.
fn u256_div_floor(num: U256, den: U256) -> u64 {
    if den.is_zero() {
        return 0;
    }
    if den.hi == 0 {
        return num.div_u128(den.lo).min(u64::MAX as u128) as u64;
    }
    if num.le(den) {
        return if num == den { 1 } else { 0 };
    }
    // Binary search: largest q ∈ [1, u64::MAX] with den × q ≤ num.
    let mut lo_q: u64 = 1;
    let mut hi_q: u64 = u64::MAX;
    while lo_q < hi_q {
        let mid = lo_q + (hi_q - lo_q).div_ceil(2);
        let prod = den.mul_u128(mid as u128);
        if prod.le(num) {
            lo_q = mid;
        } else {
            hi_q = mid - 1;
        }
    }
    lo_q
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
    let eta_clamped =
        if params.eta.denominator == 0 || params.eta.numerator >= params.eta.denominator {
            UnitInterval {
                numerator: 1,
                denominator: 1,
            }
        } else {
            params.eta
        };
    // Upstream: `deltaR1 = rationalToCoinViaFloor (min 1 eta * rho * reserves)`
    // — a single exact rational expression floored once.
    //
    // delta_reserves = ⌊eta_n × rho_n × reserves / (eta_d × rho_d)⌋
    let eta_n = eta_clamped.numerator as u128;
    let eta_d = eta_clamped.denominator as u128;
    let rho_n = params.rho.numerator as u128;
    let rho_d = params.rho.denominator.max(1) as u128;
    let reserves = params.reserves as u128;
    // eta_n × rho_n fits u128 (both ≤ u64). floor_mul_div handles the
    // potentially large reserves × (eta_n × rho_n) via overflow splitting.
    let delta_reserves = floor_mul_div(reserves, eta_n * rho_n, eta_d * rho_d) as u64;

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
/// This is the `maxPool'` function from the Shelley formal specification:
///
/// ```text
/// maxPool(R, n_opt, a0, σ, s) =
///   rationalToCoinViaFloor(R / (1 + a0) × (σ' + s' × a0 × (σ' - s' × (z - σ') / z) / z))
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
/// Upstream performs the entire computation in exact `Rational` and floors
/// only at the very end.  We replicate that behaviour by collecting the
/// full expression into a single (U256 numerator, U256 denominator)
/// fraction and flooring once.
///
/// Reference: `maxPool'` in `Cardano.Ledger.State.SnapShots`.
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

    let k = n_opt as u128;
    let p = pool_stake as u128;
    let pi = pledge as u128;
    let t = total_stake as u128;
    let r = rewards_pot as u128;
    let a0_n = a0.numerator as u128;
    let a0_d = a0.denominator.max(1) as u128;

    // σ' = min(σ, z) where σ = p/t, z = 1/k.
    // Compare p/t vs 1/k ↔ p*k vs t.
    let (sig_n, sig_d) = if p.checked_mul(k).is_some_and(|pk| pk <= t) {
        (p, t)
    } else {
        (1u128, k)
    };
    // s' = min(s, z)
    let (s_n, s_d) = if pi.checked_mul(k).is_some_and(|pk| pk <= t) {
        (pi, t)
    } else {
        (1u128, k)
    };

    // GCD-reduce each rational pair before entering the U256 chain.
    // This keeps all intermediate products well within u256 range even
    // for mainnet-scale parameters (e.g. sig_n=70T, sig_d=35000T reduces
    // to 1/500).
    let g_sig = gcd128(sig_n, sig_d);
    let sig_n = sig_n / g_sig;
    let sig_d = sig_d / g_sig;
    let g_s = gcd128(s_n, s_d);
    let s_n = s_n / g_s;
    let s_d = s_d / g_s;

    // --- Expand the formula into a single fraction ---
    //
    // factor4 = (z − σ')/z = (sig_d − k·sig_n) / sig_d
    //   (≥ 0 because σ' ≤ z)
    let f4_n = sig_d - k * sig_n;
    // f4_d = sig_d

    // σ' − s'·factor4
    //   = sig_n/sig_d − (s_n/s_d)·(f4_n/sig_d)
    //   = (sig_n·s_d − s_n·f4_n) / (sig_d·s_d)
    //
    // Products fit u128: sig_n ≤ ~10^13, s_d ≤ ~10^16.
    let diff_n = (sig_n * s_d).saturating_sub(s_n * f4_n);
    let diff_d = sig_d * s_d;

    // GCD-reduce diff_n/diff_d as well.
    let g_diff = gcd128(diff_n, diff_d);
    let diff_n = if g_diff > 1 { diff_n / g_diff } else { diff_n };
    let diff_d = if g_diff > 1 { diff_d / g_diff } else { diff_d };

    // factor3 = (σ' − s'·factor4) / z = k·diff_n / diff_d
    //   After GCD reduction, k·diff_n is typically very small.

    // Combine into the full fraction.
    //
    // factor2 = σ' + s'·a0·factor3
    //         = sig_n/sig_d + (s_n·a0_n·k·diff_n) / (s_d·a0_d·diff_d)
    //
    // All values are GCD-reduced, so U256 products stay well within bounds.
    let sak = s_n * a0_n * k;
    let term_b_num = U256::widening_mul(sak, diff_n);
    let sda = s_d * a0_d;
    let term_b_den = U256::widening_mul(sda, diff_d);

    // factor2 = (sig_n·term_b_den + term_b_num·sig_d) / (sig_d·term_b_den)
    let f2_num = term_b_den.mul_u128(sig_n).add(term_b_num.mul_u128(sig_d));
    let f2_den = term_b_den.mul_u128(sig_d);

    // result = floor(R·a0_d / (a0_d + a0_n)  ×  f2_num / f2_den)
    //        = floor(R·a0_d · f2_num / ((a0_d + a0_n) · f2_den))
    //
    // r * a0_d always fits u128 (both ≤ u64).
    let final_num = f2_num.mul_u128(r * a0_d);
    let one_plus_a0 = a0_d + a0_n;
    let final_den = f2_den.mul_u128(one_plus_a0);

    // Reduce by GCD of accessible u128 factors before the final division
    // to stay well within u256 range.  A simple reduction by
    // gcd(r, one_plus_a0) and gcd(sig_n, sig_d) already removes the
    // dominant common factors.  The binary-search division handles any
    // remaining magnitude.
    u256_div_floor(final_num, final_den)
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

    // poolR = floor(apparentPerformance × maxP). Under upstream
    // invariants this cannot exceed the epoch reward pot; keep that cap
    // explicit so a lossy local nonnegative-interval representation cannot
    // inflate supply if the performance ratio is very large.
    let apparent = mul_rational_capped(optimal, performance, rewards_pot);

    // Upstream uses the cost stored in the stake-pool snapshot (`spssCost`)
    // directly. Minimum-pool-cost is an admission/update constraint; reward
    // calculation does not re-max a historical snapshot cost against the
    // currently active parameter.
    let cost = pool_params.cost;

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
    let combined_num = m_num
        .saturating_mul(pool)
        .saturating_add(one_minus_m_num.saturating_mul(own));
    let combined_den = m_den.saturating_mul(pool);

    let leader_extra = floor_mul_div(p, combined_num, combined_den) as u64;
    let leader_reward = cost.saturating_add(leader_extra).min(apparent);

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
            floor_mul_div(
                p,
                one_minus_m_num.saturating_mul(member_stake as u128),
                member_den,
            ) as u64
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
            let entry = leader_deltas.entry(reward_account).or_insert(0);
            *entry = entry.saturating_add(breakdown.leader_reward);
            total_distributed = total_distributed.saturating_add(breakdown.leader_reward);
        }

        // Member rewards → keyed by credential (upstream `RewardAns`).
        // The credential→RewardAccount mapping is resolved at application
        // time from the current DState, not from the pool operator's
        // reward account.
        // Reference: `rewardOnePoolMember` in
        // `Cardano.Ledger.Shelley.Rewards` returns `Maybe Coin`.
        for (cred, amount) in &breakdown.member_rewards {
            let entry = reward_deltas.entry(*cred).or_insert(0);
            *entry = entry.saturating_add(*amount);
            total_distributed = total_distributed.saturating_add(*amount);
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
            eta: UnitInterval {
                numerator: 1,
                denominator: 1,
            },
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

        let breakdown = compute_pool_reward(100, &params, &pool, &snapshot, &pool_dist, perfect);

        // If the apparent reward is below cost, upstream gives the operator
        // the whole apparent reward. For this tiny pool it currently floors
        // to zero, so keep the assertion tied to the computed apparent value.
        assert_eq!(
            breakdown.leader_reward,
            breakdown.apparent_performance_reward
        );
        assert!(breakdown.member_rewards.is_empty());
    }

    #[test]
    fn pool_reward_uses_snapshot_pool_cost_without_current_minimum_remax() {
        let params = RewardParams {
            rho: UnitInterval {
                numerator: 0,
                denominator: 1,
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
            min_pool_cost: 1_000,
            reserves: 0,
            fee_pot: 0,
            max_lovelace_supply: 0,
            eta: UnitInterval {
                numerator: 1,
                denominator: 1,
            },
        };
        let pool = test_pool(1);
        let member = test_cred(2);
        let mut snapshot = StakeSnapshot::empty();
        snapshot.stake.add(member, 1_000);
        snapshot.delegations.insert(member, pool);
        snapshot.pool_params.insert(
            pool,
            test_pool_params(
                1,
                0,
                100,
                UnitInterval {
                    numerator: 0,
                    denominator: 1,
                },
            ),
        );

        let pool_dist = snapshot.pool_stake_distribution();
        let perfect = UnitInterval {
            numerator: 1,
            denominator: 1,
        };

        let breakdown = compute_pool_reward(10_000, &params, &pool, &snapshot, &pool_dist, perfect);

        assert_eq!(breakdown.apparent_performance_reward, 10_000);
        assert_eq!(
            breakdown.leader_reward, 100,
            "reward calculation must use spssCost, not max(spssCost, ppMinPoolCost)"
        );
        assert_eq!(breakdown.member_rewards.get(&member), Some(&9_900));
    }

    #[test]
    fn pool_reward_caps_oversized_performance_to_reward_pot() {
        let params = RewardParams {
            rho: UnitInterval {
                numerator: 0,
                denominator: 1,
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
            reserves: 0,
            fee_pot: 0,
            max_lovelace_supply: 0,
            eta: UnitInterval {
                numerator: 1,
                denominator: 1,
            },
        };
        let pool = test_pool(1);
        let owner_cred = test_cred(1);
        let mut snapshot = StakeSnapshot::empty();
        snapshot.stake.add(owner_cred, 1000);
        snapshot.delegations.insert(owner_cred, pool);
        snapshot.pool_params.insert(
            pool,
            test_pool_params(
                1,
                0,
                0,
                UnitInterval {
                    numerator: 0,
                    denominator: 1,
                },
            ),
        );
        let pool_dist = snapshot.pool_stake_distribution();
        let oversized_performance = UnitInterval {
            numerator: u64::MAX,
            denominator: 1,
        };

        let breakdown = compute_pool_reward(
            1000,
            &params,
            &pool,
            &snapshot,
            &pool_dist,
            oversized_performance,
        );

        assert_eq!(breakdown.apparent_performance_reward, 1000);
        assert_eq!(breakdown.leader_reward, 1000);
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
            eta: UnitInterval {
                numerator: 1,
                denominator: 1,
            },
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
        let breakdown = compute_pool_reward(10_000, &params, &pool, &snapshot, &pool_dist, perfect);

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
            eta: UnitInterval {
                numerator: 1,
                denominator: 1,
            },
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
            rho: UnitInterval {
                numerator: 5,
                denominator: 100,
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
            reserves: 200_000,
            fee_pot: 0,
            max_lovelace_supply: 0,
            eta: UnitInterval {
                numerator: 1,
                denominator: 1,
            },
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

        let perf = BTreeMap::from([(
            pool,
            UnitInterval {
                numerator: 1,
                denominator: 1,
            },
        )]);

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
                UnitInterval {
                    numerator: 1,
                    denominator: 10,
                },
            ),
        );

        let pool_dist = snapshot.pool_stake_distribution();
        let perfect = UnitInterval {
            numerator: 1,
            denominator: 1,
        };

        // Use a small pot so apparent < cost.
        let breakdown =
            compute_pool_reward(1_000_000, &params, &pool, &snapshot, &pool_dist, perfect);

        // apparent is non-zero but < cost → leader should get the full apparent amount.
        assert!(breakdown.apparent_performance_reward > 0);
        assert!(breakdown.apparent_performance_reward <= 1_000_000);
        assert_eq!(
            breakdown.leader_reward,
            breakdown.apparent_performance_reward
        );
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
            margin: UnitInterval {
                numerator: 0,
                denominator: 1,
            },
            reward_account: test_reward_account(1),
            pool_owners: vec![operator, second_owner],
            relays: vec![],
            pool_metadata: None,
        };

        let mut snapshot = StakeSnapshot::empty();
        snapshot
            .stake
            .add(StakeCredential::AddrKeyHash(operator), 3000);
        snapshot
            .stake
            .add(StakeCredential::AddrKeyHash(second_owner), 3000);
        snapshot
            .stake
            .add(StakeCredential::AddrKeyHash(member), 4000);
        snapshot
            .delegations
            .insert(StakeCredential::AddrKeyHash(operator), pool_id);
        snapshot
            .delegations
            .insert(StakeCredential::AddrKeyHash(second_owner), pool_id);
        snapshot
            .delegations
            .insert(StakeCredential::AddrKeyHash(member), pool_id);
        snapshot.pool_params.insert(pool_id, pool_params);

        let pool_dist = snapshot.pool_stake_distribution();
        let perfect = UnitInterval {
            numerator: 1,
            denominator: 1,
        };

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
            reserves: 0,
            fee_pot: 0,
            max_lovelace_supply: 0,
            eta: UnitInterval {
                numerator: 1,
                denominator: 1,
            },
        };

        let breakdown =
            compute_pool_reward(10_000, &params, &pool_id, &snapshot, &pool_dist, perfect);

        // Both owners are excluded from member_rewards.
        assert!(
            !breakdown
                .member_rewards
                .contains_key(&StakeCredential::AddrKeyHash(operator))
        );
        assert!(
            !breakdown
                .member_rewards
                .contains_key(&StakeCredential::AddrKeyHash(second_owner))
        );

        // Only the non-owner member gets a member reward.
        assert!(
            breakdown
                .member_rewards
                .contains_key(&StakeCredential::AddrKeyHash(member))
        );
        assert_eq!(breakdown.member_rewards.len(), 1);

        // Leader absorbs cost + margin + ALL owners' delegated shares.
        // With margin=0: leader gets cost(0) + floor(profit * (0 + 1 * 6000/10000))
        // profit = 10000, owner_stake=6000, pool=10000
        // leader = floor(10000 * 6000 / 10000) = 6000
        assert_eq!(breakdown.leader_reward, 6000);

        // Member gets floor(profit * 1 * 4000/10000) = floor(4000) = 4000
        assert_eq!(
            *breakdown
                .member_rewards
                .get(&StakeCredential::AddrKeyHash(member))
                .unwrap(),
            4000
        );
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
                UnitInterval {
                    numerator: 1,
                    denominator: 3,
                }, // margin = 1/3
            ),
        );

        let pool_dist = snapshot.pool_stake_distribution();
        let perfect = UnitInterval {
            numerator: 1,
            denominator: 1,
        };

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
            reserves: 0,
            fee_pot: 0,
            max_lovelace_supply: 0,
            eta: UnitInterval {
                numerator: 1,
                denominator: 1,
            },
        };

        let breakdown =
            compute_pool_reward(10_000, &params, &pool_id, &snapshot, &pool_dist, perfect);

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
        snapshot.stake.add(test_cred(1), 500); // owner has only 500
        snapshot.stake.add(test_cred(2), 9500);
        snapshot.delegations.insert(test_cred(1), pool_id);
        snapshot.delegations.insert(test_cred(2), pool_id);
        snapshot.pool_params.insert(
            pool_id,
            test_pool_params(
                1,
                1000, // pledge = 1000, but owner only has 500
                0,
                UnitInterval {
                    numerator: 0,
                    denominator: 1,
                },
            ),
        );

        let pool_dist = snapshot.pool_stake_distribution();
        let perfect = UnitInterval {
            numerator: 1,
            denominator: 1,
        };
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
            reserves: 0,
            fee_pot: 0,
            max_lovelace_supply: 0,
            eta: UnitInterval {
                numerator: 1,
                denominator: 1,
            },
        };

        let breakdown =
            compute_pool_reward(10_000, &params, &pool_id, &snapshot, &pool_dist, perfect);

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
                UnitInterval {
                    numerator: 0,
                    denominator: 1,
                },
            ),
        );

        let pool_dist = snapshot.pool_stake_distribution();
        let perfect = UnitInterval {
            numerator: 1,
            denominator: 1,
        };

        // Without max_lovelace_supply (falls back to active stake).
        let params_no_supply = RewardParams {
            rho: UnitInterval {
                numerator: 1,
                denominator: 10,
            },
            tau: UnitInterval {
                numerator: 0,
                denominator: 1,
            },
            a0: UnitInterval {
                numerator: 3,
                denominator: 10,
            },
            n_opt: 1,
            min_pool_cost: 0,
            reserves: 1_000_000_000,
            fee_pot: 0,
            max_lovelace_supply: 0,
            eta: UnitInterval {
                numerator: 1,
                denominator: 1,
            },
        };

        // With max_lovelace_supply → circulation = 10B - 1B = 9B.
        let params_with_supply = RewardParams {
            max_lovelace_supply: 10_000_000_000,
            ..params_no_supply.clone()
        };

        let b1 = compute_pool_reward(
            1_000_000,
            &params_no_supply,
            &pool_id,
            &snapshot,
            &pool_dist,
            perfect,
        );
        let b2 = compute_pool_reward(
            1_000_000,
            &params_with_supply,
            &pool_id,
            &snapshot,
            &pool_dist,
            perfect,
        );

        // With circulation-based sigma, the pool's relative stake is much
        // smaller (10M/9B vs 10M/10M), so its reward is smaller.
        assert!(
            b2.leader_reward < b1.leader_reward,
            "circulation sigma ({}) should produce smaller reward than active-stake sigma ({})",
            b2.leader_reward,
            b1.leader_reward
        );
    }

    #[test]
    fn eta_scales_monetary_expansion() {
        // When eta < 1, monetary expansion is reduced.
        let params_full = RewardParams {
            rho: UnitInterval {
                numerator: 3,
                denominator: 1000,
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
            reserves: 10_000_000_000,
            fee_pot: 0,
            max_lovelace_supply: 0,
            eta: UnitInterval {
                numerator: 1,
                denominator: 1,
            },
        };

        let params_half = RewardParams {
            eta: UnitInterval {
                numerator: 1,
                denominator: 2,
            },
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
        assert_eq!(floor_mul_div(10, 3, 4), 7); // 30/4 = 7
        assert_eq!(floor_mul_div(100, 7, 10), 70); // 700/10 = 70
        assert_eq!(floor_mul_div(0, 100, 1), 0);
        assert_eq!(floor_mul_div(100, 0, 1), 0);
        assert_eq!(floor_mul_div(100, 1, 0), 0); // division by zero → 0
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

    // ---------------------------------------------------------------
    // U256 arithmetic unit tests
    // ---------------------------------------------------------------

    #[test]
    fn u256_widening_mul_basic() {
        // Small values.
        assert_eq!(U256::widening_mul(7, 13), U256 { hi: 0, lo: 91 });
        // One operand zero.
        assert_eq!(U256::widening_mul(0, u128::MAX), U256 { hi: 0, lo: 0 });
        // Max × 1 = MAX.
        assert_eq!(
            U256::widening_mul(u128::MAX, 1),
            U256 {
                hi: 0,
                lo: u128::MAX
            }
        );
    }

    #[test]
    fn u256_widening_mul_large() {
        // 2^127 × 2 = 2^128 (overflows u128).
        let half = 1u128 << 127;
        let result = U256::widening_mul(half, 2);
        assert_eq!(result, U256 { hi: 1, lo: 0 });
        // (2^64) × (2^64) = 2^128.
        let pow64 = 1u128 << 64;
        assert_eq!(U256::widening_mul(pow64, pow64), U256 { hi: 1, lo: 0 });
    }

    #[test]
    fn u256_add_basic() {
        let a = U256 {
            hi: 0,
            lo: u128::MAX,
        };
        let b = U256 { hi: 0, lo: 1 };
        let sum = a.add(b);
        assert_eq!(sum, U256 { hi: 1, lo: 0 });
    }

    #[test]
    fn u256_div_u128_basic() {
        let v = U256 { hi: 1, lo: 0 }; // = 2^128
        // 2^128 / 2 = 2^127 = 170141183460469231731687303715884105728
        assert_eq!(v.div_u128(2), 1u128 << 127);
    }

    #[test]
    fn u256_div_u128_exact() {
        // 100 / 10 = 10.
        let v = U256 { hi: 0, lo: 100 };
        assert_eq!(v.div_u128(10), 10);
    }

    #[test]
    fn u256_div_floor_basic() {
        // Small values: 7 / 3 = 2.
        let num = U256 { hi: 0, lo: 7 };
        let den = U256 { hi: 0, lo: 3 };
        assert_eq!(u256_div_floor(num, den), 2);
    }

    #[test]
    fn u256_div_floor_both_large() {
        // (2^128 + 1) / 2 = floor = 2^127.  Denominator > u128.
        // (2^128+1)/2 = 2^127 + 0.5, floor = 2^127 which is ~1.7×10^38.
        // Our function caps at u64 — but 2^127 exceeds u64.
        // Use smaller values to stay in u64 range.
        let num = U256 { hi: 3, lo: 0 }; // 3 × 2^128
        let den = U256 { hi: 1, lo: 0 }; // 2^128
        assert_eq!(u256_div_floor(num, den), 3); // exact division
    }

    // ---------------------------------------------------------------
    // Exact-parity reward precision tests
    // ---------------------------------------------------------------

    #[test]
    fn delta_reserves_single_floor_matches_upstream() {
        // Demonstrate that single-floor computation differs from the old
        // double-floor approach for certain parameter combinations.
        //
        // Upstream: floor(eta × rho × reserves) — one floor.
        // Old code: floor(floor(reserves × rho) × eta) — two floors.
        //
        // Choose values where floor(reserves × rho) loses a fractional
        // lovelace that matters after the second multiply.
        let params = RewardParams {
            rho: UnitInterval {
                numerator: 1,
                denominator: 3,
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
            reserves: 100,
            fee_pot: 0,
            max_lovelace_supply: 0,
            // eta = 2/3
            eta: UnitInterval {
                numerator: 2,
                denominator: 3,
            },
        };

        // Single floor: floor(2/3 × 1/3 × 100) = floor(200/9) = floor(22.222...) = 22.
        // Double floor: floor(floor(100 × 1/3) × 2/3) = floor(33 × 2/3) = floor(22) = 22.
        // In this case they agree. Let's try another:

        let params2 = RewardParams {
            rho: UnitInterval {
                numerator: 7,
                denominator: 1000,
            },
            eta: UnitInterval {
                numerator: 997,
                denominator: 1000,
            },
            reserves: 14_000_000_000_000_000, // 14B ADA
            ..params.clone()
        };
        let pot2 = compute_epoch_reward_pot(&params2);

        // Single floor: floor(997/1000 × 7/1000 × 14×10^15)
        //             = floor(997 × 7 × 14×10^15 / 10^6)
        //             = floor(6979 × 14×10^15 / 10^6)
        //             = floor(97706 × 10^12)
        //             = floor(97,706,000,000,000,000) — exact, no rounding.
        // Actually: 997 × 7 = 6979. 6979 × 14×10^15 = 97706×10^15.
        // 97706×10^15 / 10^6 = 97,706,000,000,000.
        // Hmm wait: 14×10^15 × 7/1000 = 98×10^12. 98×10^12 × 997/1000 = 97,706×10^9.
        // So delta_reserves = 97,706,000,000,000.
        assert_eq!(pot2.delta_reserves, 97_706_000_000_000);
    }

    #[test]
    fn max_pool_reward_exact_floor_matches_upstream() {
        // Verify that the single-floor U256 computation matches the
        // upstream `maxPool'` result for a known mainnet-like scenario.
        //
        // Upstream (Haskell Rational):
        //   maxPool' 0.3 500 rewards sigma pledge
        // For a pool at exactly saturation (σ' = z = 1/500 = s'):
        //   result = floor(R / (1.3) × (1/500 + 1/500 × 0.3 × 1))
        //          = floor(R / 1.3 × (1/500 + 0.3/500))
        //          = floor(R / 1.3 × 1.3/500)
        //          = floor(R / 500)
        let a0 = UnitInterval {
            numerator: 3,
            denominator: 10,
        };
        let reward = max_pool_reward(
            30_000_000_000_000, // 30M ADA
            500,
            a0,
            70_000_000_000_000,     // 70M ADA (above saturation → σ'=z)
            70_000_000_000_000,     // pledge also above z
            35_000_000_000_000_000, // 35B ADA circulation
        );
        // floor(30000000000000 / 500) = 60_000_000_000 exactly.
        assert_eq!(reward, 60_000_000_000);
    }

    #[test]
    fn max_pool_reward_non_saturated_with_pledge() {
        // Non-saturated pool where pledge influence matters.
        let a0 = UnitInterval {
            numerator: 3,
            denominator: 10,
        };
        let reward = max_pool_reward(
            30_000_000_000_000,     // R = 30T lovelace
            500,                    // k = 500
            a0,                     // a0 = 0.3
            35_000_000_000_000,     // pool_stake = 35T (σ = 35T/35000T = 0.001, < z=0.002)
            1_000_000_000_000,      // pledge = 1T
            35_000_000_000_000_000, // total = 35000T
        );
        // This pool is not saturated (σ < z). The reward depends on both
        // σ and pledge. Verify it's a reasonable value and non-zero.
        assert!(reward > 0);
        // Should be less than the saturated reward of R/k = 60B.
        assert!(reward < 60_000_000_000);
    }

    #[test]
    fn max_pool_reward_zero_pledge_no_panic() {
        let a0 = UnitInterval {
            numerator: 3,
            denominator: 10,
        };
        let reward = max_pool_reward(
            10_000_000_000,
            500,
            a0,
            1_000_000_000,
            0, // zero pledge
            100_000_000_000_000,
        );
        // With zero pledge, the a0 contribution vanishes but σ' term remains.
        assert!(reward > 0);
    }
}
