//! Stake distribution snapshots and epoch-boundary snapshot rotation.
//!
//! Implements the three-snapshot mechanism described in the Shelley formal
//! ledger specification (Section 11 — SNAP rule).  At each epoch boundary
//! the snapshots rotate:
//!
//! ```text
//! go ← set ← mark ← (computed from current UTxO + reward accounts)
//! ```
//!
//! The **set** snapshot is used for leader election in the current epoch.
//! The **go** snapshot is used for reward calculation at the next epoch
//! boundary.
//!
//! Reference: `Cardano.Ledger.Shelley.LedgerState` — `SnapShots`,
//! `SnapShot`, `Stake`, and the SNAP transition rule in the formal
//! specification.

use std::collections::BTreeMap;

use crate::cbor::{CborDecode, CborEncode, Decoder, Encoder};
use crate::error::LedgerError;
use crate::state::{PoolState, RewardAccounts, StakeCredentials};
use crate::types::{
    Address, DRep, PoolKeyHash, PoolParams, RewardAccount, StakeCredential, VrfKeyHash,
};
use crate::utxo::MultiEraUtxo;

// ---------------------------------------------------------------------------
// IndividualStake — per-credential coin amounts
// ---------------------------------------------------------------------------

/// Per-credential coin amounts aggregated from the UTxO set and reward
/// account balances.
///
/// Reference: `Stake` in `Cardano.Ledger.Shelley.LedgerState`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IndividualStake {
    stakes: BTreeMap<StakeCredential, u64>,
}

impl IndividualStake {
    /// Returns a new empty stake map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the stake for `credential`, defaulting to zero.
    pub fn get(&self, credential: &StakeCredential) -> u64 {
        self.stakes.get(credential).copied().unwrap_or(0)
    }

    /// Adds `amount` to the stake of `credential`.
    pub fn add(&mut self, credential: StakeCredential, amount: u64) {
        *self.stakes.entry(credential).or_insert(0) += amount;
    }

    /// Returns an iterator over (credential, coin) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&StakeCredential, &u64)> {
        self.stakes.iter()
    }

    /// Returns the number of credentials with non-zero stake.
    pub fn len(&self) -> usize {
        self.stakes.len()
    }

    /// Returns true when no credentials carry stake.
    pub fn is_empty(&self) -> bool {
        self.stakes.is_empty()
    }
}

impl CborEncode for IndividualStake {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(self.stakes.len() as u64);
        for (cred, coin) in &self.stakes {
            enc.array(2);
            cred.encode_cbor(enc);
            enc.unsigned(*coin);
        }
    }
}

impl CborDecode for IndividualStake {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        let mut stakes = BTreeMap::new();
        for _ in 0..len {
            let inner = dec.array()?;
            if inner != 2 {
                return Err(LedgerError::CborInvalidLength {
                    expected: 2,
                    actual: inner as usize,
                });
            }
            let cred = StakeCredential::decode_cbor(dec)?;
            let coin = dec.unsigned()?;
            stakes.insert(cred, coin);
        }
        Ok(Self { stakes })
    }
}

// ---------------------------------------------------------------------------
// Delegations — credential → pool mapping
// ---------------------------------------------------------------------------

/// Delegation mapping from registered stake credentials to pools.
///
/// Reference: delegations component of `SnapShot` in the formal spec.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Delegations {
    entries: BTreeMap<StakeCredential, PoolKeyHash>,
}

impl Delegations {
    /// Creates an empty delegation map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the pool that `credential` delegates to, if any.
    pub fn get(&self, credential: &StakeCredential) -> Option<&PoolKeyHash> {
        self.entries.get(credential)
    }

    /// Sets the delegation for `credential`.
    pub fn insert(&mut self, credential: StakeCredential, pool: PoolKeyHash) {
        self.entries.insert(credential, pool);
    }

    /// Returns an iterator over (credential, pool) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&StakeCredential, &PoolKeyHash)> {
        self.entries.iter()
    }

    /// Returns the number of delegating credentials.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true when no delegations are recorded.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl CborEncode for Delegations {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(self.entries.len() as u64);
        for (cred, pool) in &self.entries {
            enc.array(2);
            cred.encode_cbor(enc);
            enc.bytes(pool);
        }
    }
}

