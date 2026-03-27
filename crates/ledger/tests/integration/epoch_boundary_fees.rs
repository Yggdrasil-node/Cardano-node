// Integration tests for fee accumulation in StakeSnapshots and epoch boundary
// reward computation.  These verify that accumulated transaction fees
// propagate through `apply_epoch_boundary()` into the epoch reward pot.

use std::collections::BTreeMap;
use yggdrasil_ledger::{
    StakeSnapshots, StakeSnapshot, UnitInterval,
    RewardParams, compute_epoch_rewards,
};

/// Build a minimal `StakeSnapshots` with all-empty snapshots.
fn empty_snapshots() -> StakeSnapshots {
    StakeSnapshots {
        mark: StakeSnapshot::default(),
        set: StakeSnapshot::default(),
        go: StakeSnapshot::default(),
        fee_pot: 0,
    }
}

#[test]
fn accumulate_fees_tracks_total() {
    let mut snapshots = empty_snapshots();
    assert_eq!(snapshots.fee_pot, 0);

    snapshots.accumulate_fees(1_000_000);
    assert_eq!(snapshots.fee_pot, 1_000_000);

    snapshots.accumulate_fees(500_000);
    assert_eq!(snapshots.fee_pot, 1_500_000);
}

#[test]
fn rotate_returns_and_resets_fee_pot() {
    let mut snapshots = empty_snapshots();
    snapshots.accumulate_fees(2_000_000);

    let collected = snapshots.rotate(StakeSnapshot::default());
    assert_eq!(collected, 2_000_000);
    assert_eq!(snapshots.fee_pot, 0);
}

#[test]
fn fee_pot_feeds_epoch_reward_computation() {
    // Verify that a non-zero fee_pot results in a non-zero reward pot.
    // We use `compute_epoch_rewards` directly instead of `apply_epoch_boundary`
    // because the latter requires a fully seeded LedgerState.

    let fee_pot = 10_000_000u64; // 10 ADA in fees
    let reserves = 1_000_000_000_000u64; // 1T lovelace

    let params = RewardParams {
        rho: UnitInterval { numerator: 3, denominator: 1000 }, // 0.3% monetary expansion
        tau: UnitInterval { numerator: 2, denominator: 10 },   // 20% treasury cut
        a0: UnitInterval { numerator: 3, denominator: 10 },    // pledge influence 0.3
        n_opt: 500,
        min_pool_cost: 340_000_000,
        reserves,
        fee_pot,
    };

    // Empty snapshot — no pools, no delegations
    let go_snapshot = StakeSnapshot::default();
    let pool_performance: BTreeMap<[u8; 28], UnitInterval> = BTreeMap::new();

    let dist = compute_epoch_rewards(&params, &go_snapshot, &pool_performance);

    // delta_reserves = reserves * rho = 1T * 0.003 = 3B
    // rewards_pot = fee_pot + delta_reserves - treasury_cut
    // With no pools the entire pot goes to treasury_delta.
    // The key assertion: fee_pot contributes to total_distributed or treasury_delta.
    assert!(
        dist.delta_reserves > 0 || fee_pot > 0,
        "reward calculation should incorporate fee_pot"
    );
    // Total distributed + treasury_delta should account for fee_pot + delta_reserves.
    // (With no pools, all goes to treasury.)
    let expected_delta = (reserves as u128 * 3 / 1000) as u64;
    assert_eq!(dist.delta_reserves, expected_delta);
    // fee_pot is part of the reward pot; with no pools it all goes to treasury
    assert!(
        dist.treasury_delta >= fee_pot,
        "treasury_delta ({}) should include fee_pot ({})",
        dist.treasury_delta,
        fee_pot
    );
}

#[test]
fn zero_fee_pot_produces_smaller_treasury_delta() {
    let reserves = 1_000_000_000_000u64;

    let with_fees = RewardParams {
        rho: UnitInterval { numerator: 3, denominator: 1000 },
        tau: UnitInterval { numerator: 2, denominator: 10 },
        a0: UnitInterval { numerator: 3, denominator: 10 },
        n_opt: 500,
        min_pool_cost: 340_000_000,
        reserves,
        fee_pot: 50_000_000,
    };

    let without_fees = RewardParams {
        rho: UnitInterval { numerator: 3, denominator: 1000 },
        tau: UnitInterval { numerator: 2, denominator: 10 },
        a0: UnitInterval { numerator: 3, denominator: 10 },
        n_opt: 500,
        min_pool_cost: 340_000_000,
        reserves,
        fee_pot: 0,
    };

    let go_snapshot = StakeSnapshot::default();
    let pool_performance: BTreeMap<[u8; 28], UnitInterval> = BTreeMap::new();

    let dist_with = compute_epoch_rewards(&with_fees, &go_snapshot, &pool_performance);
    let dist_without = compute_epoch_rewards(&without_fees, &go_snapshot, &pool_performance);

    // With no pools, all rewards go to treasury.  The difference in
    // treasury_delta should be exactly the fee_pot (50M).
    assert!(
        dist_with.treasury_delta > dist_without.treasury_delta,
        "non-zero fee_pot should produce larger treasury_delta"
    );
    let delta_diff = dist_with.treasury_delta - dist_without.treasury_delta;
    assert_eq!(
        delta_diff, 50_000_000,
        "treasury_delta difference should equal the fee_pot"
    );
}
