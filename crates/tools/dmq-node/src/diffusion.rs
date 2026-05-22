//! DMQ diffusion-layer types.
//!
//! ## Naming parity
//!
//! **Strict mirror:** none. Yggdrasil-side module for the upstream
//! `DMQ/Diffusion/` subtree. Ports the data types of
//! `Diffusion/NodeKernel/Types.hs` — `PoolId`, `StakeSnapshot`,
//! `PoolValidationCtx`, the `StakePools` stake-pool monitoring
//! record, and the `NodeKernel` itself. The rest of `Diffusion/*`
//! (the mux bundles, the `run()` event loop) lands later in the
//! Option A `run()` integration arc.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use yggdrasil_network::LedgerPeerSnapshot;

use crate::inbound_v2::SharedTxState;
use crate::keep_alive::{KeepAliveRegistry, new_keep_alive_registry};
use crate::mempool::MempoolSeq;
use crate::peer_sharing::{
    PS_POLICY_PEER_SHARE_MAX_PEERS, PS_POLICY_PEER_SHARE_STICKY_TIME, PeerSharingAPI,
    PeerSharingRegistry, PublicPeerSelectionState, new_peer_sharing_api, new_peer_sharing_registry,
};
use crate::protocol::sig_submission::{Sig, SigId};
use crate::registry::{TxChannelsVar, TxMempoolSem, new_tx_channels_var};

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

/// The stake-pool monitoring state the DMQ `NodeKernel` holds.
///
/// Mirror of upstream `data StakePools m`
/// (`Diffusion/NodeKernel/Types.hs`). The upstream
/// `withPoolValidationCtx` field is a rank-2 polymorphic closure —
/// Rust cannot carry that as a struct field, so it is modelled as a
/// method landing with the `NodeKernel` assembly (it also needs the
/// kernel-level next-epoch / ocert-counter state).
#[derive(Clone, Debug, Default)]
pub struct StakePools {
    /// Per-pool stake snapshot obtained via the local-state-query
    /// client (`stakePoolsVar`).
    pub stake_pools_var: Arc<Mutex<BTreeMap<PoolId, StakeSnapshot>>>,
    /// Big ledger peers that advertise SRV endpoints
    /// (`ledgerBigPeersVar`).
    pub ledger_big_peers_var: Arc<Mutex<Option<LedgerPeerSnapshot>>>,
    /// All ledger peers, restricted to SRV endpoints
    /// (`ledgerPeersVar`).
    pub ledger_peers_var: Arc<Mutex<Option<LedgerPeerSnapshot>>>,
}

impl StakePools {
    /// A fresh, empty stake-pool monitoring state.
    pub fn new() -> StakePools {
        StakePools::default()
    }
}

/// The DMQ node's shared runtime state — the registries, mempool, and
/// inbound-V2 state the mini-protocol applications operate on.
///
/// Mirror of upstream `data NodeKernel crypto ntnAddr m`
/// (`Diffusion/NodeKernel/Types.hs`). Generic over the node-to-node
/// peer address and the connection-id key; concrete over the DMQ
/// `SigId` / `Sig`.
#[derive(Clone, Debug)]
pub struct NodeKernel<NtnAddr: Ord, ConnId: Ord> {
    /// The keepalive registry (upstream `fetchClientRegistry`). The
    /// R813 resolution: `BlockFetch.ClientRegistry` re-exports
    /// `module KeepAlive`, and dmq-node uses this field exclusively
    /// via `bracketKeepAliveClient`, so it is a `KeepAliveRegistry`.
    pub fetch_client_registry: KeepAliveRegistry<ConnId>,
    /// The peer-sharing registry (`peerSharingRegistry`).
    pub peer_sharing_registry: PeerSharingRegistry<NtnAddr>,
    /// The peer-sharing API (`peerSharingAPI`).
    pub peer_sharing_api: PeerSharingAPI<NtnAddr>,
    /// The diffused-signature mempool (`mempool`).
    pub mempool: Arc<Mutex<MempoolSeq<SigId, Sig>>>,
    /// The inbound-V2 per-peer decision channels (`sigChannelVar`).
    pub sig_channel_var: TxChannelsVar<NtnAddr>,
    /// The mempool-access semaphore (`sigMempoolSem`).
    pub sig_mempool_sem: TxMempoolSem,
    /// The inbound-V2 shared tx-submission state
    /// (`sigSharedTxStateVar`).
    pub sig_shared_tx_state_var: Arc<Mutex<SharedTxState<NtnAddr>>>,
    /// The stake-pool monitoring state (`stakePools`).
    pub stake_pools: StakePools,
    /// The next epoch-boundary time (POSIX seconds), for clock-skew
    /// handling (`nextEpochVar`).
    pub next_epoch_var: Arc<Mutex<Option<u64>>>,
}