impl CborDecode for Delegations {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        let mut entries = BTreeMap::new();
        for _ in 0..len {
            let inner = dec.array()?;
            if inner != 2 {
                return Err(LedgerError::CborInvalidLength {
                    expected: 2,
                    actual: inner as usize,
                });
            }
            let cred = StakeCredential::decode_cbor(dec)?;
            let hash_bytes = dec.bytes()?;
            if hash_bytes.len() != 28 {
                return Err(LedgerError::CborInvalidLength {
                    expected: 28,
                    actual: hash_bytes.len(),
                });
            }
            let mut pool = [0u8; 28];
            pool.copy_from_slice(hash_bytes);
            entries.insert(cred, pool);
        }
        Ok(Self { entries })
    }
}

// ---------------------------------------------------------------------------
// StakeSnapshot — point-in-time staking state
// ---------------------------------------------------------------------------

/// A point-in-time snapshot of the staking state used for leader election
/// and reward calculation.
///
/// Reference: `SnapShot` in `Cardano.Ledger.Shelley.LedgerState`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StakeSnapshot {
    /// Per-credential coin amounts (UTxO + reward balances).
    pub stake: IndividualStake,
    /// Delegation mapping (credential → pool).
    pub delegations: Delegations,
    /// Pool parameters at snapshot time.
    pub pool_params: BTreeMap<PoolKeyHash, PoolParams>,
}

impl CborEncode for StakeSnapshot {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(3);
        self.stake.encode_cbor(enc);
        self.delegations.encode_cbor(enc);
        // Pool params as sorted array of (key, params) pairs.
        enc.array(self.pool_params.len() as u64);
        for (key, params) in &self.pool_params {
            enc.array(2);
            enc.bytes(key);
            params.encode_cbor(enc);
        }
    }
}

impl CborDecode for StakeSnapshot {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 3 {
            return Err(LedgerError::CborInvalidLength {
                expected: 3,
                actual: len as usize,
            });
        }

        let stake = IndividualStake::decode_cbor(dec)?;
        let delegations = Delegations::decode_cbor(dec)?;

        let pp_len = dec.array()?;
        let mut pool_params = BTreeMap::new();
        for _ in 0..pp_len {
            let inner = dec.array()?;
            if inner != 2 {
                return Err(LedgerError::CborInvalidLength {
                    expected: 2,
                    actual: inner as usize,
                });
            }
            let hash_bytes = dec.bytes()?;
            if hash_bytes.len() != 28 {
                return Err(LedgerError::CborInvalidLength {
                    expected: 28,
                    actual: hash_bytes.len(),
                });
            }
            let mut key = [0u8; 28];
            key.copy_from_slice(hash_bytes);
            let params = PoolParams::decode_cbor(dec)?;
            pool_params.insert(key, params);
        }

        Ok(Self {
            stake,
            delegations,
            pool_params,
        })
    }
}

impl StakeSnapshot {
    /// Creates an empty snapshot.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Computes the aggregate stake per pool from the individual stake
    /// amounts and delegation mapping.
    ///
    /// Only credentials that delegate to a registered pool (present in
    /// `pool_params`) contribute to the pool's stake total.
    pub fn pool_stake_distribution(&self) -> PoolStakeDistribution {
        let mut pool_stakes: BTreeMap<PoolKeyHash, u64> = BTreeMap::new();
        let mut total_stake: u64 = 0;

        for (cred, pool_hash) in self.delegations.iter() {
            if !self.pool_params.contains_key(pool_hash) {
                continue;
            }
            let amount = self.stake.get(cred);
            if amount == 0 {
                continue;
            }
            *pool_stakes.entry(*pool_hash).or_insert(0) += amount;
            total_stake = total_stake.saturating_add(amount);
        }

        // Populate VRF key hashes from pool params for all pools that
        // have non-zero stake in this distribution.
        let mut pool_vrf_keys: BTreeMap<PoolKeyHash, VrfKeyHash> = BTreeMap::new();
        for pool_hash in pool_stakes.keys() {
            if let Some(params) = self.pool_params.get(pool_hash) {
                pool_vrf_keys.insert(*pool_hash, params.vrf_keyhash);
            }
        }

        PoolStakeDistribution {
            pool_stakes,
            pool_vrf_keys,
            total_stake,
        }
    }
}

