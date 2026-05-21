//! DMQ diffusion-layer types.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side module for the upstream
//! `DMQ/Diffusion/` subtree. This slice ports the self-contained
//! validation-context data types from `Diffusion/NodeKernel/Types.hs`
//! — `PoolId`, `StakeSnapshot`, `PoolValidationCtx`. The runtime-heavy
//! `NodeKernel` / `StakePools` records (STM vars, fetch-client and
//! peer-sharing registries) and the rest of `Diffusion/*` land with
//! the deferred Diffusion-wiring sub-arc.

use std::collections::BTreeMap;

/// A stake-pool identifier — the 28-byte Blake2b-224 hash of the
/// pool's cold verification key.
///
/// Upstream `type PoolId = Ledger.KeyHash Ledger.StakePool`.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct PoolId(pub [u8; 28]);

/// A stake pool's stake across the three ledger snapshots.
///
/// Upstream `Cardano.Ledger.Api.State.Query.StakeSnapshot`. This is
/// the minimal projection the `SigSubmission` validator needs — the
/// per-pool mark / set / go active stake, in lovelace.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StakeSnapshot {
    /// `ssMarkPool` — the pool's stake in the mark snapshot.
    pub mark_pool: u64,
    /// `ssSetPool` — the pool's stake in the set snapshot.
    pub set_pool: u64,
    /// `ssGoPool` — the pool's stake in the go snapshot.
    pub go_pool: u64,
}

/// Context for validating a DMQ signature's issuing pool.
///
/// Upstream `data PoolValidationCtx` (`Diffusion/NodeKernel/Types.hs`)
/// — acquired and updated under STM per signature batch by the
/// validator. The default value (no epoch, empty maps) is the
/// not-yet-initialized state.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PoolValidationCtx {
    /// `vctxEpoch` — POSIX time (seconds) of the next epoch boundary,
    /// for clock-skew handling. `None` until the first
    /// local-state-query has populated the context.
    pub epoch: Option<u64>,
    /// `vctxStakeMap` — per-pool stake snapshots, for pool-eligibility
    /// checks.
    pub stake_map: BTreeMap<PoolId, StakeSnapshot>,
    /// `vctxOcertMap` — last-seen operational-certificate counter per
    /// pool, for the monotonicity check.
    pub ocert_map: BTreeMap<PoolId, u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_validation_ctx_default_is_uninitialized() {
        let ctx = PoolValidationCtx::default();
        assert_eq!(ctx.epoch, None);
        assert!(ctx.stake_map.is_empty());
        assert!(ctx.ocert_map.is_empty());
    }

    #[test]
    fn pool_validation_ctx_carries_stake_and_ocert_maps() {
        let pool = PoolId([0x11; 28]);
        let mut ctx = PoolValidationCtx {
            epoch: Some(1_700_000_000),
            ..PoolValidationCtx::default()
        };
        ctx.stake_map.insert(
            pool.clone(),
            StakeSnapshot {
                mark_pool: 10,
                set_pool: 20,
                go_pool: 30,
            },
        );
        ctx.ocert_map.insert(pool.clone(), 7);
        assert_eq!(ctx.stake_map.get(&pool).map(|s| s.set_pool), Some(20));
        assert_eq!(ctx.ocert_map.get(&pool).copied(), Some(7));
        assert_eq!(ctx.epoch, Some(1_700_000_000));
    }
}