/// Construct a fresh [`NodeKernel`] with empty registries and a
/// PRNG-seeded inbound-V2 state.
///
/// Mirror of upstream `newNodeKernel`. The upstream `splitGen` is a
/// yggdrasil-side splitmix split of the `u64` seed — it only seeds
/// the peer-ordering PRNGs and has no wire effect.
pub fn new_node_kernel<NtnAddr: Ord, ConnId: Ord>(rng: u64) -> NodeKernel<NtnAddr, ConnId> {
    let rng_shared = rng;
    let rng_peer_share = rng.wrapping_add(0x9E37_79B9_7F4A_7C15);
    NodeKernel {
        fetch_client_registry: new_keep_alive_registry(),
        peer_sharing_registry: new_peer_sharing_registry(),
        peer_sharing_api: new_peer_sharing_api(
            Arc::new(Mutex::new(PublicPeerSelectionState::empty())),
            rng_peer_share,
            PS_POLICY_PEER_SHARE_STICKY_TIME,
            PS_POLICY_PEER_SHARE_MAX_PEERS,
        ),
        mempool: Arc::new(Mutex::new(MempoolSeq::empty())),
        sig_channel_var: new_tx_channels_var(),
        sig_mempool_sem: TxMempoolSem::new(),
        sig_shared_tx_state_var: Arc::new(Mutex::new(SharedTxState {
            peer_rng: rng_shared,
            ..SharedTxState::default()
        })),
        stake_pools: StakePools::new(),
        next_epoch_var: Arc::new(Mutex::new(None)),
    }
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

    #[test]
    fn stake_pools_new_is_empty() {
        let pools = StakePools::new();
        assert!(
            pools
                .stake_pools_var
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_empty()
        );
        assert!(
            pools
                .ledger_big_peers_var
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_none()
        );
        assert!(
            pools
                .ledger_peers_var
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_none()
        );
    }

    #[test]
    fn stake_pools_var_records_a_pool_snapshot() {
        let pools = StakePools::new();
        let pool = PoolId([0x22; 28]);
        pools
            .stake_pools_var
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                pool.clone(),
                StakeSnapshot {
                    mark_pool: 1,
                    set_pool: 2,
                    go_pool: 3,
                },
            );
        let guard = pools
            .stake_pools_var
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        assert_eq!(guard.get(&pool).map(|s| s.go_pool), Some(3));
    }

    #[test]
    fn new_node_kernel_starts_empty() {
        let kernel: NodeKernel<String, String> = new_node_kernel(0x42);
        assert!(
            kernel
                .mempool
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .read()
                .next()
                .is_none()
        );
        assert!(
            kernel
                .sig_channel_var
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .tx_channel_map
                .is_empty()
        );
        assert_eq!(
            *kernel
                .next_epoch_var
                .lock()
                .unwrap_or_else(|e| e.into_inner()),
            None
        );
        // The inbound-V2 state is seeded with the kernel's PRNG.
        assert_eq!(
            kernel
                .sig_shared_tx_state_var
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .peer_rng,
            0x42
        );
    }

    #[test]
    fn new_node_kernel_carries_the_peer_share_policy() {
        let kernel: NodeKernel<String, String> = new_node_kernel(1);
        assert_eq!(
            kernel.peer_sharing_api.policy_peer_share_max_peers,
            crate::peer_sharing::PS_POLICY_PEER_SHARE_MAX_PEERS
        );
        assert!(
            kernel
                .stake_pools
                .stake_pools_var
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_empty()
        );
    }
}