// ---------------------------------------------------------------------------
// PoolStakeDistribution — per-pool aggregated stake
// ---------------------------------------------------------------------------

/// Aggregated per-pool stake derived from a `StakeSnapshot`.
///
/// Each pool entry carries both the aggregated stake and the pool's
/// registered VRF key hash, matching upstream `PoolDistr` which bundles
/// `IndividualPoolStake` (relative stake + `poolInfoVRF`).
///
/// Reference: `poolDistr` in `Cardano.Ledger.Shelley.LedgerState`,
/// `IndividualPoolStake` in `Cardano.Protocol.TPraos.API`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PoolStakeDistribution {
    pool_stakes: BTreeMap<PoolKeyHash, u64>,
    /// Per-pool VRF key hash — indexed by `PoolKeyHash`, sourced from
    /// `PoolParams.vrf_keyhash` at the time the snapshot is computed.
    pool_vrf_keys: BTreeMap<PoolKeyHash, VrfKeyHash>,
    total_stake: u64,
}

impl PoolStakeDistribution {
    /// Constructs a distribution from pre-computed pool stakes and total.
    pub fn from_raw(pool_stakes: BTreeMap<PoolKeyHash, u64>, total_stake: u64) -> Self {
        Self {
            pool_stakes,
            pool_vrf_keys: BTreeMap::new(),
            total_stake,
        }
    }

    /// Returns the absolute stake for `pool`, defaulting to zero.
    pub fn pool_stake(&self, pool: &PoolKeyHash) -> u64 {
        self.pool_stakes.get(pool).copied().unwrap_or(0)
    }

    /// Returns the total active stake across all pools.
    pub fn total_active_stake(&self) -> u64 {
        self.total_stake
    }

    /// Returns the relative stake for `pool` as a (numerator, denominator) pair.
    ///
    /// Returns `(0, 1)` when total stake is zero.
    pub fn relative_stake(&self, pool: &PoolKeyHash) -> (u64, u64) {
        if self.total_stake == 0 {
            return (0, 1);
        }
        (self.pool_stake(pool), self.total_stake)
    }

    /// Returns an iterator over (pool, absolute_stake) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&PoolKeyHash, &u64)> {
        self.pool_stakes.iter()
    }

    /// Returns the number of pools with non-zero stake.
    pub fn pool_count(&self) -> usize {
        self.pool_stakes.len()
    }

    /// Returns the registered VRF key hash for `pool`, if available.
    ///
    /// This is sourced from `PoolParams.vrf_keyhash` at snapshot time.
    ///
    /// Reference: `poolInfoVRF` in `IndividualPoolStake` from
    /// `Cardano.Protocol.TPraos.API`.
    pub fn pool_vrf_key_hash(&self, pool: &PoolKeyHash) -> Option<&VrfKeyHash> {
        self.pool_vrf_keys.get(pool)
    }

    /// Returns true if the pool is present in the distribution (has non-zero stake).
    pub fn contains_pool(&self, pool: &PoolKeyHash) -> bool {
        self.pool_stakes.contains_key(pool)
    }
}

// ---------------------------------------------------------------------------
// StakeSnapshots — rotating 3-snapshot container
// ---------------------------------------------------------------------------

/// The rotating three-snapshot container maintained across epoch boundaries.
///
/// At each epoch boundary the snapshots rotate:
/// ```text
/// go ← set ← mark ← (freshly computed from current state)
/// ```
///
/// * **mark** — most recently computed snapshot (this epoch boundary).
/// * **set** — previous mark; used for leader election in the current epoch.
/// * **go** — previous set; used for reward calculation.
/// * **fee_pot** — accumulated transaction fees during the current epoch,
///   to be distributed as rewards at the next epoch boundary.
///
/// Reference: `SnapShots` in `Cardano.Ledger.Shelley.LedgerState`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StakeSnapshots {
    /// Most recently computed snapshot (current epoch boundary).
    pub mark: StakeSnapshot,
    /// Previous mark — used for leader election in the current epoch.
    pub set: StakeSnapshot,
    /// Previous set — used for reward calculation.
    pub go: StakeSnapshot,
    /// Accumulated transaction fees for the current epoch.
    pub fee_pot: u64,
}

