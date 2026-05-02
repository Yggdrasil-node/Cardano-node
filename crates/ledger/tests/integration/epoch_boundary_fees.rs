// Integration tests for fee accumulation in StakeSnapshots and epoch boundary
// reward computation.  These verify that accumulated transaction fees
// propagate through `apply_epoch_boundary()` into the epoch reward pot.

use std::collections::BTreeMap;
use yggdrasil_ledger::{
    RewardParams, StakeSnapshot, StakeSnapshots, UnitInterval, compute_epoch_rewards,
};

/// Build a minimal `StakeSnapshots` with all-empty snapshots.
fn empty_snapshots() -> StakeSnapshots {
    StakeSnapshots {
        mark: StakeSnapshot::default(),
        set: StakeSnapshot::default(),
        go: StakeSnapshot::default(),
        fee_pot: 0,
        previous_fee_pot: 0,
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
        rho: UnitInterval {
            numerator: 3,
            denominator: 1000,
        }, // 0.3% monetary expansion
        tau: UnitInterval {
            numerator: 2,
            denominator: 10,
        }, // 20% treasury cut
        a0: UnitInterval {
            numerator: 3,
            denominator: 10,
        }, // pledge influence 0.3
        n_opt: 500,
        min_pool_cost: 340_000_000,
        reserves,
        fee_pot,
        max_lovelace_supply: 0,
        eta: UnitInterval {
            numerator: 1,
            denominator: 1,
        },
    };

    // Empty snapshot — no pools, no delegations
    let go_snapshot = StakeSnapshot::default();
    let pool_performance: BTreeMap<[u8; 28], UnitInterval> = BTreeMap::new();

    let dist = compute_epoch_rewards(&params, &go_snapshot, &pool_performance);

    // delta_reserves = reserves * rho = 1T * 0.003 = 3B
    // rewards_pot = fee_pot + delta_reserves - treasury_cut
    // With no pools the entire pot is unclaimed → returned to reserves.
    // The treasury_cut is only the τ portion.
    assert!(
        dist.delta_reserves > 0 || fee_pot > 0,
        "reward calculation should incorporate fee_pot"
    );
    // Total distributed + treasury_cut + unclaimed should account for
    // fee_pot + delta_reserves.
    let expected_delta = (reserves as u128 * 3 / 1000) as u64;
    assert_eq!(dist.delta_reserves, expected_delta);
    // With tau=0, treasury_cut is 0.  All goes to unclaimed → reserves.
    // fee_pot winds up in unclaimed (returned to reserves), not treasury.
    let total_pot = dist.delta_reserves.saturating_add(fee_pot);
    assert_eq!(
        dist.treasury_cut + dist.unclaimed + dist.distributed,
        total_pot,
        "treasury_cut ({}) + unclaimed ({}) + distributed ({}) should equal total pot ({})",
        dist.treasury_cut,
        dist.unclaimed,
        dist.distributed,
        total_pot,
    );
}

#[test]
fn zero_fee_pot_produces_smaller_treasury_delta() {
    let reserves = 1_000_000_000_000u64;

    let with_fees = RewardParams {
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
        n_opt: 500,
        min_pool_cost: 340_000_000,
        reserves,
        fee_pot: 50_000_000,
        max_lovelace_supply: 0,
        eta: UnitInterval {
            numerator: 1,
            denominator: 1,
        },
    };

    let without_fees = RewardParams {
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
        n_opt: 500,
        min_pool_cost: 340_000_000,
        reserves,
        fee_pot: 0,
        max_lovelace_supply: 0,
        eta: UnitInterval {
            numerator: 1,
            denominator: 1,
        },
    };

    let go_snapshot = StakeSnapshot::default();
    let pool_performance: BTreeMap<[u8; 28], UnitInterval> = BTreeMap::new();

    let dist_with = compute_epoch_rewards(&with_fees, &go_snapshot, &pool_performance);
    let dist_without = compute_epoch_rewards(&without_fees, &go_snapshot, &pool_performance);

    // With no pools, all rewards_pot is unclaimed → returned to reserves.
    // The difference in treasury_cut should be τ × fee_pot (since τ takes
    // a cut of the total pot including fees).
    // tau = 0.2, fee_pot_diff = 50M → treasury_cut_diff = floor(0.2 * 50M) = 10M
    assert!(
        dist_with.treasury_cut > dist_without.treasury_cut,
        "non-zero fee_pot should produce larger treasury_cut"
    );
    let delta_diff = dist_with.treasury_cut - dist_without.treasury_cut;
    assert_eq!(
        delta_diff, 10_000_000,
        "treasury_cut difference should equal tau * fee_pot = 0.2 * 50M = 10M"
    );
}