impl Default for StakeSnapshots {
    fn default() -> Self {
        Self {
            mark: StakeSnapshot::empty(),
            set: StakeSnapshot::empty(),
            go: StakeSnapshot::empty(),
            fee_pot: 0,
        }
    }
}

impl StakeSnapshots {
    /// Creates a fresh container with three empty snapshots.
    pub fn new() -> Self {
        Self::default()
    }

    /// Rotates snapshots at an epoch boundary:
    ///
    /// 1. `go` ← `set`
    /// 2. `set` ← `mark`
    /// 3. `mark` ← `new_mark` (freshly computed)
    /// 4. `fee_pot` is returned (the fees to be distributed) and reset to zero.
    ///
    /// Reference: SNAP transition rule — `snapTransition`.
    pub fn rotate(&mut self, new_mark: StakeSnapshot) -> u64 {
        self.go = std::mem::replace(&mut self.set, std::mem::replace(&mut self.mark, new_mark));
        std::mem::take(&mut self.fee_pot)
    }

    /// Adds `fees` to the current-epoch fee pot.
    ///
    /// Called after each block is applied during the epoch.
    pub fn accumulate_fees(&mut self, fees: u64) {
        self.fee_pot = self.fee_pot.saturating_add(fees);
    }
}

impl CborEncode for StakeSnapshots {
    fn encode_cbor(&self, enc: &mut Encoder) {
        enc.array(4);
        self.mark.encode_cbor(enc);
        self.set.encode_cbor(enc);
        self.go.encode_cbor(enc);
        enc.unsigned(self.fee_pot);
    }
}

impl CborDecode for StakeSnapshots {
    fn decode_cbor(dec: &mut Decoder<'_>) -> Result<Self, LedgerError> {
        let len = dec.array()?;
        if len != 4 {
            return Err(LedgerError::CborInvalidLength {
                expected: 4,
                actual: len as usize,
            });
        }
        let mark = StakeSnapshot::decode_cbor(dec)?;
        let set = StakeSnapshot::decode_cbor(dec)?;
        let go = StakeSnapshot::decode_cbor(dec)?;
        let fee_pot = dec.unsigned()?;
        Ok(Self {
            mark,
            set,
            go,
            fee_pot,
        })
    }
}

// ---------------------------------------------------------------------------
// Snapshot construction from live state
// ---------------------------------------------------------------------------

/// Computes a fresh `StakeSnapshot` from the current UTxO set, registered
/// stake credentials, reward account balances, and pool registry.
///
/// This walks the UTxO set once, extracting the staking credential from
/// each output address (base addresses only — enterprise, pointer, Byron,
/// and reward addresses do not contribute to the UTxO-based stake).
/// Reward account balances are then added on top.
///
/// Reference: SNAP rule — `stakeDistr` in the formal specification.
pub fn compute_stake_snapshot(
    utxo: &MultiEraUtxo,
    stake_creds: &StakeCredentials,
    reward_accounts: &RewardAccounts,
    pool_state: &PoolState,
) -> StakeSnapshot {
    let mut stake = IndividualStake::new();
    let mut delegations = Delegations::new();
    let mut pool_params_map: BTreeMap<PoolKeyHash, PoolParams> = BTreeMap::new();

    // 1. Walk the UTxO to accumulate per-credential stake.
    for (_txin, txout) in utxo.iter() {
        let addr_bytes = txout.address();
        if let Some(Address::Base(base)) = Address::from_bytes(addr_bytes) {
            // Only base addresses carry a staking credential that
            // participates in the stake distribution.
            if stake_creds.is_registered(&base.staking) {
                stake.add(base.staking, txout.coin());
            }
        }
    }

    // 2. Add reward account balances.
    for (account, state) in reward_accounts.iter() {
        let balance = state.balance();
        if balance > 0 && stake_creds.is_registered(&account.credential) {
            stake.add(account.credential, balance);
        }
    }

    // 3. Collect delegations from registered stake credentials.
    for (cred, cred_state) in stake_creds.iter() {
        if let Some(pool_hash) = cred_state.delegated_pool() {
            delegations.insert(*cred, pool_hash);
        }
    }

    // 4. Snapshot pool parameters — include ALL registered pools, even those
    //    that have announced future retirement.  Upstream `stakeDistr` does
    //    not filter by retirement epoch; pools participate in leader election
    //    and earn rewards until actually retired by `process_retirements()`
    //    at the epoch boundary.  Reference: `Cardano.Ledger.Shelley.
    //    LedgerState.stakeDistr`.
    for (pool_hash, registered_pool) in pool_state.iter() {
        pool_params_map.insert(*pool_hash, registered_pool.params().clone());
    }

    StakeSnapshot {
        stake,
        delegations,
        pool_params: pool_params_map,
    }
}

// ---------------------------------------------------------------------------
// DRep stake distribution
// ---------------------------------------------------------------------------

/// Computes the aggregate stake delegated to each DRep from the current
/// UTxO set, reward account balances, credential-level DRep delegations,
/// and per-credential governance proposal deposits.
///
/// Only credentials whose `delegated_drep` is `Some` contribute.
/// The `AlwaysAbstain` and `AlwaysNoConfidence` sentinels are included
/// because the tally engine handles them generically.
///
/// Proposal deposits are added to each credential's voting weight,
/// matching upstream `computeDRepDistr` in
/// `Cardano.Ledger.Conway.Governance.DRepPulser`:
///   `stakeAndDeposits = fold $ mInstantStake <> mProposalDeposit`
///
/// Reference: `Cardano.Ledger.Conway.Rules.Ratify` — the DRep
/// stake-weighted tally uses a per-DRep aggregate derived from the
/// current stake snapshot and credential-level DRep delegations.
pub fn compute_drep_stake_distribution(
    snapshot: &StakeSnapshot,
    stake_creds: &StakeCredentials,
    proposal_deposits: &BTreeMap<StakeCredential, u64>,
) -> BTreeMap<DRep, u64> {
    let mut drep_stakes: BTreeMap<DRep, u64> = BTreeMap::new();

    for (cred, cred_state) in stake_creds.iter() {
        if let Some(drep) = cred_state.delegated_drep() {
            let stake = snapshot.stake.get(cred);
            let deposit = proposal_deposits.get(cred).copied().unwrap_or(0);
            let amount = stake.saturating_add(deposit);
            if amount > 0 {
                *drep_stakes.entry(drep).or_insert(0) += amount;
            }
        }
    }

    drep_stakes
}

/// Augments a pool stake distribution with per-credential governance
/// proposal deposits for credentials delegated to pools.
///
/// Upstream `computeDRepDistr` adds proposal deposits to the SPO pool
/// distribution for credentials delegated to a stake pool:
///   `addToPoolDistr accountState mProposalDeposit distr`
///
/// Regular stake and rewards are already in the pool distribution from
/// the SNAP snapshot; only proposal deposits need to be added here.
///
/// Reference: `Cardano.Ledger.Conway.Governance.DRepPulser.computeDRepDistr`.
pub fn augment_pool_dist_with_proposal_deposits(
    pool_dist: &mut PoolStakeDistribution,
    stake_creds: &StakeCredentials,
    proposal_deposits: &BTreeMap<StakeCredential, u64>,
) {
    for (cred, deposit) in proposal_deposits {
        if *deposit == 0 {
            continue;
        }
        // Only add if the credential is delegated to a pool.
        if let Some(cred_state) = stake_creds.get(cred) {
            if let Some(pool_hash) = cred_state.delegated_pool() {
                if let Some(pool_stake) = pool_dist.pool_stakes.get_mut(&pool_hash) {
                    *pool_stake = pool_stake.saturating_add(*deposit);
                    pool_dist.total_stake = pool_dist.total_stake.saturating_add(*deposit);
                }
            }
        }
    }
}

/// Computes per-credential proposal deposit totals from active governance
/// actions.
///
/// Upstream `proposalsDeposits` in `Cardano.Ledger.Conway.Governance.Proposals`
/// aggregates proposal deposits by the staking credential of each proposal's
/// return address.
pub fn compute_proposal_deposits_per_credential(
    governance_actions: &BTreeMap<
        crate::eras::conway::GovActionId,
        crate::state::GovernanceActionState,
    >,
) -> BTreeMap<StakeCredential, u64> {
    let mut deposits: BTreeMap<StakeCredential, u64> = BTreeMap::new();

    for state in governance_actions.values() {
        let proposal = state.proposal();
        if proposal.deposit == 0 {
            continue;
        }
        if let Some(ra) = RewardAccount::from_bytes(&proposal.reward_account) {
            *deposits.entry(ra.credential).or_insert(0) += proposal.deposit;
        }
    }

    deposits
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_cred(b: u8) -> StakeCredential {
        StakeCredential::AddrKeyHash([b; 28])
    }

    fn test_pool(b: u8) -> PoolKeyHash {
        [b; 28]
    }

    fn test_pool_params(b: u8) -> PoolParams {
        use crate::types::{RewardAccount, UnitInterval};
        PoolParams {
            operator: test_pool(b),
            vrf_keyhash: [b; 32],
            pledge: 100_000_000,
            cost: 340_000_000,
            margin: UnitInterval {
                numerator: 1,
                denominator: 100,
            },
            reward_account: RewardAccount {
                network: 1,
                credential: test_cred(b),
            },
            pool_owners: vec![[b; 28]],
            relays: vec![],
            pool_metadata: None,
        }
    }

    #[test]
    fn individual_stake_add_and_get() {
        let mut stake = IndividualStake::new();
        let cred = test_cred(1);

        assert_eq!(stake.get(&cred), 0);
        stake.add(cred, 100);
        assert_eq!(stake.get(&cred), 100);
        stake.add(cred, 50);
        assert_eq!(stake.get(&cred), 150);
        assert_eq!(stake.len(), 1);
    }

    #[test]
    fn delegations_insert_and_get() {
        let mut delegations = Delegations::new();
        let cred = test_cred(1);
        let pool = test_pool(10);

        assert!(delegations.get(&cred).is_none());
        delegations.insert(cred, pool);
        assert_eq!(delegations.get(&cred), Some(&pool));
        assert_eq!(delegations.len(), 1);
    }

    #[test]
    fn pool_stake_distribution_aggregation() {
        let mut snapshot = StakeSnapshot::empty();
        let cred_a = test_cred(1);
        let cred_b = test_cred(2);
        let pool = test_pool(10);

        snapshot.stake.add(cred_a, 1000);
        snapshot.stake.add(cred_b, 2000);
        snapshot.delegations.insert(cred_a, pool);
        snapshot.delegations.insert(cred_b, pool);
        snapshot.pool_params.insert(pool, test_pool_params(10));

        let dist = snapshot.pool_stake_distribution();
        assert_eq!(dist.pool_stake(&pool), 3000);
        assert_eq!(dist.total_active_stake(), 3000);
        assert_eq!(dist.pool_count(), 1);
    }

    #[test]
    fn pool_stake_excludes_unregistered_pools() {
        let mut snapshot = StakeSnapshot::empty();
        let cred = test_cred(1);
        let pool_registered = test_pool(10);
        let pool_unknown = test_pool(20);

        snapshot.stake.add(cred, 500);
        // Delegate to an unregistered pool — should not contribute.
        snapshot.delegations.insert(cred, pool_unknown);
        snapshot
            .pool_params
            .insert(pool_registered, test_pool_params(10));

        let dist = snapshot.pool_stake_distribution();
        assert_eq!(dist.pool_stake(&pool_unknown), 0);
        assert_eq!(dist.total_active_stake(), 0);
    }

    #[test]
    fn relative_stake_zero_total() {
        let dist = PoolStakeDistribution::default();
        let pool = test_pool(1);
        assert_eq!(dist.relative_stake(&pool), (0, 1));
    }

    #[test]
    fn relative_stake_normal() {
        let mut snapshot = StakeSnapshot::empty();
        let cred_a = test_cred(1);
        let cred_b = test_cred(2);
        let pool_a = test_pool(10);
        let pool_b = test_pool(20);

        snapshot.stake.add(cred_a, 3000);
        snapshot.stake.add(cred_b, 7000);
        snapshot.delegations.insert(cred_a, pool_a);
        snapshot.delegations.insert(cred_b, pool_b);
        snapshot.pool_params.insert(pool_a, test_pool_params(10));
        snapshot.pool_params.insert(pool_b, test_pool_params(20));

        let dist = snapshot.pool_stake_distribution();
        assert_eq!(dist.relative_stake(&pool_a), (3000, 10000));
        assert_eq!(dist.relative_stake(&pool_b), (7000, 10000));
    }

    #[test]
    fn snapshot_rotation() {
        let mut snapshots = StakeSnapshots::new();

        // Create 3 distinguishable snapshots.
        let mut snap1 = StakeSnapshot::empty();
        snap1.stake.add(test_cred(1), 100);
        let mut snap2 = StakeSnapshot::empty();
        snap2.stake.add(test_cred(2), 200);
        let mut snap3 = StakeSnapshot::empty();
        snap3.stake.add(test_cred(3), 300);

        // First rotation.
        snapshots.accumulate_fees(500);
        let fees = snapshots.rotate(snap1.clone());
        assert_eq!(fees, 500);
        assert_eq!(snapshots.fee_pot, 0);
        assert_eq!(snapshots.mark, snap1);
        assert!(snapshots.set.stake.is_empty()); // was the initial empty
        assert!(snapshots.go.stake.is_empty());

        // Second rotation.
        snapshots.accumulate_fees(1000);
        let fees = snapshots.rotate(snap2.clone());
        assert_eq!(fees, 1000);
        assert_eq!(snapshots.mark, snap2);
        assert_eq!(snapshots.set, snap1);
        assert!(snapshots.go.stake.is_empty());

        // Third rotation.
        let fees = snapshots.rotate(snap3.clone());
        assert_eq!(fees, 0);
        assert_eq!(snapshots.mark, snap3);
        assert_eq!(snapshots.set, snap2);
        assert_eq!(snapshots.go, snap1);
    }

    #[test]
    fn individual_stake_cbor_round_trip() {
        let mut stake = IndividualStake::new();
        stake.add(test_cred(1), 100);
        stake.add(test_cred(2), 200);

        let bytes = {
            let mut enc = Encoder::new();
            stake.encode_cbor(&mut enc);
            enc.into_bytes()
        };
        let decoded =
            IndividualStake::decode_cbor(&mut Decoder::new(&bytes)).expect("decode should succeed");
        assert_eq!(stake, decoded);
    }

    #[test]
    fn delegations_cbor_round_trip() {
        let mut delegations = Delegations::new();
        delegations.insert(test_cred(1), test_pool(10));
        delegations.insert(test_cred(2), test_pool(20));

        let bytes = {
            let mut enc = Encoder::new();
            delegations.encode_cbor(&mut enc);
            enc.into_bytes()
        };
        let decoded =
            Delegations::decode_cbor(&mut Decoder::new(&bytes)).expect("decode should succeed");
        assert_eq!(delegations, decoded);
    }

    #[test]
    fn stake_snapshot_cbor_round_trip() {
        let mut snapshot = StakeSnapshot::empty();
        snapshot.stake.add(test_cred(1), 1000);
        snapshot.delegations.insert(test_cred(1), test_pool(10));
        snapshot
            .pool_params
            .insert(test_pool(10), test_pool_params(10));

        let bytes = {
            let mut enc = Encoder::new();
            snapshot.encode_cbor(&mut enc);
            enc.into_bytes()
        };
        let decoded =
            StakeSnapshot::decode_cbor(&mut Decoder::new(&bytes)).expect("decode should succeed");
        assert_eq!(snapshot, decoded);
    }

    #[test]
    fn stake_snapshots_cbor_round_trip() {
        let mut snapshots = StakeSnapshots::new();
        let mut snap = StakeSnapshot::empty();
        snap.stake.add(test_cred(1), 500);
        snapshots.rotate(snap);
        snapshots.accumulate_fees(42);

        let bytes = {
            let mut enc = Encoder::new();
            snapshots.encode_cbor(&mut enc);
            enc.into_bytes()
        };
        let decoded =
            StakeSnapshots::decode_cbor(&mut Decoder::new(&bytes)).expect("decode should succeed");
        assert_eq!(snapshots, decoded);
    }

    // -----------------------------------------------------------------------
    // Proposal deposit voting weight tests (Gap AR/AS)
    // -----------------------------------------------------------------------

    fn make_stake_creds_with_drep(cred: StakeCredential, drep: DRep) -> StakeCredentials {
        let mut creds = StakeCredentials::new();
        creds.register(cred);
        creds.get_mut(&cred).unwrap().set_delegated_drep(Some(drep));
        creds
    }

    fn make_stake_creds_with_pool(cred: StakeCredential, pool: PoolKeyHash) -> StakeCredentials {
        let mut creds = StakeCredentials::new();
        creds.register(cred);
        creds.get_mut(&cred).unwrap().set_delegated_pool(Some(pool));
        creds
    }

    #[test]
    fn drep_distribution_includes_proposal_deposits() {
        let cred = test_cred(1);
        let drep = DRep::KeyHash([0xAA; 28]);
        let stake_creds = make_stake_creds_with_drep(cred, drep);

        let mut snapshot = StakeSnapshot::empty();
        snapshot.stake.add(cred, 1000);

        let mut proposal_deposits = BTreeMap::new();
        proposal_deposits.insert(cred, 500);

        let dist = compute_drep_stake_distribution(&snapshot, &stake_creds, &proposal_deposits);
        // Voting weight = UTxO stake (1000) + proposal deposit (500)
        assert_eq!(dist.get(&drep), Some(&1500));
    }

    #[test]
    fn drep_distribution_no_proposal_deposits() {
        let cred = test_cred(2);
        let drep = DRep::KeyHash([0xBB; 28]);
        let stake_creds = make_stake_creds_with_drep(cred, drep);

        let mut snapshot = StakeSnapshot::empty();
        snapshot.stake.add(cred, 2000);

        let empty_deposits = BTreeMap::new();

        let dist = compute_drep_stake_distribution(&snapshot, &stake_creds, &empty_deposits);
        assert_eq!(dist.get(&drep), Some(&2000));
    }

    #[test]
    fn pool_dist_augmented_with_proposal_deposits() {
        let cred = test_cred(3);
        let pool = test_pool(30);
        let stake_creds = make_stake_creds_with_pool(cred, pool);

        let mut snapshot = StakeSnapshot::empty();
        snapshot.stake.add(cred, 5000);
        snapshot.delegations.insert(cred, pool);
        snapshot.pool_params.insert(pool, test_pool_params(30));

        let mut dist = snapshot.pool_stake_distribution();
        assert_eq!(dist.pool_stake(&pool), 5000);
        assert_eq!(dist.total_active_stake(), 5000);

        let mut proposal_deposits = BTreeMap::new();
        proposal_deposits.insert(cred, 800);

        augment_pool_dist_with_proposal_deposits(&mut dist, &stake_creds, &proposal_deposits);
        // Pool stake = original (5000) + proposal deposit (800)
        assert_eq!(dist.pool_stake(&pool), 5800);
        assert_eq!(dist.total_active_stake(), 5800);
    }

    #[test]
    fn pool_dist_augment_skips_undelegated_deposits() {
        let cred = test_cred(4);
        let drep = DRep::KeyHash([0xCC; 28]);
        // Credential is DRep-delegated, NOT pool-delegated
        let stake_creds = make_stake_creds_with_drep(cred, drep);

        let mut dist = PoolStakeDistribution::default();
        let mut proposal_deposits = BTreeMap::new();
        proposal_deposits.insert(cred, 999);

        augment_pool_dist_with_proposal_deposits(&mut dist, &stake_creds, &proposal_deposits);
        // No pool delegation → pool dist unchanged
        assert_eq!(dist.total_active_stake(), 0);
    }
}
